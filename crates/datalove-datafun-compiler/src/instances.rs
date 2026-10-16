//! Generic specialization: instances of generics, and calls pointed at them.
//!
//! A call to a generic whose bindings are concrete can call an instance of the
//! generic at those bindings instead (`ir::instantiate::instantiate`). The
//! generic stays, so this is additive: each call site is pointed at an
//! instance or left alone on its own, and leaving one alone is always right.
//! What decides is the [`GenericsPolicy`].
//!
//! An instance's own calls to generics have concrete bindings, so it is
//! scanned in turn, and `repeated[bool]` calls `push[bool]`. That stops
//! where a generic would call itself at a growing type: `f<T>` calling
//! `f<[T]>` would instantiate forever, so a request for a function already on
//! the chain of instances that led to it, at a larger binding, is left a call
//! to the generic. Caps per function and in total are the backstop.
//!
//! The module graph puts instances in their generic's module, and a script
//! unit puts them in its own `nested_units`, as const specialization does with
//! its copies (`specialize`).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use datalove_datafun_ir::instantiate::instantiate;
use datalove_datafun_ir::{CodeRef, CodeUnitId, Instruction, IrCodeUnit, IrModuleId, IrType};

use crate::specialize::CalleeKey;

/// Most instances one generic may have.
pub const MAX_INSTANCES_PER_FUNCTION: usize = 64;

/// Most instances one pass may make.
pub const MAX_INSTANCES: usize = 4096;

/// Which calls to generics are pointed at instances.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GenericsPolicy {
    /// None: every generic runs on descriptors.
    #[default]
    Off,
    /// Every call whose bindings are concrete.
    All,
    /// A coin per call site, seeded: the same seed makes the same choices.
    ///
    /// For testing. Two calls to one instance in one function can go
    /// different ways, which is where an instance meeting its generic is most
    /// likely to go wrong.
    Chaos { seed: u64 },
}

/// One call site: the function it is in, and where in it.
#[derive(Hash)]
struct Site<'u> {
    function: &'u str,
    block: usize,
    instruction: usize,
}

impl GenericsPolicy {
    /// Whether to point the call at `site` to `callee` at bindings
    /// `bindings` at an instance.
    fn wants(self, site: &Site<'_>, callee: CalleeKey, bindings: &[IrType]) -> bool {
        match self {
            GenericsPolicy::Off => false,
            GenericsPolicy::All => true,
            GenericsPolicy::Chaos { seed } => {
                // `DefaultHasher::new` is keyed the same every time, so the
                // choices are reproducible from the seed.
                let mut hasher = std::hash::DefaultHasher::new();
                (seed, site, callee, bindings).hash(&mut hasher);
                hasher.finish() & 1 == 1
            }
        }
    }
}

/// An instance made by [`Instantiator::rewrite`], and the generic it is of.
pub struct Instance {
    pub generic: CalleeKey,
    pub unit: IrCodeUnit,
}

/// One pass's instances, made as calls ask for them.
pub struct Instantiator<'a> {
    policy: GenericsPolicy,
    /// A generic's body, by where it lives; `None` for anything that is not
    /// a generic function that can be instantiated.
    generic: &'a dyn Fn(CalleeKey) -> Option<Arc<IrCodeUnit>>,
    /// Where a call reaches.
    key_of: &'a dyn Fn(&CodeRef) -> Option<CalleeKey>,
    /// An id for a new instance of a generic, and how a call reaches it.
    place: &'a mut dyn FnMut(CalleeKey) -> (CodeUnitId, CodeRef),
    made: HashMap<(CalleeKey, Vec<IrType>), CodeRef>,
    per_function: HashMap<CalleeKey, usize>,
    instances: Vec<Instance>,
}

impl<'a> Instantiator<'a> {
    pub fn new(
        policy: GenericsPolicy,
        generic: &'a dyn Fn(CalleeKey) -> Option<Arc<IrCodeUnit>>,
        key_of: &'a dyn Fn(&CodeRef) -> Option<CalleeKey>,
        place: &'a mut dyn FnMut(CalleeKey) -> (CodeUnitId, CodeRef),
    ) -> Self {
        Instantiator {
            policy, generic, key_of, place,
            made: HashMap::new(),
            per_function: HashMap::new(),
            instances: Vec::new(),
        }
    }

    /// Point `unit`'s calls at instances where the policy wants them, making
    /// the instances, and theirs in turn.
    pub fn rewrite(&mut self, unit: &mut IrCodeUnit) {
        let mut pending = Vec::new();
        self.rewrite_calls(unit, &[], &mut pending);
        while let Some((mut instance, chain)) = pending.pop() {
            self.rewrite_calls(&mut instance.unit, &chain, &mut pending);
            self.instances.push(instance);
        }
    }

    /// The instances made, in the order they were finished.
    pub fn finish(self) -> Vec<Instance> {
        self.instances
    }

    fn rewrite_calls(
        &mut self,
        unit: &mut IrCodeUnit,
        chain: &[(CalleeKey, Vec<IrType>)],
        pending: &mut Vec<(Instance, Vec<(CalleeKey, Vec<IrType>)>)>,
    ) {
        for (b, block) in unit.blocks.iter_mut().enumerate() {
            for (i, instr) in block.instructions.iter_mut().enumerate() {
                let Instruction::Call { func, bindings, .. } = instr else {
                    continue;
                };
                if bindings.is_empty() || bindings.iter().any(IrType::is_dynamic) {
                    continue;
                }
                let Some(callee) = (self.key_of)(func) else {
                    continue;
                };
                let site = Site { function: &unit.name, block: b, instruction: i };
                if !self.policy.wants(&site, callee, bindings) {
                    continue;
                }
                if let Some(instance) = self.instance_for(callee, bindings, chain, pending) {
                    *func = instance;
                    bindings.clear();
                }
            }
        }
    }

    /// How to reach the instance of `callee` at `bindings`, made now if it
    /// has to be; `None` to leave the call to the generic.
    ///
    /// The policy has been asked already, per call site, so that a site can
    /// call the generic though another has made the instance.
    fn instance_for(
        &mut self,
        callee: CalleeKey,
        bindings: &[IrType],
        chain: &[(CalleeKey, Vec<IrType>)],
        pending: &mut Vec<(Instance, Vec<(CalleeKey, Vec<IrType>)>)>,
    ) -> Option<CodeRef> {
        let key = (callee, bindings.to_vec());
        if let Some(made) = self.made.get(&key) {
            return Some(made.clone());
        }
        if grows(chain, callee, bindings) {
            return None;
        }
        let count = self.per_function.entry(callee).or_default();
        if *count >= MAX_INSTANCES_PER_FUNCTION || self.made.len() >= MAX_INSTANCES {
            return None;
        }
        let generic = (self.generic)(callee)?;
        *count += 1;

        let (id, code_ref) = (self.place)(callee);
        let unit = instantiate(&generic, bindings, id, format!("{}__g{}", generic.name, id.0));
        self.made.insert(key.clone(), code_ref.clone());
        let mut chain = chain.to_vec();
        chain.push(key);
        pending.push((Instance { generic: callee, unit }, chain));
        Some(code_ref)
    }
}

/// Whether instantiating `callee` at `bindings` from the end of `chain`
/// would be a step of a recursion at a growing type.
fn grows(chain: &[(CalleeKey, Vec<IrType>)], callee: CalleeKey, bindings: &[IrType]) -> bool {
    let size = bindings_size(bindings);
    chain.iter().any(|(earlier, earlier_bindings)| *earlier == callee && bindings_size(earlier_bindings) < size)
}

fn bindings_size(bindings: &[IrType]) -> usize {
    bindings.iter().map(type_size).sum()
}

/// How many type constructors and leaves `ty` is made of.
fn type_size(ty: &IrType) -> usize {
    1 + match ty {
        IrType::Tuple(fields) => fields.iter().map(type_size).sum(),
        IrType::Struct(fields) => fields.iter().map(|(_, t)| type_size(t)).sum(),
        IrType::Enum(variants) => variants.iter().filter_map(|(_, p)| p.as_ref()).map(type_size).sum(),
        IrType::Table(columns) => columns.iter().map(|(_, t)| type_size(t)).sum(),
        IrType::Term(_, inner) | IrType::List(inner) | IrType::Set(inner) | IrType::Option(inner)
        | IrType::Result(inner) | IrType::Tensor(inner, _) | IrType::Ref(inner) => type_size(inner),
        IrType::Map(k, v) => type_size(k) + type_size(v),
        _ => 0,
    }
}

/// Whether `unit` can be instantiated: a generic function that is called.
pub fn is_instantiable(unit: &IrCodeUnit) -> bool {
    unit.function_context().is_some_and(|ctx| ctx.type_param_count > 0 && !ctx.inline)
}

/// Whether `unit` makes a call that an instance could take.
pub fn has_concrete_generic_call(unit: &IrCodeUnit) -> bool {
    unit.blocks.iter().flat_map(|b| &b.instructions).any(|instr| matches!(instr,
        Instruction::Call { bindings, .. } if !bindings.is_empty() && !bindings.iter().any(IrType::is_dynamic)))
}

/// Specialize the module graph: `modules` is every module's functions, by id.
///
/// Each instance goes in its generic's module, numbered after the module's
/// other functions, in order of `modules` and of the functions in each.
pub fn instantiate_module_graph(
    modules: &mut [(IrModuleId, &mut Vec<Arc<IrCodeUnit>>)],
    policy: GenericsPolicy,
) {
    let generics: HashMap<CalleeKey, Arc<IrCodeUnit>> = modules.iter()
        .flat_map(|(module, functions)| functions.iter()
            .filter(|f| is_instantiable(f))
            .map(|f| (CalleeKey::Module(*module, f.id), Arc::clone(f))))
        .collect();
    let mut next_id: HashMap<IrModuleId, u32> = modules.iter()
        .map(|(module, functions)| (*module, functions.iter().map(|f| f.id.0 + 1).max().unwrap_or(0)))
        .collect();

    let generic = |key: CalleeKey| generics.get(&key).cloned();
    let key_of = crate::specialize::module_callee_key;
    let mut place = |key: CalleeKey| {
        let CalleeKey::Module(module, _) = key else {
            unreachable!("a module function calls module functions");
        };
        let next = next_id.get_mut(&module).expect("every module is numbered");
        let id = CodeUnitId(*next);
        *next += 1;
        (id, CodeRef::Module { module, id })
    };
    let mut instantiator = Instantiator::new(policy, &generic, &key_of, &mut place);
    for (_, functions) in modules.iter_mut() {
        for function in functions.iter_mut() {
            if has_concrete_generic_call(function) {
                instantiator.rewrite(Arc::make_mut(function));
            }
        }
    }
    let instances = instantiator.finish();

    for instance in instances {
        let CalleeKey::Module(module, _) = instance.generic else {
            unreachable!("a module function's instance is of a module function");
        };
        let (_, functions) = modules.iter_mut()
            .find(|(m, _)| *m == module)
            .expect("an instance's module is in the graph");
        functions.push(Arc::new(instance.unit));
    }
    for (_, functions) in modules.iter_mut() {
        functions.sort_by_key(|f| f.id.0);
    }
}

/// Specialize a script unit: its calls and its functions' calls.
///
/// Instances go in the unit's `nested_units`, reached by `CodeRef::Local`,
/// so that nothing in the module graph changes. `module_unit` reads a module
/// function out of the registry.
pub fn instantiate_script_unit(
    unit: &mut IrCodeUnit,
    module_unit: &dyn Fn(IrModuleId, CodeUnitId) -> Option<IrCodeUnit>,
    policy: GenericsPolicy,
) {
    let local: HashMap<CodeUnitId, Arc<IrCodeUnit>> = unit.nested_units.iter()
        .filter(|f| is_instantiable(f))
        .map(|f| (f.id, Arc::new(f.clone())))
        .collect();
    let generic = |key: CalleeKey| match key {
        CalleeKey::Local(id) => local.get(&id).cloned(),
        CalleeKey::Module(module, id) => module_unit(module, id)
            .filter(is_instantiable)
            .map(Arc::new),
    };
    let key_of = crate::specialize::script_callee_key;
    let mut next = unit.nested_units.iter().map(|f| f.id.0 + 1).max().unwrap_or(0);
    let mut place = |_| {
        let id = CodeUnitId(next);
        next += 1;
        (id, CodeRef::Local(id))
    };
    let mut instantiator = Instantiator::new(policy, &generic, &key_of, &mut place);
    if has_concrete_generic_call(unit) {
        instantiator.rewrite(unit);
    }
    for nested in &mut unit.nested_units {
        if has_concrete_generic_call(nested) {
            instantiator.rewrite(nested);
        }
    }
    let instances = instantiator.finish();
    unit.nested_units.extend(instances.into_iter().map(|instance| instance.unit));
    unit.nested_units.sort_by_key(|f| f.id.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use datalove_datafun_ir::{
        BlockId, CodeUnitContext, FunctionContext, IrBlock, Operand, ParamId, ParamMode, SymbolTable,
        Terminator, ValueId,
    };

    const M: IrModuleId = IrModuleId(0);

    fn list(t: IrType) -> IrType {
        IrType::List(Box::new(t))
    }

    /// A function of one parameter that calls each of `calls`, a callee and
    /// what it binds, with that parameter.
    fn function(id: u32, type_params: u32, param: IrType, calls: Vec<(u32, Vec<IrType>)>) -> IrCodeUnit {
        let instructions = calls.into_iter().enumerate()
            .map(|(i, (callee, bindings))| Instruction::Call {
                dest: ValueId(i as u32),
                func: CodeRef::Module { module: M, id: CodeUnitId(callee) },
                args: vec![Operand::Param(ParamId(0))],
                bindings,
            })
            .collect::<Vec<_>>();
        let values = instructions.len();
        IrCodeUnit {
            id: CodeUnitId(id),
            name: format!("f{id}"),
            blocks: vec![IrBlock {
                id: BlockId(0),
                params: Vec::new(),
                instructions,
                terminator: Terminator::Return { value: None },
            }],
            value_count: values as u32,
            slot_count: 0,
            value_types: vec![IrType::Unit; values],
            slot_types: Vec::new(),
            tracked_slots: Vec::new(),
            const_values: Vec::new(),
            symbols: SymbolTable::new(),
            context: CodeUnitContext::Function(FunctionContext {
                params: vec![ParamId(0)],
                param_modes: vec![ParamMode::Ref],
                param_types: vec![param],
                return_type: IrType::Unit,
                tracked_params: Vec::new(),
                type_param_count: type_params,
                descriptor_table: Vec::new(),
                inline: false,
            }),
            nested_units: Vec::new(),
        }
    }

    fn calls(unit: &IrCodeUnit) -> Vec<(CodeRef, Vec<IrType>)> {
        unit.blocks.iter().flat_map(|b| &b.instructions)
            .filter_map(|instr| match instr {
                Instruction::Call { func, bindings, .. } => Some((func.clone(), bindings.clone())),
                _ => None,
            })
            .collect()
    }

    fn module(id: u32) -> CodeRef {
        CodeRef::Module { module: M, id: CodeUnitId(id) }
    }

    fn run(functions: Vec<IrCodeUnit>) -> Vec<Arc<IrCodeUnit>> {
        let mut functions: Vec<Arc<IrCodeUnit>> = functions.into_iter().map(Arc::new).collect();
        instantiate_module_graph(&mut [(M, &mut functions)], GenericsPolicy::All);
        functions
    }

    #[test]
    fn an_instance_calls_an_instance() {
        // `main` calls `repeated<bool>`, which calls `push<T>`.
        let functions = run(vec![
            function(0, 0, IrType::Bool, vec![(1, vec![IrType::Bool])]),
            function(1, 1, IrType::Param(0), vec![(2, vec![IrType::Param(0)])]),
            function(2, 1, IrType::Param(0), vec![]),
        ]);
        let names: Vec<&str> = functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["f0", "f1", "f2", "f1__g3", "f2__g4"]);
        assert_eq!(calls(&functions[0]), [(module(3), vec![])]);
        assert_eq!(calls(&functions[3]), [(module(4), vec![])]);
        // The generics are as they were.
        assert_eq!(calls(&functions[1]), [(module(2), vec![IrType::Param(0)])]);
    }

    #[test]
    fn one_binding_is_one_instance() {
        // `main` calls `f<u32>` twice, and `f` calls itself at its own binding.
        let functions = run(vec![
            function(0, 0, IrType::U32, vec![(1, vec![IrType::U32]), (1, vec![IrType::U32])]),
            function(1, 1, IrType::Param(0), vec![(1, vec![IrType::Param(0)])]),
        ]);
        assert_eq!(functions.len(), 3);
        assert_eq!(calls(&functions[0]), [(module(2), vec![]), (module(2), vec![])]);
        assert_eq!(calls(&functions[2]), [(module(2), vec![])]);
    }

    #[test]
    fn a_growing_type_stops_at_the_generic() {
        // `f<T>` calls `f<[T]>`: one instance, whose call is left generic.
        let functions = run(vec![
            function(0, 0, IrType::U32, vec![(1, vec![IrType::U32])]),
            function(1, 1, IrType::Param(0), vec![(1, vec![list(IrType::Param(0))])]),
        ]);
        assert_eq!(functions.len(), 3);
        assert_eq!(calls(&functions[0]), [(module(2), vec![])]);
        assert_eq!(calls(&functions[2]), [(module(1), vec![list(IrType::U32)])]);
    }

    fn run_under(policy: GenericsPolicy, functions: Vec<IrCodeUnit>) -> Vec<Arc<IrCodeUnit>> {
        let mut functions: Vec<Arc<IrCodeUnit>> = functions.into_iter().map(Arc::new).collect();
        instantiate_module_graph(&mut [(M, &mut functions)], policy);
        functions
    }

    #[test]
    fn chaos_splits_sites_and_repeats_itself() {
        // Sixteen calls to `f<u32>` in one function.
        let program = || vec![
            function(0, 0, IrType::U32, vec![(1, vec![IrType::U32]); 16]),
            function(1, 1, IrType::Param(0), vec![]),
        ];
        let instanced = |functions: &[Arc<IrCodeUnit>]| calls(&functions[0]).iter()
            .map(|(func, _)| *func != module(1))
            .collect::<Vec<bool>>();
        let mixed = (0..8).any(|seed| {
            let sites = instanced(&run_under(GenericsPolicy::Chaos { seed }, program()));
            sites.contains(&true) && sites.contains(&false)
        });
        assert!(mixed, "some seed sends one site to the instance and another to the generic");
        for seed in 0..8 {
            let once = instanced(&run_under(GenericsPolicy::Chaos { seed }, program()));
            let again = instanced(&run_under(GenericsPolicy::Chaos { seed }, program()));
            assert_eq!(once, again);
        }
    }

    #[test]
    fn off_makes_nothing() {
        let mut functions: Vec<Arc<IrCodeUnit>> = vec![
            function(0, 0, IrType::U32, vec![(1, vec![IrType::U32])]),
            function(1, 1, IrType::Param(0), vec![]),
        ].into_iter().map(Arc::new).collect();
        instantiate_module_graph(&mut [(M, &mut functions)], GenericsPolicy::Off);
        assert_eq!(functions.len(), 2);
        assert_eq!(calls(&functions[0]), [(module(1), vec![IrType::U32])]);
    }
}
