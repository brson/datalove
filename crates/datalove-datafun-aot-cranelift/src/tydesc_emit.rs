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
    Data as RtData, Error as RtError, Int as RtInt, List as RtList, Map as RtMap,
    Set as RtSet, String as RtString, TyDesc, TyInfoList, TyInfoMap, TyInfoSet, TyTag,
};

use crate::AotError;

// Offsets within TyInfo union for collection types.
// TyInfoList: element_tydesc at offset 0
// TyInfoSet: element_tydesc at offset 0
// TyInfoMap: key_tydesc at offset 0, value_tydesc at offset 8 (pointer size)
const TYINFO_LIST_ELEMENT_OFFSET: usize = std::mem::offset_of!(TyInfoList, element_tydesc);
const TYINFO_SET_ELEMENT_OFFSET: usize = std::mem::offset_of!(TyInfoSet, element_tydesc);
const TYINFO_MAP_KEY_OFFSET: usize = std::mem::offset_of!(TyInfoMap, key_tydesc);
const TYINFO_MAP_VALUE_OFFSET: usize = std::mem::offset_of!(TyInfoMap, value_tydesc);

// TyDesc layout computed from runtime types.
const TYDESC_SIZE: usize = size_of::<TyDesc>();
const TYDESC_ALIGN: usize = align_of::<TyDesc>();
const OFFSET_TYPE_TAG: usize = offset_of!(TyDesc, type_tag);
const OFFSET_SIZE: usize = offset_of!(TyDesc, size);
const OFFSET_ALIGN: usize = offset_of!(TyDesc, align);
#[allow(dead_code)]
const OFFSET_TYPE_INFO: usize = offset_of!(TyDesc, type_info);

/// Emitter for TyDesc static data.
#[derive(Clone)]
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

        // Handle collection types with element type references.
        match ty {
            IrType::List(elem_ty) => {
                return self.emit_list_tydesc(module, elem_ty);
            }
            IrType::Set(elem_ty) => {
                return self.emit_set_tydesc(module, elem_ty);
            }
            IrType::Map(key_ty, val_ty) => {
                return self.emit_map_tydesc(module, key_ty, val_ty);
            }
            _ => {}
        }

        // Build TyDesc bytes for simple types.
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

    /// Emit a TyDesc for a List type with element type reference.
    fn emit_list_tydesc<M: Module>(
        &mut self,
        module: &mut M,
        elem_ty: &IrType,
    ) -> Result<DataId, AotError> {
        let list_ty = IrType::List(Box::new(elem_ty.clone()));

        // Check cache (may have been emitted during recursive call).
        if let Some(&id) = self.tydescs.get(&list_ty) {
            return Ok(id);
        }

        // First, emit the element type TyDesc.
        let elem_tydesc_id = self.emit(module, elem_ty)?;

        // Build base TyDesc bytes.
        let mut bytes = vec![0u8; TYDESC_SIZE];
        let tag = TyTag::List as u8;
        let size = size_of::<RtList>() as u32;
        let align = align_of::<RtList>() as u32;

        bytes[OFFSET_TYPE_TAG] = tag;
        bytes[OFFSET_SIZE..OFFSET_SIZE + 4].copy_from_slice(&size.to_le_bytes());
        bytes[OFFSET_ALIGN..OFFSET_ALIGN + 4].copy_from_slice(&align.to_le_bytes());

        // Create unique name.
        let name = format!("__tydesc_{}", self.counter);
        self.counter += 1;

        // Declare data.
        let data_id = module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| AotError::Module(format!("declare list tydesc: {}", e)))?;

        // Define data with relocation to element tydesc.
        let mut data_desc = DataDescription::new();
        data_desc.define(bytes.into_boxed_slice());
        data_desc.set_align(TYDESC_ALIGN as u64);

        // Add relocation for element_tydesc pointer in type_info.
        let elem_gv = module.declare_data_in_data(elem_tydesc_id, &mut data_desc);
        let elem_offset = OFFSET_TYPE_INFO + TYINFO_LIST_ELEMENT_OFFSET;
        data_desc.write_data_addr(elem_offset as u32, elem_gv, 0);

        module
            .define_data(data_id, &data_desc)
            .map_err(|e| AotError::Module(format!("define list tydesc: {}", e)))?;

        self.tydescs.insert(list_ty, data_id);
        Ok(data_id)
    }

    /// Emit a TyDesc for a Set type with element type reference.
    fn emit_set_tydesc<M: Module>(
        &mut self,
        module: &mut M,
        elem_ty: &IrType,
    ) -> Result<DataId, AotError> {
        let set_ty = IrType::Set(Box::new(elem_ty.clone()));

        // Check cache.
        if let Some(&id) = self.tydescs.get(&set_ty) {
            return Ok(id);
        }

        // First, emit the element type TyDesc.
        let elem_tydesc_id = self.emit(module, elem_ty)?;

        // Build base TyDesc bytes.
        let mut bytes = vec![0u8; TYDESC_SIZE];
        let tag = TyTag::Set as u8;
        let size = size_of::<RtSet>() as u32;
        let align = align_of::<RtSet>() as u32;

        bytes[OFFSET_TYPE_TAG] = tag;
        bytes[OFFSET_SIZE..OFFSET_SIZE + 4].copy_from_slice(&size.to_le_bytes());
        bytes[OFFSET_ALIGN..OFFSET_ALIGN + 4].copy_from_slice(&align.to_le_bytes());

        // Create unique name.
        let name = format!("__tydesc_{}", self.counter);
        self.counter += 1;

        // Declare data.
        let data_id = module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| AotError::Module(format!("declare set tydesc: {}", e)))?;

        // Define data with relocation to element tydesc.
        let mut data_desc = DataDescription::new();
        data_desc.define(bytes.into_boxed_slice());
        data_desc.set_align(TYDESC_ALIGN as u64);

        // Add relocation for element_tydesc pointer in type_info.
        let elem_gv = module.declare_data_in_data(elem_tydesc_id, &mut data_desc);
        let elem_offset = OFFSET_TYPE_INFO + TYINFO_SET_ELEMENT_OFFSET;
        data_desc.write_data_addr(elem_offset as u32, elem_gv, 0);

        module
            .define_data(data_id, &data_desc)
            .map_err(|e| AotError::Module(format!("define set tydesc: {}", e)))?;

        self.tydescs.insert(set_ty, data_id);
        Ok(data_id)
    }

    /// Emit a TyDesc for a Map type with key and value type references.
    fn emit_map_tydesc<M: Module>(
        &mut self,
        module: &mut M,
        key_ty: &IrType,
        val_ty: &IrType,
    ) -> Result<DataId, AotError> {
        let map_ty = IrType::Map(Box::new(key_ty.clone()), Box::new(val_ty.clone()));

        // Check cache.
        if let Some(&id) = self.tydescs.get(&map_ty) {
            return Ok(id);
        }

        // First, emit the key and value type TyDescs.
        let key_tydesc_id = self.emit(module, key_ty)?;
        let val_tydesc_id = self.emit(module, val_ty)?;

        // Build base TyDesc bytes.
        let mut bytes = vec![0u8; TYDESC_SIZE];
        let tag = TyTag::Map as u8;
        let size = size_of::<RtMap>() as u32;
        let align = align_of::<RtMap>() as u32;

        bytes[OFFSET_TYPE_TAG] = tag;
        bytes[OFFSET_SIZE..OFFSET_SIZE + 4].copy_from_slice(&size.to_le_bytes());
        bytes[OFFSET_ALIGN..OFFSET_ALIGN + 4].copy_from_slice(&align.to_le_bytes());

        // Create unique name.
        let name = format!("__tydesc_{}", self.counter);
        self.counter += 1;

        // Declare data.
        let data_id = module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| AotError::Module(format!("declare map tydesc: {}", e)))?;

        // Define data with relocations to key and value tydescs.
        let mut data_desc = DataDescription::new();
        data_desc.define(bytes.into_boxed_slice());
        data_desc.set_align(TYDESC_ALIGN as u64);

        // Add relocation for key_tydesc pointer in type_info.
        let key_gv = module.declare_data_in_data(key_tydesc_id, &mut data_desc);
        let key_offset = OFFSET_TYPE_INFO + TYINFO_MAP_KEY_OFFSET;
        data_desc.write_data_addr(key_offset as u32, key_gv, 0);

        // Add relocation for value_tydesc pointer in type_info.
        let val_gv = module.declare_data_in_data(val_tydesc_id, &mut data_desc);
        let val_offset = OFFSET_TYPE_INFO + TYINFO_MAP_VALUE_OFFSET;
        data_desc.write_data_addr(val_offset as u32, val_gv, 0);

        module
            .define_data(data_id, &data_desc)
            .map_err(|e| AotError::Module(format!("define map tydesc: {}", e)))?;

        self.tydescs.insert(map_ty, data_id);
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
        match ty {
            // Simple types.
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
            | IrType::Error => true,

            // Collection types - can emit if element types can be emitted.
            IrType::List(elem_ty) => self.can_emit(elem_ty),
            IrType::Set(elem_ty) => self.can_emit(elem_ty),
            IrType::Map(key_ty, val_ty) => self.can_emit(key_ty) && self.can_emit(val_ty),

            _ => false,
        }
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
