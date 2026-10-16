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
use datalove_datafun_ir::instantiate::instantiate;
use datalove_datafun_ir::{IrCodeUnit, IrType};
use datalove_rtdt::TyDesc;

/// Most run-time instances one generic may have; a binding past it runs the
/// generic.
pub const MAX_INSTANCES_PER_FUNCTION: usize = 64;

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
    /// Each instance's generic and body.
    units: FxHashMap<u32, (FuncIdentity, Arc<IrCodeUnit>)>,
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
                let id = Self::make(&mut self.by_types, &mut self.units, &mut self.next, base, generic, type_params, interp);
                self.by_descriptors.get_mut(&base).expect("entered above").push((type_params.into(), id));
                id
            }
        }?;
        let (_, unit) = &self.units[&id];
        Some((FuncIdentity::Instance { id }, Arc::clone(unit)))
    }

    fn make(
        by_types: &mut FxHashMap<(FuncIdentity, Box<[IrType]>), u32>,
        units: &mut FxHashMap<u32, (FuncIdentity, Arc<IrCodeUnit>)>,
        next: &mut u32,
        base: FuncIdentity,
        generic: &IrCodeUnit,
        type_params: &[*const TyDesc],
        interp: &mut IrInterpreter,
    ) -> Option<u32> {
        let bindings: Box<[IrType]> = type_params.iter()
            .map(|&tydesc| {
                let ty = ir_type_of_tydesc(tydesc)?;
                // Read back exactly, or not at all: an instance at a type
                // that is not the one the call was made at would be wrong
                // code, where leaving the call to the generic costs a little.
                let again = interp.tydesc_table_mut().get_or_create(&ty);
                datalove_rt::c::same_tydesc(tydesc, again).then_some(ty)
            })
            .collect::<Option<_>>()?;
        let key = (base, bindings);
        if let Some(&id) = by_types.get(&key) {
            return Some(id);
        }
        if units.values().filter(|(b, _)| *b == base).count() >= MAX_INSTANCES_PER_FUNCTION {
            return None;
        }
        let id = *next;
        *next += 1;
        let unit = instantiate(generic, &key.1, generic.id, format!("{}__i{id}", generic.name));
        units.insert(id, (base, Arc::new(unit)));
        by_types.insert(key, id);
        Some(id)
    }

    /// Forget the instances of the generics `which` picks, and say which they
    /// were.
    pub fn forget(&mut self, mut which: impl FnMut(&FuncIdentity) -> bool) -> FxHashSet<u32> {
        let gone: FxHashSet<u32> = self.units.iter()
            .filter(|(_, (base, _))| which(base))
            .map(|(id, _)| *id)
            .collect();
        self.units.retain(|id, _| !gone.contains(id));
        self.by_types.retain(|(base, _), _| !which(base));
        self.by_descriptors.retain(|base, _| !which(base));
        gone
    }
}
