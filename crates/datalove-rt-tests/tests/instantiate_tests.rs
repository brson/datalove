//! Tests for descriptors built at run time from templates, the payload
//! queries beside them, and the shadow stack.
//!
//! A template is made here the way the compiler will make one: by taking the
//! descriptor of a concrete type and putting a `Param` wherever a chosen part
//! stood. Instantiating it with that part has to give back a descriptor equal
//! in every field to the concrete one, names, offsets and flags included.

use rmx::prelude::*;

use datalove_datalit::Database;
use datalove_datalit::tydesc_table::TyDescTable;
use datalove_rt::c::{self, RtHeader};
use datalove_rtdt as rtdt;
use rtdt::{TyDesc, TyInfo, TyTag};

#[salsa::tracked(returns(copy))]
fn compile<'db>(db: &'db dyn salsa::Database, source: bct::input::Source) -> datalove_datalit::tycheck::TypecheckResult<'db> {
    let parse_result = datalove_datalit::parser::parse(db, source);
    let parsed = parse_result.expr(db);
    let resolved = datalove_datalit::resolve::resolve_names(db, source, parsed.clone());
    datalove_datalit::tycheck::type_check(db, parsed, resolved)
}

/// The descriptor of the type of a datalit value.
fn tydesc_of<'db>(db: &'db Database, table: &mut TyDescTable<'db>, source_text: &str) -> *const TyDesc {
    let source = bct::input::Source::new(db, source_text.to_string());
    let checked = compile(db, source);
    assert!(checked.errors(db).is_empty(), "{source_text} does not typecheck");
    let ty = checked.root_type(db).as_ref().expect("a type for the value");
    table.get_or_create(ty)
}

/// Storage for the templates a test makes.
///
/// Kept as raw pointers, freed on drop: moving a `Box` after lending a pointer
/// into it would invalidate the pointer.
struct Arena {
    frees: Vec<Box<dyn FnOnce()>>,
}

impl Drop for Arena {
    fn drop(&mut self) {
        for free in self.frees.drain(..) {
            free();
        }
    }
}

impl Arena {
    fn new() -> Arena {
        Arena { frees: Vec::new() }
    }

    fn desc(&mut self, desc: TyDesc) -> *const TyDesc {
        let ptr = Box::into_raw(Box::new(desc));
        self.frees.push(Box::new(move || drop(unsafe { Box::from_raw(ptr) })));
        ptr
    }

    fn slice<T: 'static>(&mut self, items: Vec<T>) -> *const T {
        let ptr = Box::into_raw(items.into_boxed_slice());
        self.frees.push(Box::new(move || drop(unsafe { Box::from_raw(ptr) })));
        ptr as *const T
    }

    fn param(&mut self, index: u32) -> *const TyDesc {
        self.desc(TyDesc {
            type_tag: TyTag::Param,
            flags: 0,
            size: 0,
            align: 0,
            type_info: TyInfo { param: rtdt::TyInfoParam { index } },
        })
    }

    /// The template of `td` with `params[i]` replaced by parameter `i`
    /// wherever it appears, or `td` itself if none does.
    ///
    /// The descriptor table shares one descriptor per type, so a part is found
    /// by its address.
    fn templatize(&mut self, td: *const TyDesc, params: &[*const TyDesc]) -> *const TyDesc {
        if td.is_null() {
            return td;
        }
        if let Some(i) = params.iter().position(|p| *p == td) {
            return self.param(i as u32);
        }
        let d = unsafe { &*td };
        // Size and alignment are left as the concrete type's. A template's
        // are meaningless, and instantiation has to work them out rather
        // than copy them, which leaving them right would hide; so they are
        // spoiled where a parameter was put in.
        let spoiled = |type_info| TyDesc { type_tag: d.type_tag, flags: 0, size: 999, align: 64, type_info };
        unsafe {
            match d.type_tag {
                TyTag::List => {
                    let e = self.templatize(d.type_info.list.element_tydesc, params);
                    if e == d.type_info.list.element_tydesc { return td; }
                    self.desc(spoiled(TyInfo { list: rtdt::TyInfoList { element_tydesc: e } }))
                }
                TyTag::Set => {
                    let e = self.templatize(d.type_info.set.element_tydesc, params);
                    if e == d.type_info.set.element_tydesc { return td; }
                    self.desc(spoiled(TyInfo { set: rtdt::TyInfoSet { element_tydesc: e, ..d.type_info.set } }))
                }
                TyTag::Map => {
                    let info = d.type_info.map;
                    let k = self.templatize(info.key_tydesc, params);
                    let v = self.templatize(info.value_tydesc, params);
                    if k == info.key_tydesc && v == info.value_tydesc { return td; }
                    self.desc(spoiled(TyInfo { map: rtdt::TyInfoMap { key_tydesc: k, value_tydesc: v, ..info } }))
                }
                TyTag::Tensor => {
                    let info = d.type_info.tensor;
                    let e = self.templatize(info.element_tydesc, params);
                    if e == info.element_tydesc { return td; }
                    self.desc(spoiled(TyInfo { tensor: rtdt::TyInfoTensor { element_tydesc: e, ..info } }))
                }
                TyTag::Option => {
                    let i = self.templatize(d.type_info.option.inner_tydesc, params);
                    if i == d.type_info.option.inner_tydesc { return td; }
                    self.desc(spoiled(TyInfo { option: rtdt::TyInfoOption { inner_tydesc: i } }))
                }
                TyTag::Result => {
                    let o = self.templatize(d.type_info.result.ok_tydesc, params);
                    if o == d.type_info.result.ok_tydesc { return td; }
                    self.desc(spoiled(TyInfo { result: rtdt::TyInfoResult { ok_tydesc: o } }))
                }
                TyTag::Term => {
                    let info = d.type_info.term;
                    let p = self.templatize(info.payload, params);
                    if p == info.payload { return td; }
                    self.desc(spoiled(TyInfo { term: rtdt::TyInfoTerm { payload: p, ..info } }))
                }
                TyTag::Tuple => {
                    let info = d.type_info.tuple;
                    let fields = std::slice::from_raw_parts(info.fields, info.num_fields as usize);
                    let new: Vec<_> = fields.iter()
                        .map(|f| rtdt::TyInfoTupleField { tydesc: self.templatize(f.tydesc, params), offset: 0 })
                        .collect();
                    if new.iter().zip(fields).all(|(n, f)| n.tydesc == f.tydesc) { return td; }
                    let fields = self.slice(new);
                    self.desc(spoiled(TyInfo { tuple: rtdt::TyInfoTuple { fields, ..info } }))
                }
                TyTag::Struct => {
                    let info = d.type_info.struct_;
                    let fields = std::slice::from_raw_parts(info.fields, info.num_fields as usize);
                    let new: Vec<_> = fields.iter()
                        .map(|f| rtdt::TyInfoStructField { tydesc: self.templatize(f.tydesc, params), offset: 0, ..*f })
                        .collect();
                    if new.iter().zip(fields).all(|(n, f)| n.tydesc == f.tydesc) { return td; }
                    let fields = self.slice(new);
                    self.desc(spoiled(TyInfo { struct_: rtdt::TyInfoStruct { fields, ..info } }))
                }
                TyTag::Enum => {
                    let info = d.type_info.enum_;
                    let variants = std::slice::from_raw_parts(info.variants, info.num_variants as usize);
                    let new: Vec<_> = variants.iter()
                        .map(|v| rtdt::TyInfoEnumVariant { payload: self.templatize(v.payload, params), offset: 0, ..*v })
                        .collect();
                    if new.iter().zip(variants).all(|(n, v)| n.payload == v.payload) { return td; }
                    let variants = self.slice(new);
                    self.desc(spoiled(TyInfo { enum_: rtdt::TyInfoEnum { variants, ..info } }))
                }
                TyTag::Table => {
                    let info = d.type_info.table;
                    let columns = std::slice::from_raw_parts(info.columns, info.num_columns as usize);
                    let new: Vec<_> = columns.iter()
                        .map(|c| rtdt::TyInfoTableColumn { tydesc: self.templatize(c.tydesc, params), ..*c })
                        .collect();
                    if new.iter().zip(columns).all(|(n, c)| n.tydesc == c.tydesc) { return td; }
                    let columns = self.slice(new);
                    self.desc(spoiled(TyInfo { table: rtdt::TyInfoTable { columns, ..info } }))
                }
                _ => td,
            }
        }
    }
}

fn name(ptr: *const u8, len: u32) -> &'static [u8] {
    unsafe { std::slice::from_raw_parts(ptr, len as usize) }
}

/// Assert that two descriptors agree in every field, all the way down.
fn assert_same(a: *const TyDesc, b: *const TyDesc, path: &str) {
    if a.is_null() || b.is_null() {
        assert_eq!(a.is_null(), b.is_null(), "{path}: one part is missing");
        return;
    }
    let (x, y) = unsafe { (&*a, &*b) };
    assert_eq!(x.type_tag, y.type_tag, "{path}: tag");
    assert_eq!(x.size, y.size, "{path}: size of a {:?}", x.type_tag);
    assert_eq!(x.align, y.align, "{path}: align of a {:?}", x.type_tag);
    assert_eq!(x.flags, y.flags, "{path}: flags of a {:?}", x.type_tag);
    unsafe {
        match x.type_tag {
            TyTag::List => assert_same(x.type_info.list.element_tydesc, y.type_info.list.element_tydesc, &format!("{path}[]")),
            TyTag::Set => {
                let (p, q) = (x.type_info.set, y.type_info.set);
                assert_eq!(p.leaf, q.leaf, "{path}: set leaf layout");
                assert_eq!(p.internal, q.internal, "{path}: set internal layout");
                assert_same(p.element_tydesc, q.element_tydesc, &format!("{path}#"));
            }
            TyTag::Map => {
                let (p, q) = (x.type_info.map, y.type_info.map);
                assert_eq!(p.leaf, q.leaf, "{path}: map leaf layout");
                assert_eq!(p.internal, q.internal, "{path}: map internal layout");
                assert_same(p.key_tydesc, q.key_tydesc, &format!("{path}%key"));
                assert_same(p.value_tydesc, q.value_tydesc, &format!("{path}%value"));
            }
            TyTag::Tensor => {
                assert_eq!(x.type_info.tensor.rank, y.type_info.tensor.rank, "{path}: rank");
                assert_same(x.type_info.tensor.element_tydesc, y.type_info.tensor.element_tydesc, &format!("{path}[||]"));
            }
            TyTag::Option => assert_same(x.type_info.option.inner_tydesc, y.type_info.option.inner_tydesc, &format!("{path}?")),
            TyTag::Result => assert_same(x.type_info.result.ok_tydesc, y.type_info.result.ok_tydesc, &format!("{path}!")),
            TyTag::Term => {
                let (p, q) = (x.type_info.term, y.type_info.term);
                assert_eq!(name(p.name, p.name_len), name(q.name, q.name_len), "{path}: term name");
                assert_same(p.payload, q.payload, &format!("{path}.term"));
            }
            TyTag::Atom => {
                let (p, q) = (x.type_info.atom, y.type_info.atom);
                assert_eq!(name(p.name, p.name_len), name(q.name, q.name_len), "{path}: atom name");
            }
            TyTag::Tuple => {
                let (p, q) = (x.type_info.tuple, y.type_info.tuple);
                assert_eq!(p.num_fields, q.num_fields, "{path}: field count");
                for i in 0..p.num_fields as usize {
                    let (f, g) = (&*p.fields.add(i), &*q.fields.add(i));
                    assert_eq!(f.offset, g.offset, "{path}.{i}: offset");
                    assert_same(f.tydesc, g.tydesc, &format!("{path}.{i}"));
                }
            }
            TyTag::Struct => {
                let (p, q) = (x.type_info.struct_, y.type_info.struct_);
                assert_eq!(p.num_fields, q.num_fields, "{path}: field count");
                for i in 0..p.num_fields as usize {
                    let (f, g) = (&*p.fields.add(i), &*q.fields.add(i));
                    assert_eq!(name(f.name, f.name_len), name(g.name, g.name_len), "{path}.{i}: name");
                    assert_eq!(f.offset, g.offset, "{path}.{i}: offset");
                    assert_same(f.tydesc, g.tydesc, &format!("{path}.{i}"));
                }
            }
            TyTag::Enum => {
                let (p, q) = (x.type_info.enum_, y.type_info.enum_);
                assert_eq!(p.num_variants, q.num_variants, "{path}: variant count");
                for i in 0..p.num_variants as usize {
                    let (v, w) = (&*p.variants.add(i), &*q.variants.add(i));
                    assert_eq!(name(v.name, v.name_len), name(w.name, w.name_len), "{path}.{i}: name");
                    if !v.payload.is_null() {
                        assert_eq!(v.offset, w.offset, "{path}.{i}: offset");
                    }
                    assert_same(v.payload, w.payload, &format!("{path}.{i}"));
                }
            }
            TyTag::Table => {
                let (p, q) = (x.type_info.table, y.type_info.table);
                assert_eq!(p.num_columns, q.num_columns, "{path}: column count");
                for i in 0..p.num_columns as usize {
                    let (c, d) = (&*p.columns.add(i), &*q.columns.add(i));
                    assert_eq!(name(c.name, c.name_len), name(d.name, d.name_len), "{path}.{i}: name");
                    assert_same(c.tydesc, d.tydesc, &format!("{path}.{i}"));
                }
            }
            _ => {}
        }
    }
}

fn instantiate(rt: &datalove_rt::rust::Runtime, template: *const TyDesc, params: &[*const TyDesc]) -> *const TyDesc {
    unsafe { c::dtlv_rti_tydesc_instantiate(rt.handle(), &*template, params.as_ptr(), params.len() as u32) }
}

/// Make a template of `concrete` over the types of `params`, instantiate it
/// with them, and check that what comes back is `concrete` again.
fn round_trip(concrete: &str, params: &[&str]) {
    let db = Database::default();
    let mut table = TyDescTable::new(&db);
    let mut arena = Arena::new();
    let rt = datalove_rt::rust::Runtime::new();

    let expected = tydesc_of(&db, &mut table, concrete);
    let params: Vec<_> = params.iter().map(|p| tydesc_of(&db, &mut table, p)).collect();
    let template = arena.templatize(expected, &params);
    assert_ne!(template, expected, "{concrete}: nothing in it was made a parameter");

    let built = instantiate(&rt, template, &params);
    assert_ne!(built, template);
    assert_same(built, expected, concrete);
}

#[test]
fn a_tuple() {
    round_trip(": (u8, u32) / (1, 2)", &[": u8 / 1"]);
    round_trip(": (u32, u8, u64) / (1, 2, 3)", &[": u8 / 1"]);
}

#[test]
fn a_struct_keeps_its_names() {
    round_trip(r#": {a: u8, b: string, c: u16} / {a = 1, b = "x", c = 3}"#, &[": u8 / 1"]);
}

#[test]
fn an_owning_part_clears_the_plain_flag() {
    round_trip(r#": (string, u32) / ("x", 2)"#, &[r#": string / "x""#]);
    round_trip(": (u8, u32) / (1, 2)", &[": (u8, u32) / (1, 2)"]);
}

#[test]
fn an_option_and_a_result() {
    round_trip(": ?u8 / none", &[": u8 / 1"]);
    round_trip(": ?(u8, u64) / none", &[": u8 / 1"]);
    round_trip(": !u8 / ok 1", &[": u8 / 1"]);
    round_trip(": !(u64, u8) / ok (1, 2)", &[": u8 / 1"]);
}

#[test]
fn the_containers() {
    round_trip(": [u8] / []", &[": u8 / 1"]);
    round_trip(": #{u16} / #{}", &[": u16 / 1"]);
    round_trip(": %{u8 = string} / %{}", &[": u8 / 1", r#": string / "x""#]);
    round_trip(": %{u8 = string} / %{}", &[r#": string / "x""#]);
    round_trip(": [|u8, 2|] / [| 1 2, 3 4 |]", &[": u8 / 1"]);
}

#[test]
fn a_table_keeps_its_column_names() {
    round_trip(": {|a: u8, b: string|} / {| a, b; |}", &[": u8 / 1"]);
}

#[test]
fn a_term_and_an_enum() {
    round_trip(": term Wrap u8 / term Wrap 1", &[": u8 / 1"]);
    round_trip(": enum {atom None, term Some u8, term Pair (u64, u8)} / enum {atom None}", &[": u8 / 1"]);
}

#[test]
fn nested_forms() {
    round_trip(r#": [(?u8, %{u8 = [string]})] / []"#, &[": u8 / 1"]);
    round_trip(r#": [(?u8, %{u8 = [string]})] / []"#, &[": u8 / 1", r#": string / "x""#]);
    round_trip(": ?[?(u16, u8)] / none", &[": u8 / 1", ": u16 / 1"]);
}

#[test]
fn a_bare_parameter_is_what_it_is_bound_to() {
    let db = Database::default();
    let mut table = TyDescTable::new(&db);
    let mut arena = Arena::new();
    let rt = datalove_rt::rust::Runtime::new();
    let u8_desc = tydesc_of(&db, &mut table, ": u8 / 1");
    let string_desc = tydesc_of(&db, &mut table, r#": string / "x""#);
    let second = arena.param(1);
    assert_eq!(instantiate(&rt, second, &[u8_desc, string_desc]), string_desc);
}

#[test]
fn a_template_with_no_parameter_is_its_own_instance() {
    let db = Database::default();
    let mut table = TyDescTable::new(&db);
    let rt = datalove_rt::rust::Runtime::new();
    let closed = tydesc_of(&db, &mut table, ": (u8, string) / (1, \"x\")");
    let u32_desc = tydesc_of(&db, &mut table, ": u32 / 1");
    assert_eq!(instantiate(&rt, closed, &[u32_desc]), closed);
}

#[test]
fn asking_again_finds_what_was_built() {
    let db = Database::default();
    let mut table = TyDescTable::new(&db);
    let mut arena = Arena::new();
    let rt = datalove_rt::rust::Runtime::new();
    let u8_desc = tydesc_of(&db, &mut table, ": u8 / 1");
    let u16_desc = tydesc_of(&db, &mut table, ": u16 / 1");
    let pair = tydesc_of(&db, &mut table, ": (u8, u32) / (1, 2)");
    let template = arena.templatize(pair, &[u8_desc]);

    let first = instantiate(&rt, template, &[u8_desc]);
    assert_eq!(instantiate(&rt, template, &[u8_desc]), first, "a second ask builds again");

    // Another binding is another descriptor.
    let other = instantiate(&rt, template, &[u16_desc]);
    assert_ne!(other, first);
    assert_eq!(unsafe { (*other).size }, 8);

    // So is another template with the same parts, since what is around the
    // parts -- names, here -- is the template's.
    let named = tydesc_of(&db, &mut table, ": {a: u8, b: u32} / {a = 1, b = 2}");
    let renamed = tydesc_of(&db, &mut table, ": {x: u8, y: u32} / {x = 1, y = 2}");
    let named = arena.templatize(named, &[u8_desc]);
    let renamed = arena.templatize(renamed, &[u8_desc]);
    let a = instantiate(&rt, named, &[u8_desc]);
    let b = instantiate(&rt, renamed, &[u8_desc]);
    assert_ne!(a, b);
    let field_name = |td: *const TyDesc| unsafe {
        let f = &*(*td).type_info.struct_.fields;
        name(f.name, f.name_len)
    };
    assert_eq!(field_name(a), b"a");
    assert_eq!(field_name(b), b"x");
}

#[test]
fn built_descriptors_survive_a_reset() {
    let db = Database::default();
    let mut table = TyDescTable::new(&db);
    let mut arena = Arena::new();
    let rt = datalove_rt::rust::Runtime::new();
    let u8_desc = tydesc_of(&db, &mut table, ": u8 / 1");
    let list = tydesc_of(&db, &mut table, ": [(u8, string)] / []");
    let template = arena.templatize(list, &[u8_desc]);

    let built = instantiate(&rt, template, &[u8_desc]);
    unsafe { c::dtlv_rti_reset(rt.handle()) };
    assert_eq!(instantiate(&rt, template, &[u8_desc]), built);
    assert_same(built, list, "after a reset");
}

#[test]
fn a_built_descriptor_clones_like_a_static_one() {
    // A static descriptor and a built one for the same type are different
    // pointers to the same type, and the runtime treats them as one.
    let db = Database::default();
    let mut table = TyDescTable::new(&db);
    let mut arena = Arena::new();
    let rt = datalove_rt::rust::Runtime::new();
    let u8_desc = tydesc_of(&db, &mut table, ": u8 / 1");
    let pair = tydesc_of(&db, &mut table, ": (u8, u32) / (1, 2)");
    let built = instantiate(&rt, arena.templatize(pair, &[u8_desc]), &[u8_desc]);

    let value: [u32; 2] = [7, 9];
    let mut out: [u32; 2] = [0, 0];
    let status = unsafe {
        c::dtlv_rti_clone_local(rt.handle(), value.as_ptr() as *const u8, &*pair, out.as_mut_ptr() as *mut u8, &*built)
    };
    assert_eq!(status, c::RtStatus::Ok);
    assert_eq!(out[1], 9);
}

#[test]
fn payload_offsets() {
    let db = Database::default();
    let mut table = TyDescTable::new(&db);
    let offset = |td: *const datalove_rtdt::TyDesc| unsafe { c::dtlv_rti_payload_offset(&*td, 0) };

    let option_u8 = tydesc_of(&db, &mut table, ": ?u8 / none");
    let option_u64 = tydesc_of(&db, &mut table, ": ?u64 / none");
    assert_eq!(offset(option_u8), 1);
    assert_eq!(offset(option_u64), 8);

    // A result's payload is aligned for the error it may hold, whatever its
    // value is.
    let result_u8 = tydesc_of(&db, &mut table, ": !u8 / ok 1");
    assert_eq!(offset(result_u8), 8);

    let term = tydesc_of(&db, &mut table, ": term Wrap u64 / term Wrap 1");
    assert_eq!(offset(term), 0);

    let enum_desc = tydesc_of(&db, &mut table, ": enum {atom A, term B u8, term C u64} / enum {atom A}");
    let variants = unsafe { (*enum_desc).type_info.enum_ };
    for i in 0..variants.num_variants {
        let variant = unsafe { &*variants.variants.add(i as usize) };
        assert_eq!(unsafe { c::dtlv_rti_payload_offset(&*enum_desc, i) }, variant.offset);
        assert_eq!(unsafe { c::dtlv_rti_payload_tydesc(&*enum_desc, i) }, variant.payload);
    }

    assert_eq!(unsafe { c::dtlv_rti_payload_tydesc(&*option_u64, 0) }, unsafe { (*option_u64).type_info.option.inner_tydesc });
    assert_eq!(unsafe { c::dtlv_rti_payload_tydesc(&*result_u8, 0) }, unsafe { (*result_u8).type_info.result.ok_tydesc });
}

// ============================================================================
// The shadow stack
// ============================================================================

fn header(rt: &datalove_rt::rust::Runtime) -> *mut RtHeader {
    rt.handle() as *mut RtHeader
}

fn shadow_alloc(rt: &datalove_rt::rust::Runtime, size: u32, align: u32) -> *mut u8 {
    unsafe { c::dtlv_rti_shadow_alloc(rt.handle(), size, align) }
}

/// Where the shadow stack stands, as compiled code saves it on entry.
fn mark(rt: &datalove_rt::rust::Runtime) -> (usize, usize) {
    unsafe { ((*header(rt)).shadow_top, (*header(rt)).shadow_limit) }
}

fn release(rt: &datalove_rt::rust::Runtime, (top, limit): (usize, usize)) {
    unsafe {
        (*header(rt)).shadow_top = top;
        (*header(rt)).shadow_limit = limit;
    }
}

#[test]
fn shadow_allocations_are_aligned_and_disjoint() {
    let rt = datalove_rt::rust::Runtime::new();
    let a = shadow_alloc(&rt, 1, 1);
    let b = shadow_alloc(&rt, 8, 8);
    let c = shadow_alloc(&rt, 3, 4);
    assert_eq!(b as usize % 8, 0);
    assert_eq!(c as usize % 4, 0);
    assert!(a < b && b as usize + 8 <= c as usize);
    let (top, _) = mark(&rt);
    assert_eq!(top, c as usize + 3);
}

#[test]
fn releasing_gives_the_space_back() {
    let rt = datalove_rt::rust::Runtime::new();
    let entry = mark(&rt);
    let first = shadow_alloc(&rt, 40, 8);
    release(&rt, entry);
    assert_eq!(shadow_alloc(&rt, 40, 8), first);
}

#[test]
fn growing_moves_to_a_new_chunk_and_keeps_the_old() {
    let rt = datalove_rt::rust::Runtime::new();
    let small = shadow_alloc(&rt, 16, 8);
    unsafe { (small as *mut u64).write(0x5eed) };

    // Bigger than a chunk, so it cannot fit in the first.
    let entry = mark(&rt);
    let big = shadow_alloc(&rt, 100_000, 8);
    assert!(!big.is_null());
    unsafe { std::ptr::write_bytes(big, 0xab, 100_000) };
    assert_eq!(unsafe { (small as *const u64).read() }, 0x5eed, "growing moved what was there");
    // The big request filled its chunk, so this one is in yet another, which
    // the allocator may have put anywhere, as long as it is clear of the big.
    let after_big = shadow_alloc(&rt, 8, 8);
    let after_big_end = after_big as usize + 8;
    assert!(after_big_end <= big as usize || after_big as usize >= big as usize + 100_000);

    // Restoring past the chunk boundary goes back to the first chunk, limit
    // and all, so the next small request is beside the first.
    release(&rt, entry);
    assert_eq!(mark(&rt), entry);
    let again = shadow_alloc(&rt, 16, 8);
    assert_eq!(again as usize, small as usize + 16);

    // And growing again reuses the chunk the stack unwound out of.
    let entry = mark(&rt);
    let big_again = shadow_alloc(&rt, 100_000, 8);
    assert_eq!(big_again, big);
    release(&rt, entry);
}

#[test]
fn a_chunk_too_small_for_a_request_is_replaced() {
    let rt = datalove_rt::rust::Runtime::new();
    let entry = mark(&rt);
    let _ = shadow_alloc(&rt, 70_000, 8);
    release(&rt, entry);
    let bigger = shadow_alloc(&rt, 300_000, 8);
    unsafe { std::ptr::write_bytes(bigger, 0, 300_000) };
    release(&rt, entry);
}

#[test]
fn a_reset_empties_the_shadow_stack() {
    let rt = datalove_rt::rust::Runtime::new();
    let start = mark(&rt);
    let _ = shadow_alloc(&rt, 32, 8);
    let _ = shadow_alloc(&rt, 100_000, 8);
    unsafe { c::dtlv_rti_reset(rt.handle()) };
    assert_eq!(mark(&rt), start);
}
