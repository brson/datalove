//! Instances of generics made at run time, by the binding a call was made at.
//!
//! A call to a generic arrives with a descriptor per type parameter. When the
//! jit is specializing, it reads the binding back out of those descriptors,
//! makes the generic's instance at it (`ir::instantiate`), and counts,
//! compiles and enters the instance in place of the generic. The instance
//! takes the generic's arguments, a pointer each, and no descriptors.
//!
//! Until the instance is compiled, and if it never can be, the interpreter
//! runs the generic with the descriptors, which means the same thing. So the
//! interpreter never runs an instance, and needs nothing from here.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use datalove_datafun_interp::{FuncIdentity, IrInterpreter, ir_type_of_tydesc};
use datalove_datafun_ir::instantiate::{clean_copy_types, instantiate};
use datalove_datafun_ir::{IrCodeUnit, IrType};
use datalove_rtdt::TyDesc;

/// Most run-time instances one generic may have; a binding past it runs the
/// generic.
pub const MAX_INSTANCES_PER_FUNCTION: usize = 64;

/// One instance: its generic, the binding it is at, and its body.
pub(crate) struct Instance {
    pub base: FuncIdentity,
    pub bindings: Box<[IrType]>,
    pub unit: Arc<IrCodeUnit>,
}

/// The instances made so far, numbered in the order they were made.
#[derive(Default)]
pub(crate) struct Instances {
    /// Each generic's bindings as the descriptors calls have arrived with,
    /// and the instance each is, or `None` for one that has none.
    ///
    /// Searched rather than hashed, so that a call that finds its binding
    /// allocates nothing. A generic has few.
    by_descriptors: FxHashMap<FuncIdentity, Vec<(Box<[*const TyDesc]>, Option<u32>)>>,
    /// Each generic's bindings as types: two descriptions of one type are one
    /// instance.
    by_types: FxHashMap<(FuncIdentity, Box<[IrType]>), u32>,
    /// Each instance's generic, binding and body.
    units: FxHashMap<u32, Instance>,
    next: u32,
}

impl Instances {
    /// The instance of `generic`, which is `base`, at the binding
    /// `type_params` describe, made now if it has to be.
    ///
    /// `None` if the binding cannot be read back exactly -- a type the
    /// descriptors describe that `IrType` does not say the same way -- or the
    /// generic has as many instances as it may.
    pub fn get(
        &mut self,
        base: FuncIdentity,
        generic: &IrCodeUnit,
        type_params: &[*const TyDesc],
        interp: &mut IrInterpreter,
    ) -> Option<(FuncIdentity, Arc<IrCodeUnit>)> {
        let seen = self.by_descriptors.entry(base).or_default();
        let id = match seen.iter().find(|(descriptors, _)| **descriptors == *type_params) {
            Some((_, id)) => *id,
            None => {
                let id = Self::read_back(type_params, interp)
                    .and_then(|bindings| self.by_types(base, generic, bindings));
                self.by_descriptors.get_mut(&base).expect("entered above").push((type_params.into(), id));
                id
            }
        }?;
        Some((FuncIdentity::Instance { id }, Arc::clone(&self.units[&id].unit)))
    }

    /// The instance of `base` at the binding `type_params` describe, if a
    /// call has been made at it with those descriptors and it has one.
    ///
    /// For a caller with no interpreter to read a binding back with; nothing
    /// is made.
    pub fn find(&self, base: FuncIdentity, type_params: &[*const TyDesc]) -> Option<FuncIdentity> {
        let seen = self.by_descriptors.get(&base)?;
        let (_, id) = seen.iter().find(|(descriptors, _)| **descriptors == *type_params)?;
        id.map(|id| FuncIdentity::Instance { id })
    }

    /// The instance of `generic`, which is `base`, at `bindings`, made now if
    /// it has to be; `None` if the generic has as many as it may.
    pub fn by_types(&mut self, base: FuncIdentity, generic: &IrCodeUnit, bindings: Box<[IrType]>) -> Option<u32> {
        let key = (base, bindings);
        if let Some(&id) = self.by_types.get(&key) {
            return Some(id);
        }
        if self.units.values().filter(|i| i.base == base).count() >= MAX_INSTANCES_PER_FUNCTION {
            return None;
        }
        let id = self.next;
        self.next += 1;
        let mut unit = instantiate(generic, &key.1, generic.id, format!("{}__i{id}", generic.name));
        clean_copy_types(&mut unit);
        self.units.insert(id, Instance { base, bindings: key.1.clone(), unit: Arc::new(unit) });
        self.by_types.insert(key, id);
        Some(id)
    }

    /// Instance `id`, which this has made.
    pub fn instance(&self, id: u32) -> &Instance {
        &self.units[&id]
    }

    /// The binding `type_params` describe, read back exactly, or `None`.
    fn read_back(type_params: &[*const TyDesc], interp: &mut IrInterpreter) -> Option<Box<[IrType]>> {
        type_params.iter()
            .map(|&tydesc| {
                let ty = ir_type_of_tydesc(tydesc)?;
                // Read back exactly, or not at all: an instance at a type
                // that is not the one the call was made at would be wrong
                // code, where leaving the call to the generic costs a little.
                let again = interp.tydesc_table_mut().get_or_create(&ty);
                datalove_rt::c::same_tydesc(tydesc, again).then_some(ty)
            })
            .collect()
    }

    /// Forget the instances of the generics `which` picks, and say which they
    /// were.
    pub fn forget(&mut self, mut which: impl FnMut(&FuncIdentity) -> bool) -> FxHashSet<u32> {
        let gone: FxHashSet<u32> = self.units.iter()
            .filter(|(_, instance)| which(&instance.base))
            .map(|(id, _)| *id)
            .collect();
        self.units.retain(|id, _| !gone.contains(id));
        self.by_types.retain(|(base, _), _| !which(base));
        self.by_descriptors.retain(|base, _| !which(base));
        gone
    }
}
