//! Cross-checks the two layout authorities against each other.
//!
//! `datalove_datafun_ir::layout` lays out an `IrType` and is what the AOT
//! backends compile against. `datalove_rtdt::layout` lays out a `TyDesc` and
//! is what the runtime reads values back through. Generated code writes at
//! offsets from the first and the runtime reads at offsets from the second,
//! so a disagreement corrupts values rather than failing a build. That is how
//! the C AOT backend once emitted an enum payload at offset 4 while the
//! runtime looked for it at offset 8.
//!
//! Every type below is checked in both directions: overall size and
//! alignment, and the offset of each field, variant payload and tag payload.

use datalove_datafun_interp::IrTyDescTable;
use datalove_datafun_ir::IrType;
use datalove_datafun_ir::layout as ir_layout;
use datalove_rtdt as rtdt;
use rtdt::TyDescRef;

/// Types spanning every shape the layout code distinguishes.
///
/// Mixed alignments are deliberate: the divergences worth catching are the
/// ones where a small payload sits beside a large one.
fn corpus() -> Vec<IrType> {
    let scalars = vec![
        IrType::Bool, IrType::U8, IrType::I8, IrType::U16, IrType::I16,
        IrType::U32, IrType::I32, IrType::U64, IrType::I64,
        IrType::Index, IrType::Offset, IrType::F32, IrType::F64,
        IrType::Int, IrType::String, IrType::Data, IrType::Error,
        IrType::Unit,
    ];

    let mut types = scalars.clone();

    // Collections, whose own layout is fixed but whose element type varies.
    for elem in [IrType::U8, IrType::U64, IrType::String] {
        types.push(IrType::List(Box::new(elem.clone())));
        types.push(IrType::Set(Box::new(elem.clone())));
        types.push(IrType::Map(Box::new(IrType::U32), Box::new(elem.clone())));
        types.push(IrType::Tensor(Box::new(elem.clone()), 2));
    }

    // Tuples and structs, including alignment-mixing orders both ways.
    let field_sets: Vec<Vec<IrType>> = vec![
        vec![],
        vec![IrType::U8],
        vec![IrType::U8, IrType::U64],
        vec![IrType::U64, IrType::U8],
        vec![IrType::U8, IrType::U16, IrType::U8, IrType::U64],
        vec![IrType::Bool, IrType::String, IrType::U8],
        vec![IrType::Unit, IrType::U32],
    ];
    for fields in &field_sets {
        types.push(IrType::Tuple(fields.clone()));
        let named: Vec<(String, IrType)> = fields.iter().enumerate()
            .map(|(i, ty)| (format!("f{}", i), ty.clone()))
            .collect();
        types.push(IrType::Struct(named));
    }

    // Nested aggregates.
    types.push(IrType::Tuple(vec![
        IrType::Tuple(vec![IrType::U8, IrType::U32]),
        IrType::U64,
    ]));

    // Enums, including the mixed-alignment payloads that once diverged.
    let variant_sets: Vec<Vec<(String, Option<IrType>)>> = vec![
        vec![("A".into(), None)],
        vec![("A".into(), None), ("B".into(), None)],
        vec![("Small".into(), Some(IrType::U8)), ("Big".into(), Some(IrType::U64))],
        vec![("Big".into(), Some(IrType::U64)), ("Small".into(), Some(IrType::U8))],
        vec![
            ("None".into(), None),
            ("Byte".into(), Some(IrType::U8)),
            ("Text".into(), Some(IrType::String)),
        ],
        vec![("Nested".into(), Some(IrType::Tuple(vec![IrType::U8, IrType::U64])))],
    ];
    for variants in &variant_sets {
        types.push(IrType::Enum(variants.clone()));
    }

    // Atoms and terms.
    types.push(IrType::Atom("Red".into()));
    for payload in [IrType::U8, IrType::U64, IrType::String] {
        types.push(IrType::Term("Wrap".into(), Box::new(payload)));
    }

    // Options and results over every scalar plus a few aggregates.
    let inners: Vec<IrType> = scalars.iter().cloned()
        .chain([
            IrType::Tuple(vec![IrType::U8, IrType::U64]),
            IrType::List(Box::new(IrType::U32)),
        ])
        .collect();
    for inner in &inners {
        types.push(IrType::Option(Box::new(inner.clone())));
        types.push(IrType::Result(Box::new(inner.clone())));
    }

    // Options of options, and results of options.
    types.push(IrType::Option(Box::new(IrType::Option(Box::new(IrType::U8)))));
    types.push(IrType::Result(Box::new(IrType::Option(Box::new(IrType::U64)))));

    types
}

/// Size and alignment must agree for every type.
#[test]
fn size_and_align_agree() {
    let mut table = IrTyDescTable::new();
    for ty in corpus() {
        let ir = ir_layout::layout_of(&ty);
        let tydesc = table.get_or_create(&ty);
        let (rt_size, rt_align) = unsafe { ((*tydesc).size, (*tydesc).align) };

        assert_eq!(
            ir.size, rt_size,
            "size disagrees for {:?}: ir {} vs runtime {}", ty, ir.size, rt_size
        );
        assert_eq!(
            ir.align, rt_align,
            "align disagrees for {:?}: ir {} vs runtime {}", ty, ir.align, rt_align
        );
    }
}

/// Field offsets must agree for tuples and structs.
#[test]
fn aggregate_field_offsets_agree() {
    let mut table = IrTyDescTable::new();
    for ty in corpus() {
        let fields: Vec<IrType> = match &ty {
            IrType::Tuple(fields) => fields.clone(),
            IrType::Struct(fields) => fields.iter().map(|(_, t)| t.clone()).collect(),
            _ => continue,
        };
        if fields.is_empty() {
            continue;
        }

        let ir_offsets = ir_layout::aggregate_field_offsets(&fields);
        let tydesc = table.get_or_create(&ty);
        let rt_offsets: Vec<u32> = unsafe {
            let td = TyDescRef::from_ptr(tydesc);
            match &ty {
                IrType::Tuple(_) => td.iter_tuple_fields().map(|f| f.offset()).collect(),
                _ => td.iter_struct_fields().map(|f| f.offset()).collect(),
            }
        };

        assert_eq!(
            ir_offsets, rt_offsets,
            "field offsets disagree for {:?}", ty
        );
    }
}

/// Variant payload offsets must agree for enums.
#[test]
fn enum_variant_offsets_agree() {
    let mut table = IrTyDescTable::new();
    for ty in corpus() {
        let IrType::Enum(variants) = &ty else { continue };

        let ir_offsets = ir_layout::enum_variant_offsets(variants);
        let tydesc = table.get_or_create(&ty);
        let rt_offsets: Vec<u32> = unsafe {
            TyDescRef::from_ptr(tydesc).iter_enum_variants().map(|v| v.offset()).collect()
        };

        assert_eq!(
            ir_offsets, rt_offsets,
            "variant offsets disagree for {:?}", ty
        );
    }
}

/// Payload offsets must agree for options and results.
#[test]
fn option_and_result_payload_offsets_agree() {
    let mut table = IrTyDescTable::new();
    for ty in corpus() {
        let tydesc = table.get_or_create(&ty);
        unsafe {
            let td = TyDescRef::from_ptr(tydesc);
            match &ty {
                IrType::Option(inner) => {
                    let ir = ir_layout::option_payload_offset(inner);
                    let rt = rtdt::layout::compute_option_layout(td).payload_offset;
                    assert_eq!(ir, rt, "option payload offset disagrees for {:?}", ty);
                }
                IrType::Result(ok) => {
                    let ir = ir_layout::result_payload_offset(ok);
                    let rt = rtdt::layout::compute_result_layout(td).payload_offset;
                    assert_eq!(ir, rt, "result payload offset disagrees for {:?}", ty);
                }
                _ => {}
            }
        }
    }
}

/// Node layouts must agree for maps and sets.
///
/// A map's or set's descriptor carries its node layouts. The interpreter's
/// table works them out from the element descriptors, and the AOT backends
/// from the `IrType`'s size and alignment.
#[test]
fn map_and_set_node_layouts_agree() {
    let mut table = IrTyDescTable::new();
    for ty in corpus() {
        let ir = ir_layout::layout_of(&ty);

        let set = IrType::Set(Box::new(ty.clone()));
        let map = IrType::Map(Box::new(ty.clone()), Box::new(IrType::U8));
        unsafe {
            let set_td = TyDescRef::from_ptr(table.get_or_create(&set));
            assert_eq!(*set_td.set_leaf_layout(), rtdt::layout::set_leaf_node_layout(ir.size, ir.align),
                "set leaf layout disagrees for {:?}", ty);
            assert_eq!(*set_td.set_internal_layout(), rtdt::layout::set_internal_node_layout(ir.size, ir.align),
                "set internal layout disagrees for {:?}", ty);

            let map_td = TyDescRef::from_ptr(table.get_or_create(&map));
            assert_eq!(*map_td.map_leaf_layout(), rtdt::layout::map_leaf_node_layout(ir.size, ir.align, 1, 1),
                "map leaf layout disagrees for {:?}", ty);
            assert_eq!(*map_td.map_internal_layout(), rtdt::layout::map_internal_node_layout(ir.size, ir.align),
                "map internal layout disagrees for {:?}", ty);
        }
    }
}
