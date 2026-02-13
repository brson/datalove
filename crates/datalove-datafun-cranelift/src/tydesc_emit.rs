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
use datalove_datafun_ir::{IrCodeUnit, IrType};
use datalove_rtdt::{
    Data as RtData, Error as RtError, Int as RtInt, List as RtList, Map as RtMap,
    Set as RtSet, String as RtString, Table as RtTable, Tensor as RtTensor, TyDesc,
    TyInfoEnum, TyInfoEnumVariant, TyInfoList, TyInfoMap, TyInfoOption, TyInfoResult,
    TyInfoSet, TyInfoStruct, TyInfoStructField, TyInfoTable, TyInfoTableColumn, TyInfoTensor,
    TyInfoTuple, TyInfoTupleField, TyTag,
};

use crate::CraneliftError;

// Offsets within TyInfo union for collection types.
// TyInfoList: element_tydesc at offset 0
// TyInfoSet: element_tydesc at offset 0
// TyInfoMap: key_tydesc at offset 0, value_tydesc at offset 8 (pointer size)
const TYINFO_LIST_ELEMENT_OFFSET: usize = std::mem::offset_of!(TyInfoList, element_tydesc);
const TYINFO_SET_ELEMENT_OFFSET: usize = std::mem::offset_of!(TyInfoSet, element_tydesc);
const TYINFO_MAP_KEY_OFFSET: usize = std::mem::offset_of!(TyInfoMap, key_tydesc);
const TYINFO_MAP_VALUE_OFFSET: usize = std::mem::offset_of!(TyInfoMap, value_tydesc);

// Offsets within TyInfo union for Tensor type.
const TYINFO_TENSOR_ELEMENT_OFFSET: usize = std::mem::offset_of!(TyInfoTensor, element_tydesc);
const TYINFO_TENSOR_RANK_OFFSET: usize = std::mem::offset_of!(TyInfoTensor, rank);

// Offsets within TyInfo union for Option/Result types.
const TYINFO_OPTION_INNER_OFFSET: usize = std::mem::offset_of!(TyInfoOption, inner_tydesc);
const TYINFO_RESULT_OK_OFFSET: usize = std::mem::offset_of!(TyInfoResult, ok_tydesc);

// Offsets within TyInfo union for Tuple type.
const TYINFO_TUPLE_NUM_FIELDS_OFFSET: usize = std::mem::offset_of!(TyInfoTuple, num_fields);
const TYINFO_TUPLE_FIELDS_OFFSET: usize = std::mem::offset_of!(TyInfoTuple, fields);

// TyInfoTupleField layout.
const TYINFO_TUPLE_FIELD_SIZE: usize = size_of::<TyInfoTupleField>();
const TYINFO_TUPLE_FIELD_OFFSET_OFFSET: usize = std::mem::offset_of!(TyInfoTupleField, offset);
const TYINFO_TUPLE_FIELD_TYDESC_OFFSET: usize = std::mem::offset_of!(TyInfoTupleField, tydesc);

// Offsets within TyInfo union for Struct type.
const TYINFO_STRUCT_FIELDS_OFFSET: usize = std::mem::offset_of!(TyInfoStruct, fields);
const TYINFO_STRUCT_NUM_FIELDS_OFFSET: usize = std::mem::offset_of!(TyInfoStruct, num_fields);

// TyInfoStructField layout.
const TYINFO_STRUCT_FIELD_SIZE: usize = size_of::<TyInfoStructField>();
const TYINFO_STRUCT_FIELD_NAME_OFFSET: usize = std::mem::offset_of!(TyInfoStructField, name);
const TYINFO_STRUCT_FIELD_NAME_LEN_OFFSET: usize = std::mem::offset_of!(TyInfoStructField, name_len);
const TYINFO_STRUCT_FIELD_OFFSET_OFFSET: usize = std::mem::offset_of!(TyInfoStructField, offset);
const TYINFO_STRUCT_FIELD_TYDESC_OFFSET: usize = std::mem::offset_of!(TyInfoStructField, tydesc);

// Offsets within TyInfo union for Enum type.
const TYINFO_ENUM_VARIANTS_OFFSET: usize = std::mem::offset_of!(TyInfoEnum, variants);
const TYINFO_ENUM_NUM_VARIANTS_OFFSET: usize = std::mem::offset_of!(TyInfoEnum, num_variants);

// TyInfoEnumVariant layout.
const TYINFO_ENUM_VARIANT_SIZE: usize = size_of::<TyInfoEnumVariant>();
const TYINFO_ENUM_VARIANT_NAME_OFFSET: usize = std::mem::offset_of!(TyInfoEnumVariant, name);
const TYINFO_ENUM_VARIANT_NAME_LEN_OFFSET: usize = std::mem::offset_of!(TyInfoEnumVariant, name_len);
const TYINFO_ENUM_VARIANT_OFFSET_OFFSET: usize = std::mem::offset_of!(TyInfoEnumVariant, offset);
const TYINFO_ENUM_VARIANT_PAYLOAD_OFFSET: usize = std::mem::offset_of!(TyInfoEnumVariant, payload);

// Offsets within TyInfo union for Table type.
const TYINFO_TABLE_NUM_COLUMNS_OFFSET: usize = std::mem::offset_of!(TyInfoTable, num_columns);
const TYINFO_TABLE_COLUMNS_OFFSET: usize = std::mem::offset_of!(TyInfoTable, columns);

// TyInfoTableColumn layout.
const TYINFO_TABLE_COLUMN_SIZE: usize = size_of::<TyInfoTableColumn>();
const TYINFO_TABLE_COLUMN_NAME_OFFSET: usize = std::mem::offset_of!(TyInfoTableColumn, name);
const TYINFO_TABLE_COLUMN_NAME_LEN_OFFSET: usize = std::mem::offset_of!(TyInfoTableColumn, name_len);
const TYINFO_TABLE_COLUMN_TYDESC_OFFSET: usize = std::mem::offset_of!(TyInfoTableColumn, tydesc);

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
    pub fn emit<M: Module>(&mut self, module: &mut M, ty: &IrType) -> Result<DataId, CraneliftError> {
        // Check cache.
        if let Some(&id) = self.tydescs.get(ty) {
            return Ok(id);
        }

        // Handle types with inner type references.
        match ty {
            IrType::Unit => {
                // Unit is an empty tuple.
                return self.emit_tuple_tydesc(module, ty, &[]);
            }
            IrType::List(elem_ty) => {
                return self.emit_list_tydesc(module, elem_ty);
            }
            IrType::Set(elem_ty) => {
                return self.emit_set_tydesc(module, elem_ty);
            }
            IrType::Map(key_ty, val_ty) => {
                return self.emit_map_tydesc(module, key_ty, val_ty);
            }
            IrType::Tensor(elem_ty, rank) => {
                return self.emit_tensor_tydesc(module, elem_ty, *rank);
            }
            IrType::Option(inner_ty) => {
                return self.emit_option_tydesc(module, inner_ty);
            }
            IrType::Result(ok_ty) => {
                return self.emit_result_tydesc(module, ok_ty);
            }
            IrType::Tuple(field_types) => {
                return self.emit_tuple_tydesc(module, ty, field_types);
            }
            IrType::Struct(fields) => {
                return self.emit_struct_tydesc(module, ty, fields);
            }
            IrType::Enum(variants) => {
                return self.emit_enum_tydesc(module, ty, variants);
            }
            IrType::Atom(name) => {
                return self.emit_atom_tydesc(module, ty, name);
            }
            IrType::Term(name, payload) => {
                return self.emit_term_tydesc(module, ty, name, payload);
            }
            IrType::Ref(inner_ty) => {
                // Ref is a pointer to the inner type. Emit the inner type's tydesc
                // first (for when reading through the ref), then emit a pointer-sized
                // tydesc for the ref itself.
                let _inner_id = self.emit(module, inner_ty)?;
                return self.emit_ref_tydesc(module, ty, inner_ty);
            }
            IrType::Table(columns) => {
                return self.emit_table_tydesc(module, ty, columns);
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
            .map_err(|e| CraneliftError::Module(format!("declare tydesc data: {}", e)))?;

        // Define data.
        let mut data_desc = DataDescription::new();
        data_desc.define(bytes.into_boxed_slice());
        data_desc.set_align(TYDESC_ALIGN as u64);

        module
            .define_data(data_id, &data_desc)
            .map_err(|e| CraneliftError::Module(format!("define tydesc data: {}", e)))?;

        self.tydescs.insert(ty.clone(), data_id);
        Ok(data_id)
    }

    /// Emit a TyDesc for a List type with element type reference.
    fn emit_list_tydesc<M: Module>(
        &mut self,
        module: &mut M,
        elem_ty: &IrType,
    ) -> Result<DataId, CraneliftError> {
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
            .map_err(|e| CraneliftError::Module(format!("declare list tydesc: {}", e)))?;

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
            .map_err(|e| CraneliftError::Module(format!("define list tydesc: {}", e)))?;

        self.tydescs.insert(list_ty, data_id);
        Ok(data_id)
    }

    /// Emit a TyDesc for a Set type with element type reference.
    fn emit_set_tydesc<M: Module>(
        &mut self,
        module: &mut M,
        elem_ty: &IrType,
    ) -> Result<DataId, CraneliftError> {
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
            .map_err(|e| CraneliftError::Module(format!("declare set tydesc: {}", e)))?;

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
            .map_err(|e| CraneliftError::Module(format!("define set tydesc: {}", e)))?;

        self.tydescs.insert(set_ty, data_id);
        Ok(data_id)
    }

    /// Emit a TyDesc for a Map type with key and value type references.
    fn emit_map_tydesc<M: Module>(
        &mut self,
        module: &mut M,
        key_ty: &IrType,
        val_ty: &IrType,
    ) -> Result<DataId, CraneliftError> {
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
            .map_err(|e| CraneliftError::Module(format!("declare map tydesc: {}", e)))?;

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
            .map_err(|e| CraneliftError::Module(format!("define map tydesc: {}", e)))?;

        self.tydescs.insert(map_ty, data_id);
        Ok(data_id)
    }

    /// Emit a TyDesc for a Tensor type with element type reference and rank.
    fn emit_tensor_tydesc<M: Module>(
        &mut self,
        module: &mut M,
        elem_ty: &IrType,
        rank: u32,
    ) -> Result<DataId, CraneliftError> {
        let tensor_ty = IrType::Tensor(Box::new(elem_ty.clone()), rank);

        // Check cache.
        if let Some(&id) = self.tydescs.get(&tensor_ty) {
            return Ok(id);
        }

        // First, emit the element type TyDesc.
        let elem_tydesc_id = self.emit(module, elem_ty)?;

        // Build base TyDesc bytes.
        let mut bytes = vec![0u8; TYDESC_SIZE];
        let tag = TyTag::Tensor as u8;
        let size = size_of::<RtTensor>() as u32;
        let align = align_of::<RtTensor>() as u32;

        bytes[OFFSET_TYPE_TAG] = tag;
        bytes[OFFSET_SIZE..OFFSET_SIZE + 4].copy_from_slice(&size.to_le_bytes());
        bytes[OFFSET_ALIGN..OFFSET_ALIGN + 4].copy_from_slice(&align.to_le_bytes());

        // Write rank in type_info.
        let rank_offset = OFFSET_TYPE_INFO + TYINFO_TENSOR_RANK_OFFSET;
        bytes[rank_offset..rank_offset + 4].copy_from_slice(&rank.to_le_bytes());

        // Create unique name.
        let name = format!("__tydesc_{}", self.counter);
        self.counter += 1;

        // Declare data.
        let data_id = module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| CraneliftError::Module(format!("declare tensor tydesc: {}", e)))?;

        // Define data with relocation to element tydesc.
        let mut data_desc = DataDescription::new();
        data_desc.define(bytes.into_boxed_slice());
        data_desc.set_align(TYDESC_ALIGN as u64);

        // Add relocation for element_tydesc pointer in type_info.
        let elem_gv = module.declare_data_in_data(elem_tydesc_id, &mut data_desc);
        let elem_offset = OFFSET_TYPE_INFO + TYINFO_TENSOR_ELEMENT_OFFSET;
        data_desc.write_data_addr(elem_offset as u32, elem_gv, 0);

        module
            .define_data(data_id, &data_desc)
            .map_err(|e| CraneliftError::Module(format!("define tensor tydesc: {}", e)))?;

        self.tydescs.insert(tensor_ty, data_id);
        Ok(data_id)
    }

    /// Emit a TyDesc for an Option type with inner type reference.
    fn emit_option_tydesc<M: Module>(
        &mut self,
        module: &mut M,
        inner_ty: &IrType,
    ) -> Result<DataId, CraneliftError> {
        let option_ty = IrType::Option(Box::new(inner_ty.clone()));

        // Check cache.
        if let Some(&id) = self.tydescs.get(&option_ty) {
            return Ok(id);
        }

        // First, emit the inner type TyDesc.
        let inner_tydesc_id = self.emit(module, inner_ty)?;

        // Compute Option layout.
        let inner_layout = crate::types::ir_type_to_cranelift(inner_ty).layout();
        let tag_size = 1u32;
        let payload_offset = crate::types::align_up(tag_size, inner_layout.align);
        let overall_align = inner_layout.align.max(1);
        let total_size = crate::types::align_up(payload_offset + inner_layout.size, overall_align);

        // Build base TyDesc bytes.
        let mut bytes = vec![0u8; TYDESC_SIZE];
        let tag = TyTag::Option as u8;

        bytes[OFFSET_TYPE_TAG] = tag;
        bytes[OFFSET_SIZE..OFFSET_SIZE + 4].copy_from_slice(&total_size.to_le_bytes());
        bytes[OFFSET_ALIGN..OFFSET_ALIGN + 4].copy_from_slice(&overall_align.to_le_bytes());

        // Create unique name.
        let name = format!("__tydesc_{}", self.counter);
        self.counter += 1;

        // Declare data.
        let data_id = module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| CraneliftError::Module(format!("declare option tydesc: {}", e)))?;

        // Define data with relocation to inner tydesc.
        let mut data_desc = DataDescription::new();
        data_desc.define(bytes.into_boxed_slice());
        data_desc.set_align(TYDESC_ALIGN as u64);

        // Add relocation for inner_tydesc pointer in type_info.
        let inner_gv = module.declare_data_in_data(inner_tydesc_id, &mut data_desc);
        let inner_offset = OFFSET_TYPE_INFO + TYINFO_OPTION_INNER_OFFSET;
        data_desc.write_data_addr(inner_offset as u32, inner_gv, 0);

        module
            .define_data(data_id, &data_desc)
            .map_err(|e| CraneliftError::Module(format!("define option tydesc: {}", e)))?;

        self.tydescs.insert(option_ty, data_id);
        Ok(data_id)
    }

    /// Emit a TyDesc for a Result type with ok type reference.
    fn emit_result_tydesc<M: Module>(
        &mut self,
        module: &mut M,
        ok_ty: &IrType,
    ) -> Result<DataId, CraneliftError> {
        let result_ty = IrType::Result(Box::new(ok_ty.clone()));

        // Check cache.
        if let Some(&id) = self.tydescs.get(&result_ty) {
            return Ok(id);
        }

        // First, emit the ok type TyDesc.
        let ok_tydesc_id = self.emit(module, ok_ty)?;

        // Compute Result layout.
        let ok_layout = crate::types::ir_type_to_cranelift(ok_ty).layout();
        let error_size = size_of::<RtError>() as u32;
        let error_align = align_of::<RtError>() as u32;
        let tag_size = 1u32;
        let max_payload_size = ok_layout.size.max(error_size);
        let max_payload_align = ok_layout.align.max(error_align);
        let payload_offset = crate::types::align_up(tag_size, max_payload_align);
        let overall_align = max_payload_align.max(1);
        let total_size = crate::types::align_up(payload_offset + max_payload_size, overall_align);

        // Build base TyDesc bytes.
        let mut bytes = vec![0u8; TYDESC_SIZE];
        let tag = TyTag::Result as u8;

        bytes[OFFSET_TYPE_TAG] = tag;
        bytes[OFFSET_SIZE..OFFSET_SIZE + 4].copy_from_slice(&total_size.to_le_bytes());
        bytes[OFFSET_ALIGN..OFFSET_ALIGN + 4].copy_from_slice(&overall_align.to_le_bytes());

        // Create unique name.
        let name = format!("__tydesc_{}", self.counter);
        self.counter += 1;

        // Declare data.
        let data_id = module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| CraneliftError::Module(format!("declare result tydesc: {}", e)))?;

        // Define data with relocation to ok tydesc.
        let mut data_desc = DataDescription::new();
        data_desc.define(bytes.into_boxed_slice());
        data_desc.set_align(TYDESC_ALIGN as u64);

        // Add relocation for ok_tydesc pointer in type_info.
        let ok_gv = module.declare_data_in_data(ok_tydesc_id, &mut data_desc);
        let ok_offset = OFFSET_TYPE_INFO + TYINFO_RESULT_OK_OFFSET;
        data_desc.write_data_addr(ok_offset as u32, ok_gv, 0);

        module
            .define_data(data_id, &data_desc)
            .map_err(|e| CraneliftError::Module(format!("define result tydesc: {}", e)))?;

        self.tydescs.insert(result_ty, data_id);
        Ok(data_id)
    }

    /// Emit a TyDesc for a Ref type (pointer to inner type).
    ///
    /// Ref is represented as a 1-element tuple containing the inner type's tydesc,
    /// but with pointer size/align. This allows getting the inner tydesc when
    /// reading through the ref.
    fn emit_ref_tydesc<M: Module>(
        &mut self,
        module: &mut M,
        ref_ty: &IrType,
        inner_ty: &IrType,
    ) -> Result<DataId, CraneliftError> {
        // Check cache.
        if let Some(&id) = self.tydescs.get(ref_ty) {
            return Ok(id);
        }

        // Get inner type's tydesc (should already be emitted).
        let inner_tydesc_id = self.tydescs.get(inner_ty)
            .copied()
            .ok_or_else(|| CraneliftError::Codegen("inner tydesc not found for ref".into()))?;

        // Create the fields array with 1 field (the inner type).
        let fields_size = TYINFO_TUPLE_FIELD_SIZE;
        let mut fields_bytes = vec![0u8; fields_size];

        // Write offset field (0 for single field).
        fields_bytes[TYINFO_TUPLE_FIELD_OFFSET_OFFSET..TYINFO_TUPLE_FIELD_OFFSET_OFFSET + 4]
            .copy_from_slice(&0u32.to_le_bytes());

        let fields_name = format!("__tydesc_ref_fields_{}", self.counter);
        self.counter += 1;

        let fields_id = module
            .declare_data(&fields_name, Linkage::Local, false, false)
            .map_err(|e| CraneliftError::Module(format!("declare ref fields: {}", e)))?;

        let mut fields_desc = DataDescription::new();
        fields_desc.define(fields_bytes.into_boxed_slice());
        fields_desc.set_align(align_of::<TyInfoTupleField>() as u64);

        // Add relocation for inner type's tydesc pointer.
        let inner_gv = module.declare_data_in_data(inner_tydesc_id, &mut fields_desc);
        fields_desc.write_data_addr(
            (TYINFO_TUPLE_FIELD_TYDESC_OFFSET) as u32,
            inner_gv,
            0,
        );

        module
            .define_data(fields_id, &fields_desc)
            .map_err(|e| CraneliftError::Module(format!("define ref fields: {}", e)))?;

        // Build the TyDesc for the ref type.
        let mut bytes = vec![0u8; TYDESC_SIZE];
        bytes[OFFSET_TYPE_TAG] = TyTag::Tuple as u8;
        bytes[OFFSET_SIZE..OFFSET_SIZE + 4].copy_from_slice(&8u32.to_le_bytes()); // Pointer size
        bytes[OFFSET_ALIGN..OFFSET_ALIGN + 4].copy_from_slice(&8u32.to_le_bytes()); // Pointer align

        // Write num_fields (1).
        let num_fields_offset = OFFSET_TYPE_INFO;
        bytes[num_fields_offset..num_fields_offset + 8].copy_from_slice(&1u64.to_le_bytes());

        let name = format!("__tydesc_ref_{}", self.counter);
        self.counter += 1;

        let data_id = module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| CraneliftError::Module(format!("declare ref tydesc: {}", e)))?;

        let mut data_desc = DataDescription::new();
        data_desc.define(bytes.into_boxed_slice());
        data_desc.set_align(TYDESC_ALIGN as u64);

        // Add relocation for fields pointer.
        let fields_gv = module.declare_data_in_data(fields_id, &mut data_desc);
        let fields_ptr_offset = OFFSET_TYPE_INFO + 8; // After num_fields
        data_desc.write_data_addr(fields_ptr_offset as u32, fields_gv, 0);

        module
            .define_data(data_id, &data_desc)
            .map_err(|e| CraneliftError::Module(format!("define ref tydesc: {}", e)))?;

        self.tydescs.insert(ref_ty.clone(), data_id);
        Ok(data_id)
    }

    /// Emit a TyDesc for a Tuple type with field type references.
    fn emit_tuple_tydesc<M: Module>(
        &mut self,
        module: &mut M,
        original_ty: &IrType,
        field_types: &[IrType],
    ) -> Result<DataId, CraneliftError> {
        // Check cache using the original type.
        if let Some(&id) = self.tydescs.get(original_ty) {
            return Ok(id);
        }

        // First, emit all field type TyDescs.
        let mut field_tydesc_ids = Vec::with_capacity(field_types.len());
        for field_ty in field_types {
            let tydesc_id = self.emit(module, field_ty)?;
            field_tydesc_ids.push(tydesc_id);
        }

        // Compute tuple layout (size, align, field offsets).
        let layout = crate::types::ir_type_to_cranelift(original_ty).layout();
        let field_offsets = crate::types::compute_tuple_field_offsets(field_types);

        // Create the fields array as a separate data object.
        let fields_data_id = if field_types.is_empty() {
            // No fields array needed for empty tuple.
            None
        } else {
            // Build the TyInfoTupleField array.
            let fields_size = TYINFO_TUPLE_FIELD_SIZE * field_types.len();
            let mut fields_bytes = vec![0u8; fields_size];

            for (i, offset) in field_offsets.iter().enumerate() {
                let field_base = i * TYINFO_TUPLE_FIELD_SIZE;
                // Write offset field.
                fields_bytes[field_base + TYINFO_TUPLE_FIELD_OFFSET_OFFSET..
                             field_base + TYINFO_TUPLE_FIELD_OFFSET_OFFSET + 4]
                    .copy_from_slice(&offset.to_le_bytes());
                // tydesc pointer will be added as relocation.
            }

            let fields_name = format!("__tydesc_tuple_fields_{}", self.counter);
            self.counter += 1;

            let fields_id = module
                .declare_data(&fields_name, Linkage::Local, false, false)
                .map_err(|e| CraneliftError::Module(format!("declare tuple fields: {}", e)))?;

            let mut fields_desc = DataDescription::new();
            fields_desc.define(fields_bytes.into_boxed_slice());
            fields_desc.set_align(align_of::<TyInfoTupleField>() as u64);

            // Add relocations for each field's tydesc pointer.
            for (i, &tydesc_id) in field_tydesc_ids.iter().enumerate() {
                let field_gv = module.declare_data_in_data(tydesc_id, &mut fields_desc);
                let tydesc_offset = (i * TYINFO_TUPLE_FIELD_SIZE + TYINFO_TUPLE_FIELD_TYDESC_OFFSET) as u32;
                fields_desc.write_data_addr(tydesc_offset, field_gv, 0);
            }

            module
                .define_data(fields_id, &fields_desc)
                .map_err(|e| CraneliftError::Module(format!("define tuple fields: {}", e)))?;

            Some(fields_id)
        };

        // Build base TyDesc bytes.
        let mut bytes = vec![0u8; TYDESC_SIZE];
        let tag = TyTag::Tuple as u8;
        let num_fields = field_types.len() as u32;

        bytes[OFFSET_TYPE_TAG] = tag;
        bytes[OFFSET_SIZE..OFFSET_SIZE + 4].copy_from_slice(&layout.size.to_le_bytes());
        bytes[OFFSET_ALIGN..OFFSET_ALIGN + 4].copy_from_slice(&layout.align.to_le_bytes());

        // Write num_fields in type_info.
        let num_fields_offset = OFFSET_TYPE_INFO + TYINFO_TUPLE_NUM_FIELDS_OFFSET;
        bytes[num_fields_offset..num_fields_offset + 4].copy_from_slice(&num_fields.to_le_bytes());

        // For empty tuple, write a non-null dangling pointer for the fields array.
        // Rust's slice::from_raw_parts requires non-null even for size 0.
        if fields_data_id.is_none() {
            let dangling: u64 = align_of::<TyInfoTupleField>() as u64;
            let fields_ptr_offset = OFFSET_TYPE_INFO + TYINFO_TUPLE_FIELDS_OFFSET;
            bytes[fields_ptr_offset..fields_ptr_offset + 8].copy_from_slice(&dangling.to_le_bytes());
        }

        // Create unique name.
        let name = format!("__tydesc_{}", self.counter);
        self.counter += 1;

        // Declare data.
        let data_id = module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| CraneliftError::Module(format!("declare tuple tydesc: {}", e)))?;

        // Define data with relocation to fields array.
        let mut data_desc = DataDescription::new();
        data_desc.define(bytes.into_boxed_slice());
        data_desc.set_align(TYDESC_ALIGN as u64);

        // Add relocation for fields pointer if we have fields.
        // Empty tuple already has dangling pointer written in bytes above.
        if let Some(fields_id) = fields_data_id {
            let fields_gv = module.declare_data_in_data(fields_id, &mut data_desc);
            let fields_ptr_offset = (OFFSET_TYPE_INFO + TYINFO_TUPLE_FIELDS_OFFSET) as u32;
            data_desc.write_data_addr(fields_ptr_offset, fields_gv, 0);
        }

        module
            .define_data(data_id, &data_desc)
            .map_err(|e| CraneliftError::Module(format!("define tuple tydesc: {}", e)))?;

        self.tydescs.insert(original_ty.clone(), data_id);
        Ok(data_id)
    }

    /// Emit a TyDesc for a Struct type with field names.
    fn emit_struct_tydesc<M: Module>(
        &mut self,
        module: &mut M,
        original_ty: &IrType,
        fields: &[(String, IrType)],
    ) -> Result<DataId, CraneliftError> {
        // Check cache using the original type.
        if let Some(&id) = self.tydescs.get(original_ty) {
            return Ok(id);
        }

        // First, emit all field type TyDescs.
        let mut field_tydesc_ids = Vec::with_capacity(fields.len());
        for (_, field_ty) in fields {
            let tydesc_id = self.emit(module, field_ty)?;
            field_tydesc_ids.push(tydesc_id);
        }

        // Compute struct layout (size, align, field offsets).
        let layout = crate::types::ir_type_to_cranelift(original_ty).layout();
        let field_types: Vec<_> = fields.iter().map(|(_, ty)| ty.clone()).collect();
        let field_offsets = crate::types::compute_tuple_field_offsets(&field_types);

        // Create the fields array as a separate data object.
        let fields_data_id = if fields.is_empty() {
            // No fields array needed for empty struct.
            None
        } else {
            // First, create static data for each field name.
            let mut name_data_ids = Vec::with_capacity(fields.len());
            for (name, _) in fields {
                let name_bytes = name.as_bytes();
                let name_name = format!("__struct_field_name_{}", self.counter);
                self.counter += 1;

                let name_id = module
                    .declare_data(&name_name, Linkage::Local, false, false)
                    .map_err(|e| CraneliftError::Module(format!("declare struct field name: {}", e)))?;

                let mut name_desc = DataDescription::new();
                name_desc.define(name_bytes.to_vec().into_boxed_slice());
                name_desc.set_align(1);

                module
                    .define_data(name_id, &name_desc)
                    .map_err(|e| CraneliftError::Module(format!("define struct field name: {}", e)))?;

                name_data_ids.push(name_id);
            }

            // Build the TyInfoStructField array.
            let fields_size = TYINFO_STRUCT_FIELD_SIZE * fields.len();
            let mut fields_bytes = vec![0u8; fields_size];

            for (i, ((name, _), &offset)) in fields.iter().zip(field_offsets.iter()).enumerate() {
                let field_base = i * TYINFO_STRUCT_FIELD_SIZE;

                // Write name_len.
                let name_len = name.len() as u32;
                fields_bytes[field_base + TYINFO_STRUCT_FIELD_NAME_LEN_OFFSET..
                             field_base + TYINFO_STRUCT_FIELD_NAME_LEN_OFFSET + 4]
                    .copy_from_slice(&name_len.to_le_bytes());

                // Write offset.
                fields_bytes[field_base + TYINFO_STRUCT_FIELD_OFFSET_OFFSET..
                             field_base + TYINFO_STRUCT_FIELD_OFFSET_OFFSET + 4]
                    .copy_from_slice(&offset.to_le_bytes());

                // name pointer and tydesc pointer will be added as relocations.
            }

            let fields_name = format!("__tydesc_struct_fields_{}", self.counter);
            self.counter += 1;

            let fields_id = module
                .declare_data(&fields_name, Linkage::Local, false, false)
                .map_err(|e| CraneliftError::Module(format!("declare struct fields: {}", e)))?;

            let mut fields_desc = DataDescription::new();
            fields_desc.define(fields_bytes.into_boxed_slice());
            fields_desc.set_align(align_of::<TyInfoStructField>() as u64);

            // Add relocations for name pointers.
            for (i, &name_id) in name_data_ids.iter().enumerate() {
                let name_gv = module.declare_data_in_data(name_id, &mut fields_desc);
                let name_offset = (i * TYINFO_STRUCT_FIELD_SIZE + TYINFO_STRUCT_FIELD_NAME_OFFSET) as u32;
                fields_desc.write_data_addr(name_offset, name_gv, 0);
            }

            // Add relocations for each field's tydesc pointer.
            for (i, &tydesc_id) in field_tydesc_ids.iter().enumerate() {
                let field_gv = module.declare_data_in_data(tydesc_id, &mut fields_desc);
                let tydesc_offset = (i * TYINFO_STRUCT_FIELD_SIZE + TYINFO_STRUCT_FIELD_TYDESC_OFFSET) as u32;
                fields_desc.write_data_addr(tydesc_offset, field_gv, 0);
            }

            module
                .define_data(fields_id, &fields_desc)
                .map_err(|e| CraneliftError::Module(format!("define struct fields: {}", e)))?;

            Some(fields_id)
        };

        // Build base TyDesc bytes.
        let mut bytes = vec![0u8; TYDESC_SIZE];
        let tag = TyTag::Struct as u8;
        let num_fields = fields.len() as u32;

        bytes[OFFSET_TYPE_TAG] = tag;
        bytes[OFFSET_SIZE..OFFSET_SIZE + 4].copy_from_slice(&layout.size.to_le_bytes());
        bytes[OFFSET_ALIGN..OFFSET_ALIGN + 4].copy_from_slice(&layout.align.to_le_bytes());

        // Write num_fields in type_info.
        let num_fields_offset = OFFSET_TYPE_INFO + TYINFO_STRUCT_NUM_FIELDS_OFFSET;
        bytes[num_fields_offset..num_fields_offset + 4].copy_from_slice(&num_fields.to_le_bytes());

        // Create unique name.
        let name = format!("__tydesc_{}", self.counter);
        self.counter += 1;

        // Declare data.
        let data_id = module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| CraneliftError::Module(format!("declare struct tydesc: {}", e)))?;

        // Define data with relocation to fields array.
        let mut data_desc = DataDescription::new();
        data_desc.define(bytes.into_boxed_slice());
        data_desc.set_align(TYDESC_ALIGN as u64);

        // Add relocation for fields pointer if we have fields.
        if let Some(fields_id) = fields_data_id {
            let fields_gv = module.declare_data_in_data(fields_id, &mut data_desc);
            let fields_ptr_offset = (OFFSET_TYPE_INFO + TYINFO_STRUCT_FIELDS_OFFSET) as u32;
            data_desc.write_data_addr(fields_ptr_offset, fields_gv, 0);
        }

        module
            .define_data(data_id, &data_desc)
            .map_err(|e| CraneliftError::Module(format!("define struct tydesc: {}", e)))?;

        self.tydescs.insert(original_ty.clone(), data_id);
        Ok(data_id)
    }

    /// Emit a TyDesc for an Enum type with variant info.
    fn emit_enum_tydesc<M: Module>(
        &mut self,
        module: &mut M,
        original_ty: &IrType,
        variants: &[(String, Option<IrType>)],
    ) -> Result<DataId, CraneliftError> {
        // Check cache using the original type.
        if let Some(&id) = self.tydescs.get(original_ty) {
            return Ok(id);
        }

        // First, emit all payload type TyDescs.
        let mut payload_tydesc_ids: Vec<Option<DataId>> = Vec::with_capacity(variants.len());
        for (_, payload_ty) in variants {
            let id = if let Some(ty) = payload_ty {
                Some(self.emit(module, ty)?)
            } else {
                None
            };
            payload_tydesc_ids.push(id);
        }

        // Compute enum layout.
        let layout = crate::types::ir_type_to_cranelift(original_ty).layout();
        let variant_offsets = crate::types::compute_enum_variant_offsets(variants);

        // Create variant name data objects and the variants array.
        let variants_data_id = if variants.is_empty() {
            None
        } else {
            // Build the TyInfoEnumVariant array.
            let variants_size = TYINFO_ENUM_VARIANT_SIZE * variants.len();
            let mut variants_bytes = vec![0u8; variants_size];

            // First, create static data for each variant name.
            let mut name_data_ids = Vec::with_capacity(variants.len());
            for (name, _) in variants {
                let name_bytes = name.as_bytes();
                let name_name = format!("__enum_variant_name_{}", self.counter);
                self.counter += 1;

                let name_id = module
                    .declare_data(&name_name, Linkage::Local, false, false)
                    .map_err(|e| CraneliftError::Module(format!("declare enum variant name: {}", e)))?;

                let mut name_desc = DataDescription::new();
                name_desc.define(name_bytes.to_vec().into_boxed_slice());
                name_desc.set_align(1);

                module
                    .define_data(name_id, &name_desc)
                    .map_err(|e| CraneliftError::Module(format!("define enum variant name: {}", e)))?;

                name_data_ids.push(name_id);
            }

            // Build variant entries.
            for (i, ((name, _), &offset)) in variants.iter().zip(variant_offsets.iter()).enumerate() {
                let variant_base = i * TYINFO_ENUM_VARIANT_SIZE;

                // Write name_len.
                let name_len = name.len() as u32;
                variants_bytes[variant_base + TYINFO_ENUM_VARIANT_NAME_LEN_OFFSET..
                               variant_base + TYINFO_ENUM_VARIANT_NAME_LEN_OFFSET + 4]
                    .copy_from_slice(&name_len.to_le_bytes());

                // Write offset.
                variants_bytes[variant_base + TYINFO_ENUM_VARIANT_OFFSET_OFFSET..
                               variant_base + TYINFO_ENUM_VARIANT_OFFSET_OFFSET + 4]
                    .copy_from_slice(&offset.to_le_bytes());

                // name pointer and payload pointer will be added as relocations.
            }

            let variants_name = format!("__tydesc_enum_variants_{}", self.counter);
            self.counter += 1;

            let variants_id = module
                .declare_data(&variants_name, Linkage::Local, false, false)
                .map_err(|e| CraneliftError::Module(format!("declare enum variants: {}", e)))?;

            let mut variants_desc = DataDescription::new();
            variants_desc.define(variants_bytes.into_boxed_slice());
            variants_desc.set_align(align_of::<TyInfoEnumVariant>() as u64);

            // Add relocations for name pointers.
            for (i, &name_id) in name_data_ids.iter().enumerate() {
                let name_gv = module.declare_data_in_data(name_id, &mut variants_desc);
                let name_offset = (i * TYINFO_ENUM_VARIANT_SIZE + TYINFO_ENUM_VARIANT_NAME_OFFSET) as u32;
                variants_desc.write_data_addr(name_offset, name_gv, 0);
            }

            // Add relocations for payload tydesc pointers.
            for (i, payload_id) in payload_tydesc_ids.iter().enumerate() {
                if let Some(tydesc_id) = payload_id {
                    let payload_gv = module.declare_data_in_data(*tydesc_id, &mut variants_desc);
                    let payload_offset = (i * TYINFO_ENUM_VARIANT_SIZE + TYINFO_ENUM_VARIANT_PAYLOAD_OFFSET) as u32;
                    variants_desc.write_data_addr(payload_offset, payload_gv, 0);
                }
                // For None payloads, the pointer stays null (zero-initialized).
            }

            module
                .define_data(variants_id, &variants_desc)
                .map_err(|e| CraneliftError::Module(format!("define enum variants: {}", e)))?;

            Some(variants_id)
        };

        // Build base TyDesc bytes.
        let mut bytes = vec![0u8; TYDESC_SIZE];
        let tag = TyTag::Enum as u8;
        let num_variants = variants.len() as u32;

        bytes[OFFSET_TYPE_TAG] = tag;
        bytes[OFFSET_SIZE..OFFSET_SIZE + 4].copy_from_slice(&layout.size.to_le_bytes());
        bytes[OFFSET_ALIGN..OFFSET_ALIGN + 4].copy_from_slice(&layout.align.to_le_bytes());

        // Write num_variants in type_info.
        let num_variants_offset = OFFSET_TYPE_INFO + TYINFO_ENUM_NUM_VARIANTS_OFFSET;
        bytes[num_variants_offset..num_variants_offset + 4].copy_from_slice(&num_variants.to_le_bytes());

        // Create unique name.
        let name = format!("__tydesc_{}", self.counter);
        self.counter += 1;

        // Declare data.
        let data_id = module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| CraneliftError::Module(format!("declare enum tydesc: {}", e)))?;

        // Define data with relocation to variants array.
        let mut data_desc = DataDescription::new();
        data_desc.define(bytes.into_boxed_slice());
        data_desc.set_align(TYDESC_ALIGN as u64);

        // Add relocation for variants pointer if we have variants.
        if let Some(variants_id) = variants_data_id {
            let variants_gv = module.declare_data_in_data(variants_id, &mut data_desc);
            let variants_ptr_offset = (OFFSET_TYPE_INFO + TYINFO_ENUM_VARIANTS_OFFSET) as u32;
            data_desc.write_data_addr(variants_ptr_offset, variants_gv, 0);
        }

        module
            .define_data(data_id, &data_desc)
            .map_err(|e| CraneliftError::Module(format!("define enum tydesc: {}", e)))?;

        self.tydescs.insert(original_ty.clone(), data_id);
        Ok(data_id)
    }

    /// Emit a TyDesc for an Atom type (zero-sized named type).
    fn emit_atom_tydesc<M: Module>(
        &mut self,
        module: &mut M,
        original_ty: &IrType,
        name: &str,
    ) -> Result<DataId, CraneliftError> {
        if let Some(&id) = self.tydescs.get(original_ty) {
            return Ok(id);
        }

        // Create static data for the atom name.
        let name_bytes = name.as_bytes();
        let name_data_name = format!("__atom_name_{}", self.counter);
        self.counter += 1;

        let name_data_id = module
            .declare_data(&name_data_name, Linkage::Local, false, false)
            .map_err(|e| CraneliftError::Module(format!("declare atom name: {}", e)))?;
        let mut name_desc = DataDescription::new();
        name_desc.define(name_bytes.to_vec().into_boxed_slice());
        name_desc.set_align(1);
        module
            .define_data(name_data_id, &name_desc)
            .map_err(|e| CraneliftError::Module(format!("define atom name: {}", e)))?;

        // Build TyDesc bytes.
        let mut bytes = vec![0u8; TYDESC_SIZE];
        bytes[OFFSET_TYPE_TAG] = TyTag::Atom as u8;
        bytes[OFFSET_SIZE..OFFSET_SIZE + 4].copy_from_slice(&0u32.to_le_bytes());
        bytes[OFFSET_ALIGN..OFFSET_ALIGN + 4].copy_from_slice(&1u32.to_le_bytes());

        // TyInfoAtom: name (*const u8) at +0, name_len (u32) at +8.
        let name_len = name.len() as u32;
        let name_len_offset = OFFSET_TYPE_INFO + 8;
        bytes[name_len_offset..name_len_offset + 4].copy_from_slice(&name_len.to_le_bytes());

        let tydesc_name = format!("__tydesc_{}", self.counter);
        self.counter += 1;

        let data_id = module
            .declare_data(&tydesc_name, Linkage::Local, false, false)
            .map_err(|e| CraneliftError::Module(format!("declare atom tydesc: {}", e)))?;

        let mut data_desc = DataDescription::new();
        data_desc.define(bytes.into_boxed_slice());
        data_desc.set_align(TYDESC_ALIGN as u64);

        // Relocation for name pointer at OFFSET_TYPE_INFO + 0.
        let name_gv = module.declare_data_in_data(name_data_id, &mut data_desc);
        data_desc.write_data_addr(OFFSET_TYPE_INFO as u32, name_gv, 0);

        module
            .define_data(data_id, &data_desc)
            .map_err(|e| CraneliftError::Module(format!("define atom tydesc: {}", e)))?;

        self.tydescs.insert(original_ty.clone(), data_id);
        Ok(data_id)
    }

    /// Emit a TyDesc for a Term type (named wrapper around payload).
    fn emit_term_tydesc<M: Module>(
        &mut self,
        module: &mut M,
        original_ty: &IrType,
        name: &str,
        payload: &IrType,
    ) -> Result<DataId, CraneliftError> {
        if let Some(&id) = self.tydescs.get(original_ty) {
            return Ok(id);
        }

        // Emit payload tydesc first.
        let payload_tydesc_id = self.emit(module, payload)?;

        // Create static data for the term name.
        let name_bytes = name.as_bytes();
        let name_data_name = format!("__term_name_{}", self.counter);
        self.counter += 1;

        let name_data_id = module
            .declare_data(&name_data_name, Linkage::Local, false, false)
            .map_err(|e| CraneliftError::Module(format!("declare term name: {}", e)))?;
        let mut name_desc = DataDescription::new();
        name_desc.define(name_bytes.to_vec().into_boxed_slice());
        name_desc.set_align(1);
        module
            .define_data(name_data_id, &name_desc)
            .map_err(|e| CraneliftError::Module(format!("define term name: {}", e)))?;

        // Build TyDesc bytes.
        let layout = crate::types::ir_type_to_cranelift(original_ty).layout();
        let mut bytes = vec![0u8; TYDESC_SIZE];
        bytes[OFFSET_TYPE_TAG] = TyTag::Term as u8;
        bytes[OFFSET_SIZE..OFFSET_SIZE + 4].copy_from_slice(&layout.size.to_le_bytes());
        bytes[OFFSET_ALIGN..OFFSET_ALIGN + 4].copy_from_slice(&layout.align.to_le_bytes());

        // TyInfoTerm: name (*const u8) at +0, name_len (u32) at +8, payload (*const TyDesc) at +16.
        let name_len = name.len() as u32;
        let name_len_offset = OFFSET_TYPE_INFO + 8;
        bytes[name_len_offset..name_len_offset + 4].copy_from_slice(&name_len.to_le_bytes());
        // payload pointer at +16 will be a relocation.

        let tydesc_name = format!("__tydesc_{}", self.counter);
        self.counter += 1;

        let data_id = module
            .declare_data(&tydesc_name, Linkage::Local, false, false)
            .map_err(|e| CraneliftError::Module(format!("declare term tydesc: {}", e)))?;

        let mut data_desc = DataDescription::new();
        data_desc.define(bytes.into_boxed_slice());
        data_desc.set_align(TYDESC_ALIGN as u64);

        // Relocation for name pointer at OFFSET_TYPE_INFO + 0.
        let name_gv = module.declare_data_in_data(name_data_id, &mut data_desc);
        data_desc.write_data_addr(OFFSET_TYPE_INFO as u32, name_gv, 0);

        // Relocation for payload tydesc pointer at OFFSET_TYPE_INFO + 16.
        let payload_gv = module.declare_data_in_data(payload_tydesc_id, &mut data_desc);
        data_desc.write_data_addr((OFFSET_TYPE_INFO + 16) as u32, payload_gv, 0);

        module
            .define_data(data_id, &data_desc)
            .map_err(|e| CraneliftError::Module(format!("define term tydesc: {}", e)))?;

        self.tydescs.insert(original_ty.clone(), data_id);
        Ok(data_id)
    }

    /// Emit a TyDesc for a Table type with column info.
    fn emit_table_tydesc<M: Module>(
        &mut self,
        module: &mut M,
        original_ty: &IrType,
        columns: &[(String, Box<IrType>)],
    ) -> Result<DataId, CraneliftError> {
        // Check cache using the original type.
        if let Some(&id) = self.tydescs.get(original_ty) {
            return Ok(id);
        }

        // First, emit all column type TyDescs.
        let mut column_tydesc_ids = Vec::with_capacity(columns.len());
        for (_, col_ty) in columns {
            let tydesc_id = self.emit(module, col_ty)?;
            column_tydesc_ids.push(tydesc_id);
        }

        // Create the columns array as a separate data object.
        let columns_data_id = if columns.is_empty() {
            None
        } else {
            // First, create static data for each column name.
            let mut name_data_ids = Vec::with_capacity(columns.len());
            for (name, _) in columns {
                let name_bytes = name.as_bytes();
                let name_name = format!("__table_column_name_{}", self.counter);
                self.counter += 1;

                let name_id = module
                    .declare_data(&name_name, Linkage::Local, false, false)
                    .map_err(|e| CraneliftError::Module(format!("declare table column name: {}", e)))?;

                let mut name_desc = DataDescription::new();
                name_desc.define(name_bytes.to_vec().into_boxed_slice());
                name_desc.set_align(1);

                module
                    .define_data(name_id, &name_desc)
                    .map_err(|e| CraneliftError::Module(format!("define table column name: {}", e)))?;

                name_data_ids.push(name_id);
            }

            // Build the TyInfoTableColumn array.
            let columns_size = TYINFO_TABLE_COLUMN_SIZE * columns.len();
            let mut columns_bytes = vec![0u8; columns_size];

            for (i, (name, _)) in columns.iter().enumerate() {
                let col_base = i * TYINFO_TABLE_COLUMN_SIZE;

                // Write name_len.
                let name_len = name.len() as u32;
                columns_bytes[col_base + TYINFO_TABLE_COLUMN_NAME_LEN_OFFSET..
                              col_base + TYINFO_TABLE_COLUMN_NAME_LEN_OFFSET + 4]
                    .copy_from_slice(&name_len.to_le_bytes());

                // name pointer and tydesc pointer will be added as relocations.
            }

            let columns_name = format!("__tydesc_table_columns_{}", self.counter);
            self.counter += 1;

            let columns_id = module
                .declare_data(&columns_name, Linkage::Local, false, false)
                .map_err(|e| CraneliftError::Module(format!("declare table columns: {}", e)))?;

            let mut columns_desc = DataDescription::new();
            columns_desc.define(columns_bytes.into_boxed_slice());
            columns_desc.set_align(align_of::<TyInfoTableColumn>() as u64);

            // Add relocations for name pointers.
            for (i, &name_id) in name_data_ids.iter().enumerate() {
                let name_gv = module.declare_data_in_data(name_id, &mut columns_desc);
                let name_offset = (i * TYINFO_TABLE_COLUMN_SIZE + TYINFO_TABLE_COLUMN_NAME_OFFSET) as u32;
                columns_desc.write_data_addr(name_offset, name_gv, 0);
            }

            // Add relocations for each column's tydesc pointer.
            for (i, &tydesc_id) in column_tydesc_ids.iter().enumerate() {
                let col_gv = module.declare_data_in_data(tydesc_id, &mut columns_desc);
                let tydesc_offset = (i * TYINFO_TABLE_COLUMN_SIZE + TYINFO_TABLE_COLUMN_TYDESC_OFFSET) as u32;
                columns_desc.write_data_addr(tydesc_offset, col_gv, 0);
            }

            module
                .define_data(columns_id, &columns_desc)
                .map_err(|e| CraneliftError::Module(format!("define table columns: {}", e)))?;

            Some(columns_id)
        };

        // Build base TyDesc bytes.
        let mut bytes = vec![0u8; TYDESC_SIZE];
        let tag = TyTag::Table as u8;
        let num_columns = columns.len() as u32;
        let size = size_of::<RtTable>() as u32;
        let align = align_of::<RtTable>() as u32;

        bytes[OFFSET_TYPE_TAG] = tag;
        bytes[OFFSET_SIZE..OFFSET_SIZE + 4].copy_from_slice(&size.to_le_bytes());
        bytes[OFFSET_ALIGN..OFFSET_ALIGN + 4].copy_from_slice(&align.to_le_bytes());

        // Write num_columns in type_info.
        let num_columns_offset = OFFSET_TYPE_INFO + TYINFO_TABLE_NUM_COLUMNS_OFFSET;
        bytes[num_columns_offset..num_columns_offset + 4].copy_from_slice(&num_columns.to_le_bytes());

        // Create unique name.
        let name = format!("__tydesc_{}", self.counter);
        self.counter += 1;

        // Declare data.
        let data_id = module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| CraneliftError::Module(format!("declare table tydesc: {}", e)))?;

        // Define data with relocation to columns array.
        let mut data_desc = DataDescription::new();
        data_desc.define(bytes.into_boxed_slice());
        data_desc.set_align(TYDESC_ALIGN as u64);

        // Add relocation for columns pointer if we have columns.
        if let Some(columns_id) = columns_data_id {
            let columns_gv = module.declare_data_in_data(columns_id, &mut data_desc);
            let columns_ptr_offset = (OFFSET_TYPE_INFO + TYINFO_TABLE_COLUMNS_OFFSET) as u32;
            data_desc.write_data_addr(columns_ptr_offset, columns_gv, 0);
        }

        module
            .define_data(data_id, &data_desc)
            .map_err(|e| CraneliftError::Module(format!("define table tydesc: {}", e)))?;

        self.tydescs.insert(original_ty.clone(), data_id);
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
    ) -> Result<(), CraneliftError> {
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
            | IrType::Index
            | IrType::Offset
            | IrType::F32
            | IrType::F64
            | IrType::Int
            | IrType::String
            | IrType::Data
            | IrType::Error => true,

            // Collection types - can emit if element types can be emitted.
            IrType::List(elem_ty) => self.can_emit(elem_ty),
            IrType::Set(elem_ty) => self.can_emit(elem_ty),
            IrType::Map(key_ty, val_ty) => self.can_emit(key_ty) && self.can_emit(val_ty),
            IrType::Tensor(elem_ty, _rank) => self.can_emit(elem_ty),

            // Option/Result types - can emit if inner type can be emitted.
            IrType::Option(inner_ty) => self.can_emit(inner_ty),
            IrType::Result(ok_ty) => self.can_emit(ok_ty),

            // Tuple/Struct types - can emit if all field types can be emitted.
            IrType::Tuple(field_types) => field_types.iter().all(|t| self.can_emit(t)),
            IrType::Struct(fields) => fields.iter().all(|(_, t)| self.can_emit(t)),

            // Enum types - can emit if all payload types can be emitted.
            IrType::Enum(variants) => variants.iter().all(|(_, payload)| {
                payload.as_ref().map_or(true, |t| self.can_emit(t))
            }),
            // Atom has no payload, always emittable.
            IrType::Atom(_) => true,
            // Term can emit if payload can be emitted.
            IrType::Term(_, payload) => self.can_emit(payload),

            // Ref types - can emit if inner type can be emitted.
            IrType::Ref(inner_ty) => self.can_emit(inner_ty),

            // Table types - can emit if all column types can be emitted.
            IrType::Table(columns) => columns.iter().all(|(_, t)| self.can_emit(t)),
        }
    }

    /// Build the raw bytes for a TyDesc.
    fn build_tydesc_bytes(&self, ty: &IrType) -> Result<Vec<u8>, CraneliftError> {
        let mut bytes = vec![0u8; TYDESC_SIZE];

        let (tag, size, align) = match ty {
            // Unit is handled by emit_tuple_tydesc, not here.
            IrType::Bool => (TyTag::Bool as u8, size_of::<bool>() as u32, align_of::<bool>() as u32),
            IrType::U8 => (TyTag::U8 as u8, size_of::<u8>() as u32, align_of::<u8>() as u32),
            IrType::I8 => (TyTag::I8 as u8, size_of::<i8>() as u32, align_of::<i8>() as u32),
            IrType::U16 => (TyTag::U16 as u8, size_of::<u16>() as u32, align_of::<u16>() as u32),
            IrType::I16 => (TyTag::I16 as u8, size_of::<i16>() as u32, align_of::<i16>() as u32),
            IrType::U32 => (TyTag::U32 as u8, size_of::<u32>() as u32, align_of::<u32>() as u32),
            IrType::I32 => (TyTag::I32 as u8, size_of::<i32>() as u32, align_of::<i32>() as u32),
            IrType::U64 => (TyTag::U64 as u8, size_of::<u64>() as u32, align_of::<u64>() as u32),
            IrType::I64 => (TyTag::I64 as u8, size_of::<i64>() as u32, align_of::<i64>() as u32),
            IrType::Index => (TyTag::Index as u8, datalove_rtdt::INDEX_SIZE, datalove_rtdt::INDEX_ALIGN),
            IrType::Offset => (TyTag::Offset as u8, datalove_rtdt::INDEX_SIZE, datalove_rtdt::INDEX_ALIGN),
            IrType::F32 => (TyTag::F32 as u8, size_of::<f32>() as u32, align_of::<f32>() as u32),
            IrType::F64 => (TyTag::F64 as u8, size_of::<f64>() as u32, align_of::<f64>() as u32),
            IrType::Int => (TyTag::Int as u8, size_of::<RtInt>() as u32, align_of::<RtInt>() as u32),
            IrType::String => (TyTag::String as u8, size_of::<RtString>() as u32, align_of::<RtString>() as u32),
            IrType::Data => (TyTag::Data as u8, size_of::<RtData>() as u32, align_of::<RtData>() as u32),
            IrType::Error => (TyTag::Error as u8, size_of::<RtError>() as u32, align_of::<RtError>() as u32),
            _ => {
                return Err(CraneliftError::Unsupported(format!(
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

/// Collect all types from a code unit for TyDesc emission.
pub fn collect_types_from_code_unit(unit: &IrCodeUnit, types: &mut HashSet<IrType>) {
    // Collect from unit's value and slot types.
    for ty in &unit.value_types {
        types.insert(ty.clone());
    }
    for ty in &unit.slot_types {
        types.insert(ty.clone());
    }

    // Collect from function context if present.
    if let Some(func_ctx) = unit.function_context() {
        for ty in &func_ctx.param_types {
            types.insert(ty.clone());
        }
    }

    // Recursively collect from nested units.
    for nested in &unit.nested_units {
        collect_types_from_code_unit(nested, types);
    }
}

/// Collect all types from a script unit for upfront TyDesc emission.
pub fn collect_types_from_script_unit(unit: &IrCodeUnit) -> HashSet<IrType> {
    let mut types = HashSet::new();
    collect_types_from_code_unit(unit, &mut types);
    types
}

/// Collect types from an iterator of code units.
///
/// Use this to collect types from module functions in a ScriptEnvironment.
pub fn collect_types_from_code_units<'a>(
    units: impl Iterator<Item = &'a IrCodeUnit>,
    types: &mut HashSet<IrType>,
) {
    for unit in units {
        collect_types_from_code_unit(unit, types);
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
