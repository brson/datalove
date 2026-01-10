use rmx::prelude::*;
use std::path::Path;
use datalove_datalit as datalit;
use datalove_rt as rt;

/// Compare two types for structural equality, recursively comparing nested types.
fn types_equal<'db>(
    db: &'db datalit::Database,
    type1: &datalit::tycheck::Type<'db>,
    type2: &datalit::tycheck::Type<'db>,
) -> bool {
    use datalit::tycheck::Type;

    match (type1, type2) {
        (Type::Bool, Type::Bool) => true,
        (Type::U8, Type::U8) => true,
        (Type::I8, Type::I8) => true,
        (Type::U16, Type::U16) => true,
        (Type::I16, Type::I16) => true,
        (Type::U32, Type::U32) => true,
        (Type::I32, Type::I32) => true,
        (Type::U64, Type::U64) => true,
        (Type::I64, Type::I64) => true,
        (Type::F32, Type::F32) => true,
        (Type::Int, Type::Int) => true,
        (Type::String, Type::String) => true,
        (Type::Data, Type::Data) => true,
        (Type::Error, Type::Error) => true,

        (Type::AnonTuple(t1), Type::AnonTuple(t2)) => {
            let fields1 = &t1.fields;
            let fields2 = &t2.fields;
            if fields1.len() != fields2.len() {
                return false;
            }
            fields1.iter().zip(fields2.iter()).all(|(f1, f2)| {
                let heap1 = std::mem::discriminant(&f1.heap(db));
                let heap2 = std::mem::discriminant(&f2.heap(db));
                heap1 == heap2 && types_equal(db, f1.ty(db), f2.ty(db))
            })
        }

        (Type::List(t1), Type::List(t2)) => {
            let elem1 = t1.element_type;
            let elem2 = t2.element_type;
            let heap1 = std::mem::discriminant(&elem1.heap(db));
            let heap2 = std::mem::discriminant(&elem2.heap(db));
            heap1 == heap2 && types_equal(db, elem1.ty(db), elem2.ty(db))
        }

        (Type::Option(t1), Type::Option(t2)) => {
            let inner1 = t1.inner_type;
            let inner2 = t2.inner_type;
            let heap1 = std::mem::discriminant(&inner1.heap(db));
            let heap2 = std::mem::discriminant(&inner2.heap(db));
            heap1 == heap2 && types_equal(db, inner1.ty(db), inner2.ty(db))
        }

        (Type::Result(t1), Type::Result(t2)) => {
            let inner1 = t1.inner_type;
            let inner2 = t2.inner_type;
            let heap1 = std::mem::discriminant(&inner1.heap(db));
            let heap2 = std::mem::discriminant(&inner2.heap(db));
            heap1 == heap2 && types_equal(db, inner1.ty(db), inner2.ty(db))
        }

        (Type::Map(m1), Type::Map(m2)) => {
            let key1 = m1.key_type;
            let key2 = m2.key_type;
            let value1 = m1.value_type;
            let value2 = m2.value_type;
            let key_heap1 = std::mem::discriminant(&key1.heap(db));
            let key_heap2 = std::mem::discriminant(&key2.heap(db));
            let value_heap1 = std::mem::discriminant(&value1.heap(db));
            let value_heap2 = std::mem::discriminant(&value2.heap(db));
            key_heap1 == key_heap2 && value_heap1 == value_heap2
                && types_equal(db, key1.ty(db), key2.ty(db))
                && types_equal(db, value1.ty(db), value2.ty(db))
        }

        (Type::Set(s1), Type::Set(s2)) => {
            let elem1 = s1.element_type;
            let elem2 = s2.element_type;
            let heap1 = std::mem::discriminant(&elem1.heap(db));
            let heap2 = std::mem::discriminant(&elem2.heap(db));
            heap1 == heap2 && types_equal(db, elem1.ty(db), elem2.ty(db))
        }

        _ => {
            // For other types or mismatched variants, use standard equality.
            type1 == type2
        }
    }
}

/// Compile and instantiate a datalit value.
#[salsa::tracked]
fn compile<'db>(
    db: &'db dyn salsa::Database,
    source: bct::input::Source,
) -> (datalit::ast::ExprFull<'db>, datalit::resolve::ResolvedExpr<'db>, datalit::tycheck::TypecheckResult<'db>) {
    let parse_result = datalit::parser::parse(db, source);
    let parsed = parse_result.expr(db);
    let resolved = datalit::resolve::resolve_names(db, source, parsed);
    let typechecked = datalit::tycheck::type_check(db, parsed, resolved);
    (parsed, resolved, typechecked)
}

fn compile_and_instantiate<'db, 't>(
    db: &'db datalit::Database,
    rt: &rt::rust::Runtime,
    tydesc_table: &'t mut datalit::tydesc_table::TyDescTable<'db>,
    source_text: &str,
) -> Result<(datalit::instantiate2::InstantiatedValue<'t>, datalit::tycheck::TypecheckResult<'db>), String> {
    let source = bct::input::Source::new(db, source_text.S());
    let (_parsed, _resolved, typechecked) = compile(db, source);

    // Check if we have a root type.
    if typechecked.root_type(db).is_none() {
        return Err(format!("No root type for source: {}", source_text));
    }

    // Check for typecheck errors.
    let errors = typechecked.errors(db);
    if !errors.is_empty() {
        return Err(format!("Type check errors: {} error(s)", errors.len()));
    }

    let inst_value = datalit::instantiate2::instantiate_value(db, rt.handle(), tydesc_table, typechecked)
        .map_err(|e| format!("Instantiation error: {}", e))?;

    Ok((inst_value, typechecked))
}

/// Pretty-print a runtime value to a string with type hint.
fn rt_pretty_print<'db>(
    db: &'db datalit::Database,
    ty: &datalit::tycheck::TypeAndHeap<'db>,
    value_ref: *const u8,
    tydesc_ref: *const rt::rtdt::TyDesc,
) -> Result<String, String> {
    datalit::pretty::pretty_print_runtime_value(db, ty, value_ref, tydesc_ref)
}

/// Round-trip test: parse -> instantiate -> rt pretty-print -> parse -> instantiate -> rt pretty-print.
///
/// Both pretty-prints should be identical, and all three sources should typecheck identically.
fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Step 1: Parse, type check, and instantiate the original datalit.
    let db = datalit::Database::default();
    let rt_inst = rt::rust::Runtime::new();
    let mut tydesc_table = datalit::tydesc_table::TyDescTable::new(&db);
    let ((ptr1, tydesc1), tycheck1) = {
        let (inst, tycheck) = compile_and_instantiate(&db, &rt_inst, &mut tydesc_table, &source_text)?;
        ((inst.ptr, inst.tydesc.as_ptr()), tycheck)
    };

    // Extract the root type from the first typecheck.
    let type1 = tycheck1.root_type(&db).X();

    // Step 2: Pretty-print using runtime pretty printer.
    let pretty1 = rt_pretty_print(&db, &type1, ptr1, tydesc1)?;

    // Step 3: Parse, type check, and instantiate the pretty-printed output.
    let ((ptr2, tydesc2), tycheck2) = {
        let (inst, tycheck) = compile_and_instantiate(&db, &rt_inst, &mut tydesc_table, &pretty1)?;
        ((inst.ptr, inst.tydesc.as_ptr()), tycheck)
    };

    // Extract the root type from the second typecheck.
    let type2 = tycheck2.root_type(&db).X();

    // Step 4: Pretty-print again.
    let pretty2 = rt_pretty_print(&db, &type2, ptr2, tydesc2)?;

    // Step 5: Parse and typecheck the second pretty-print to get the third type.
    let source3 = bct::input::Source::new(&db, pretty2.S());
    let (_parsed3, _resolved3, tycheck3) = compile(&db, source3);
    let type3 = tycheck3.root_type(&db).X();

    // Step 6: Check that all three types and heaps are identical.
    // We compare the actual type and heap content, not salsa identity.
    let heap1 = std::mem::discriminant(&type1.heap(&db));
    let heap2 = std::mem::discriminant(&type2.heap(&db));
    let heap3 = std::mem::discriminant(&type3.heap(&db));

    let rt_handle = rt_inst.handle();

    if heap1 != heap2 || !types_equal(&db, type1.ty(&db), type2.ty(&db)) {
        unsafe {
            rt::c::dtlv_rti_any_destroy_local(rt_handle, ptr1 as *mut u8, tydesc1);
            rt::c::dtlv_rti_mem_free_local(rt_handle, tydesc1, 1, ptr1 as *mut u8);
            rt::c::dtlv_rti_any_destroy_local(rt_handle, ptr2 as *mut u8, tydesc2);
            rt::c::dtlv_rti_mem_free_local(rt_handle, tydesc2, 1, ptr2 as *mut u8);
        }
        return Err(format!(
            "Types differ between original and first pretty-print:\nOriginal: {}\nFirst:    {}",
            source_text, pretty1
        ));
    }

    if heap2 != heap3 || !types_equal(&db, type2.ty(&db), type3.ty(&db)) {
        unsafe {
            rt::c::dtlv_rti_any_destroy_local(rt_handle, ptr1 as *mut u8, tydesc1);
            rt::c::dtlv_rti_mem_free_local(rt_handle, tydesc1, 1, ptr1 as *mut u8);
            rt::c::dtlv_rti_any_destroy_local(rt_handle, ptr2 as *mut u8, tydesc2);
            rt::c::dtlv_rti_mem_free_local(rt_handle, tydesc2, 1, ptr2 as *mut u8);
        }
        return Err(format!(
            "Types differ between first and second pretty-print:\nFirst:  {}\nSecond: {}",
            pretty1, pretty2
        ));
    }

    // Step 7: Check that both pretty-prints are identical.
    if pretty1 != pretty2 {
        unsafe {
            rt::c::dtlv_rti_any_destroy_local(rt_handle, ptr1 as *mut u8, tydesc1);
            rt::c::dtlv_rti_mem_free_local(rt_handle, tydesc1, 1, ptr1 as *mut u8);
            rt::c::dtlv_rti_any_destroy_local(rt_handle, ptr2 as *mut u8, tydesc2);
            rt::c::dtlv_rti_mem_free_local(rt_handle, tydesc2, 1, ptr2 as *mut u8);
        }
        return Err(format!(
            "Pretty-prints differ:\nFirst:  {}\nSecond: {}",
            pretty1, pretty2
        ));
    }

    // Clean up instantiated values before shutdown (Runtime drops on scope exit).
    unsafe {
        rt::c::dtlv_rti_any_destroy_local(rt_handle, ptr1 as *mut u8, tydesc1);
        rt::c::dtlv_rti_mem_free_local(rt_handle, tydesc1, 1, ptr1 as *mut u8);
        rt::c::dtlv_rti_any_destroy_local(rt_handle, ptr2 as *mut u8, tydesc2);
        rt::c::dtlv_rti_mem_free_local(rt_handle, tydesc2, 1, ptr2 as *mut u8);
    }
    Ok(pretty1)
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("roundtrip")
        .file_extension("dlt")
        .run();
}
