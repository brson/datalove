//! TyDesc emission as static object data.
//!
//! Emits type descriptors as static data in the object file,
//! matching the runtime's `rtdt::TyDesc` layout exactly.
//!
//! Since datalove does whole-world compilation, all types are known
//! ahead of time. TyDescs are emitted upfront in a single pass.

use std::collections::{HashMap, HashSet};

use cranelift_module::{DataDescription, DataId, Linkage, Module};
use datalove_datafun_ir::{IrFunction, IrScriptUnit, IrType};

use crate::AotError;

/// TyDesc layout constants (must match rtdt::TyDesc repr(C)).
const TYDESC_SIZE: usize = 32;
const TYDESC_ALIGN: usize = 8;

/// Offset of type_tag field.
const OFFSET_TYPE_TAG: usize = 0;
/// Offset of size field (after 3 bytes padding).
const OFFSET_SIZE: usize = 4;
/// Offset of align field.
const OFFSET_ALIGN: usize = 8;
/// Offset of type_info union (after 4 bytes padding for 8-byte alignment).
const OFFSET_TYPE_INFO: usize = 16;

/// TyTag values (must match rtdt::TyTag repr(u8)).
mod ty_tag {
    pub const BOOL: u8 = 0x01;
    pub const U8: u8 = 0x10;
    pub const I8: u8 = 0x11;
    pub const U16: u8 = 0x12;
    pub const I16: u8 = 0x13;
    pub const U32: u8 = 0x14;
    pub const I32: u8 = 0x15;
    pub const U64: u8 = 0x16;
    pub const I64: u8 = 0x17;
    pub const F32: u8 = 0x20;
    pub const INT: u8 = 0x30;
    pub const STRING: u8 = 0x51;
    pub const TUPLE: u8 = 0x40;
    pub const DATA: u8 = 0x70;
    pub const ERROR: u8 = 0x71;
}

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
            IrType::Unit => (ty_tag::TUPLE, 0u32, 1u32), // Unit is empty tuple
            IrType::Bool => (ty_tag::BOOL, 1, 1),
            IrType::U8 => (ty_tag::U8, 1, 1),
            IrType::I8 => (ty_tag::I8, 1, 1),
            IrType::U16 => (ty_tag::U16, 2, 2),
            IrType::I16 => (ty_tag::I16, 2, 2),
            IrType::U32 => (ty_tag::U32, 4, 4),
            IrType::I32 => (ty_tag::I32, 4, 4),
            IrType::U64 => (ty_tag::U64, 8, 8),
            IrType::I64 => (ty_tag::I64, 8, 8),
            IrType::F32 => (ty_tag::F32, 4, 4),
            IrType::Int => (ty_tag::INT, 16, 8), // size_of::<rtdt::Int>()
            IrType::String => (ty_tag::STRING, 16, 8), // size_of::<rtdt::String>()
            IrType::Data => (ty_tag::DATA, 16, 8),
            IrType::Error => (ty_tag::ERROR, 16, 8),
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
        assert_eq!(bytes[OFFSET_TYPE_TAG], ty_tag::I32);

        // Check size field (little-endian u32).
        let size = u32::from_le_bytes([
            bytes[OFFSET_SIZE],
            bytes[OFFSET_SIZE + 1],
            bytes[OFFSET_SIZE + 2],
            bytes[OFFSET_SIZE + 3],
        ]);
        assert_eq!(size, 4);

        // Check align field.
        let align = u32::from_le_bytes([
            bytes[OFFSET_ALIGN],
            bytes[OFFSET_ALIGN + 1],
            bytes[OFFSET_ALIGN + 2],
            bytes[OFFSET_ALIGN + 3],
        ]);
        assert_eq!(align, 4);
    }
}
