//! Instances: a generic function with its type parameters bound.
//!
//! A generic's IR is the IR it would have over concrete types, with
//! `IrType::Param` where those types would be. No instruction carries an
//! `IrType` -- one that names a type carries a `TypeRef`, which is a shape --
//! so the parameters appear only in the unit's value, slot, parameter and
//! return types and in the bindings of the calls it makes. Substituting there
//! gives a concrete function, which every backend already compiles.
//!
//! What lowering chose because a `T` is not a copy type -- moves, drops,
//! tracked slots -- stays correct when the binding is one: a move of a copy
//! type copies, and a drop of one does nothing.
//!
//! An instance is additive. The generic stays, and a call to it at a binding
//! means what a call to its instance at that binding does, so any call site
//! may be pointed at either.

use crate::*;

/// `generic` with each type parameter `i` bound to `bindings[i]`: a concrete
/// function, named `name` and numbered `id`.
///
/// Every binding has to be concrete, and so has everything the substitution
/// leaves; a type parameter surviving it is a panic.
pub fn instantiate(generic: &IrCodeUnit, bindings: &[IrType], id: CodeUnitId, name: String) -> IrCodeUnit {
    let ctx = generic.function_context().expect("only a function is generic");
    assert_eq!(
        ctx.type_param_count as usize, bindings.len(),
        "{} has {} type parameters and was bound to {} types", generic.name, ctx.type_param_count, bindings.len(),
    );
    assert!(!ctx.inline, "{} is inline, so it is never called and never instantiated", generic.name);
    assert!(
        bindings.iter().all(|b| !b.is_dynamic()),
        "{} instantiated at a binding that is not concrete: {bindings:?}", generic.name,
    );
    let subst = |ty: &IrType| ty.substitute(bindings);

    let mut unit = generic.clone();
    unit.id = id;
    unit.name = name;
    unit.value_types = generic.value_types.iter().map(subst).collect();
    unit.slot_types = generic.slot_types.iter().map(subst).collect();
    for block in &mut unit.blocks {
        for instr in &mut block.instructions {
            if let Instruction::Call { bindings: inner, .. } | Instruction::ComptimeCall { bindings: inner, .. } = instr {
                *inner = inner.iter().map(subst).collect();
            }
        }
    }
    let CodeUnitContext::Function(ctx) = &mut unit.context else {
        unreachable!("checked above");
    };
    ctx.param_types = ctx.param_types.iter().map(subst).collect();
    ctx.return_type = subst(&ctx.return_type);
    ctx.type_param_count = 0;
    ctx.descriptor_table = Vec::new();

    if let Some(ty) = dynamic_type_in(&unit) {
        panic!("instance {} of {} still mentions a type parameter: {ty}", unit.name, generic.name);
    }
    unit
}

/// The first type in `unit` that mentions a type parameter, if any.
///
/// Everywhere a type parameter can be: the signature, the values and slots,
/// and the bindings of calls.
pub fn dynamic_type_in(unit: &IrCodeUnit) -> Option<&IrType> {
    let signature = unit.function_context().into_iter()
        .flat_map(|ctx| ctx.param_types.iter().chain(std::iter::once(&ctx.return_type)));
    let bindings = unit.blocks.iter()
        .flat_map(|b| &b.instructions)
        .flat_map(|instr| match instr {
            Instruction::Call { bindings, .. } | Instruction::ComptimeCall { bindings, .. } => bindings.as_slice(),
            _ => &[],
        });
    signature
        .chain(&unit.value_types)
        .chain(&unit.slot_types)
        .chain(bindings)
        .find(|ty| ty.is_dynamic())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(i: u32) -> IrType {
        IrType::Param(i)
    }

    fn list(t: IrType) -> IrType {
        IrType::List(Box::new(t))
    }

    /// `fun f<T, U>(x: T, ys: [U]): (T, [U])`, which calls `g<[T]>` and
    /// keeps a `var` of `?U`.
    fn generic() -> IrCodeUnit {
        let pair = IrType::Tuple(vec![t(0), list(t(1))]);
        let value_types = vec![pair.clone(), t(0), list(t(1))];
        let call = Instruction::Call {
            dest: ValueId(1),
            func: CodeRef::Module { module: IrModuleId(0), id: CodeUnitId(1) },
            args: vec![Operand::Param(ParamId(0))],
            bindings: vec![list(t(0))],
        };
        let mut unit = IrCodeUnit {
            id: CodeUnitId(0),
            name: "f".to_string(),
            blocks: vec![IrBlock {
                id: BlockId(0),
                params: Vec::new(),
                instructions: vec![call],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(0))) },
            }],
            value_count: value_types.len() as u32,
            slot_count: 1,
            value_types,
            slot_types: vec![IrType::Option(Box::new(t(1)))],
            tracked_slots: Vec::new(),
            const_values: Vec::new(),
            symbols: SymbolTable::new(),
            context: CodeUnitContext::Function(FunctionContext {
                params: vec![ParamId(0), ParamId(1)],
                param_modes: vec![ParamMode::In, ParamMode::Ref],
                param_types: vec![t(0), list(t(1))],
                return_type: pair,
                tracked_params: Vec::new(),
                type_param_count: 2,
                descriptor_table: Vec::new(),
                inline: false,
            }),
            nested_units: Vec::new(),
        };
        let table = descriptor_table(&unit);
        let CodeUnitContext::Function(ctx) = &mut unit.context else { unreachable!() };
        ctx.descriptor_table = table;
        unit
    }

    #[test]
    fn an_instance_mentions_no_type_parameter() {
        let generic = generic();
        assert!(dynamic_type_in(&generic).is_some());
        let instance = instantiate(&generic, &[IrType::U32, IrType::String], CodeUnitId(7), "f__g7".into());
        assert_eq!(dynamic_type_in(&instance), None);
        assert_eq!(instance.id, CodeUnitId(7));

        let ctx = instance.function_context().unwrap();
        assert_eq!(ctx.type_param_count, 0);
        assert!(ctx.descriptor_table.is_empty());
        assert_eq!(ctx.param_types, vec![IrType::U32, list(IrType::String)]);
        assert_eq!(ctx.return_type, IrType::Tuple(vec![IrType::U32, list(IrType::String)]));
        assert_eq!(instance.slot_types, vec![IrType::Option(Box::new(IrType::String))]);
        assert_eq!(descriptor_table(&instance), Vec::<IrType>::new());
    }

    #[test]
    fn a_call_in_an_instance_is_bound_over_the_instance() {
        let instance = instantiate(&generic(), &[IrType::Bool, IrType::U8], CodeUnitId(7), "f__g7".into());
        let Instruction::Call { bindings, .. } = &instance.blocks[0].instructions[0] else {
            panic!("the call is where it was");
        };
        assert_eq!(bindings, &vec![list(IrType::Bool)]);
    }

    #[test]
    #[should_panic(expected = "not concrete")]
    fn a_binding_has_to_be_concrete() {
        instantiate(&generic(), &[t(0), IrType::U8], CodeUnitId(7), "f__g7".into());
    }
}
