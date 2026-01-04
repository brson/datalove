//! TyDesc emission as static object data.
//!
//! Emits type descriptors as static data in the object file,
//! using layout from `rtdt::TyDesc` directly.
//!
//! Since datalove does whole-world compilation, all types are known
//! ahead of time. TyDescs are emitted upfront in a single pass.

use std::collections::{HashMap, HashSet};
use std::mem::{align_of, offset_of, size_of};

use cranelift_module::{DataDescription, DataId, Linkage, Module};
use datalove_datafun_ir::{IrFunction, IrScriptUnit, IrType};
use datalove_rtdt::{
    Data as RtData, Error as RtError, Int as RtInt, String as RtString, TyDesc, TyTag,
};

use crate::AotError;

// TyDesc layout computed from runtime types.
const TYDESC_SIZE: usize = size_of::<TyDesc>();
const TYDESC_ALIGN: usize = align_of::<TyDesc>();
const OFFSET_TYPE_TAG: usize = offset_of!(TyDesc, type_tag);
const OFFSET_SIZE: usize = offset_of!(TyDesc, size);
const OFFSET_ALIGN: usize = offset_of!(TyDesc, align);
#[allow(dead_code)]
const OFFSET_TYPE_INFO: usize = offset_of!(TyDesc, type_info);

/// Emitter for TyDesc static data.
pub struct TyDescEmitter {
    /// Maps IrType -> DataId for emitted tydescs.
    tydescs: HashMap<IrType, DataId>,
    /// Counter for unique names.
    counter: u32,
}

impl TyDescEmitter {
    /// Create a new TyDesc emitter.
    pub fn new() -> Self {
        Self {
            tydescs: HashMap::new(),
            counter: 0,
        }
    }

    /// Emit a TyDesc for the given type, returning its DataId.
    ///
    /// Returns cached DataId if already emitted.
    pub fn emit<M: Module>(&mut self, module: &mut M, ty: &IrType) -> Result<DataId, AotError> {
        // Check cache.
        if let Some(&id) = self.tydescs.get(ty) {
            return Ok(id);
        }

        // Build TyDesc bytes.
        let bytes = self.build_tydesc_bytes(ty)?;

        // Create unique name.
        let name = format!("__tydesc_{}", self.counter);
        self.counter += 1;

        // Declare data.
        let data_id = module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| AotError::Module(format!("declare tydesc data: {}", e)))?;

        // Define data.
        let mut data_desc = DataDescription::new();
        data_desc.define(bytes.into_boxed_slice());
        data_desc.set_align(TYDESC_ALIGN as u64);

        module
            .define_data(data_id, &data_desc)
            .map_err(|e| AotError::Module(format!("define tydesc data: {}", e)))?;

        self.tydescs.insert(ty.clone(), data_id);
        Ok(data_id)
    }

    /// Emit TyDescs for all types upfront.
    ///
    /// Call this before codegen to populate the cache. After this,
    /// use `get()` for lookup-only access during codegen.
    pub fn emit_all<M: Module>(
        &mut self,
        module: &mut M,
        types: impl IntoIterator<Item = IrType>,
    ) -> Result<(), AotError> {
        for ty in types {
            // Skip types we can't emit - they'll error at use site if needed.
            if self.can_emit(&ty) {
                self.emit(module, &ty)?;
            }
        }
        Ok(())
    }

    /// Look up a previously-emitted TyDesc.
    ///
    /// Returns None if the type was not emitted. Panics are appropriate
    /// during codegen since all types should have been emitted upfront.
    pub fn get(&self, ty: &IrType) -> Option<DataId> {
        self.tydescs.get(ty).copied()
    }

    /// Check if a type can be emitted as a TyDesc.
    fn can_emit(&self, ty: &IrType) -> bool {
        matches!(
            ty,
            IrType::Unit
                | IrType::Bool
                | IrType::U8
                | IrType::I8
                | IrType::U16
                | IrType::I16
                | IrType::U32
                | IrType::I32
                | IrType::U64
                | IrType::I64
                | IrType::F32
                | IrType::Int
                | IrType::String
                | IrType::Data
                | IrType::Error
        )
    }

    /// Build the raw bytes for a TyDesc.
    fn build_tydesc_bytes(&self, ty: &IrType) -> Result<Vec<u8>, AotError> {
        let mut bytes = vec![0u8; TYDESC_SIZE];

        let (tag, size, align) = match ty {
            // Unit is empty tuple.
            IrType::Unit => (TyTag::Tuple as u8, 0u32, 1u32),
            IrType::Bool => (TyTag::Bool as u8, size_of::<bool>() as u32, align_of::<bool>() as u32),
            IrType::U8 => (TyTag::U8 as u8, size_of::<u8>() as u32, align_of::<u8>() as u32),
            IrType::I8 => (TyTag::I8 as u8, size_of::<i8>() as u32, align_of::<i8>() as u32),
            IrType::U16 => (TyTag::U16 as u8, size_of::<u16>() as u32, align_of::<u16>() as u32),
            IrType::I16 => (TyTag::I16 as u8, size_of::<i16>() as u32, align_of::<i16>() as u32),
            IrType::U32 => (TyTag::U32 as u8, size_of::<u32>() as u32, align_of::<u32>() as u32),
            IrType::I32 => (TyTag::I32 as u8, size_of::<i32>() as u32, align_of::<i32>() as u32),
            IrType::U64 => (TyTag::U64 as u8, size_of::<u64>() as u32, align_of::<u64>() as u32),
            IrType::I64 => (TyTag::I64 as u8, size_of::<i64>() as u32, align_of::<i64>() as u32),
            IrType::F32 => (TyTag::F32 as u8, size_of::<f32>() as u32, align_of::<f32>() as u32),
            IrType::Int => (TyTag::Int as u8, size_of::<RtInt>() as u32, align_of::<RtInt>() as u32),
            IrType::String => (TyTag::String as u8, size_of::<RtString>() as u32, align_of::<RtString>() as u32),
            IrType::Data => (TyTag::Data as u8, size_of::<RtData>() as u32, align_of::<RtData>() as u32),
            IrType::Error => (TyTag::Error as u8, size_of::<RtError>() as u32, align_of::<RtError>() as u32),
            _ => {
                return Err(AotError::Unsupported(format!(
                    "tydesc emission for type: {:?}",
                    ty
                )));
            }
        };

        // Write fields.
        bytes[OFFSET_TYPE_TAG] = tag;
        bytes[OFFSET_SIZE..OFFSET_SIZE + 4].copy_from_slice(&size.to_le_bytes());
        bytes[OFFSET_ALIGN..OFFSET_ALIGN + 4].copy_from_slice(&align.to_le_bytes());
        // type_info is zero-filled (nothing variant for scalars).

        Ok(bytes)
    }
}

impl Default for TyDescEmitter {
    fn default() -> Self {
        Self::new()
    }
}

/// Collect all types from a single function.
pub fn collect_types_from_function(func: &IrFunction, types: &mut HashSet<IrType>) {
    for ty in &func.param_types {
        types.insert(ty.clone());
    }
    for ty in &func.value_types {
        types.insert(ty.clone());
    }
    for ty in &func.slot_types {
        types.insert(ty.clone());
    }
}

/// Collect all types from a script unit for upfront TyDesc emission.
pub fn collect_types_from_script_unit(unit: &IrScriptUnit) -> HashSet<IrType> {
    let mut types = HashSet::new();

    // Collect from unit's value and slot types.
    for ty in &unit.value_types {
        types.insert(ty.clone());
    }
    for ty in &unit.slot_types {
        types.insert(ty.clone());
    }

    // Collect from each function's types.
    for func in &unit.functions {
        collect_types_from_function(func, &mut types);
    }

    types
}

/// Collect types from an iterator of functions.
///
/// Use this to collect types from module functions in a ScriptEnvironment.
pub fn collect_types_from_functions<'a>(
    funcs: impl Iterator<Item = &'a IrFunction>,
    types: &mut HashSet<IrType>,
) {
    for func in funcs {
        collect_types_from_function(func, types);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift_codegen::isa;
    use cranelift_codegen::settings::{self, Configurable};
    use cranelift_object::{ObjectBuilder, ObjectModule};
    use target_lexicon::Triple;

    fn create_test_module() -> ObjectModule {
        let mut settings_builder = settings::builder();
        settings_builder.set("opt_level", "speed").unwrap();
        let flags = settings::Flags::new(settings_builder);

        let isa = isa::lookup(Triple::host())
            .unwrap()
            .finish(flags)
            .unwrap();

        let obj_builder = ObjectBuilder::new(
            isa,
            "test",
            cranelift_module::default_libcall_names(),
        ).unwrap();

        ObjectModule::new(obj_builder)
    }

    #[test]
    fn test_emit_scalar_tydescs() {
        let mut module = create_test_module();
        let mut emitter = TyDescEmitter::new();

        // Emit various scalar types.
        let types = vec![
            IrType::Bool,
            IrType::U8,
            IrType::I32,
            IrType::U64,
            IrType::F32,
        ];

        for ty in types {
            let result = emitter.emit(&mut module, &ty);
            assert!(result.is_ok(), "failed to emit tydesc for {:?}: {:?}", ty, result.err());
        }
    }

    #[test]
    fn test_tydesc_caching() {
        let mut module = create_test_module();
        let mut emitter = TyDescEmitter::new();

        // Emit same type twice.
        let id1 = emitter.emit(&mut module, &IrType::I32).unwrap();
        let id2 = emitter.emit(&mut module, &IrType::I32).unwrap();

        // Should return same DataId.
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_tydesc_bytes_layout() {
        let emitter = TyDescEmitter::new();

        // Check I32 layout.
        let bytes = emitter.build_tydesc_bytes(&IrType::I32).unwrap();
        assert_eq!(bytes.len(), TYDESC_SIZE);
        assert_eq!(bytes[OFFSET_TYPE_TAG], TyTag::I32 as u8);

        // Check size field (little-endian u32).
        let size = u32::from_le_bytes([
            bytes[OFFSET_SIZE],
            bytes[OFFSET_SIZE + 1],
            bytes[OFFSET_SIZE + 2],
            bytes[OFFSET_SIZE + 3],
        ]);
        assert_eq!(size, size_of::<i32>() as u32);

        // Check align field.
        let align = u32::from_le_bytes([
            bytes[OFFSET_ALIGN],
            bytes[OFFSET_ALIGN + 1],
            bytes[OFFSET_ALIGN + 2],
            bytes[OFFSET_ALIGN + 3],
        ]);
        assert_eq!(align, align_of::<i32>() as u32);
    }
}
