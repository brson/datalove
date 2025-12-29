//! Frame-based IR interpreter.
//!
//! Executes IR using the same runtime model as the tree-walking interpreter:
//! - Frame = flat `Vec<u8>` byte buffer with computed offsets
//! - Values = `(ptr, tydesc)` pairs pointing into frame memory
//! - Operations via datalove-rt runtime calls

use rmx::prelude::*;
use rmx::std::collections::HashMap;
use datalove_rt::rtdt::{self, TyDesc};

use super::{
    IrFunction, IrScriptUnit, IrBlock, IrType, Instruction, Terminator,
    ValueId, SlotId, BlockId, FuncId, FuncRef, Operand, SlotDest,
    ConstValue, BinOp, UnaryOp,
};

/// Align a value up to the given alignment.
#[inline]
fn align_up(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}

/// Interpreter error.
#[derive(Debug)]
pub enum InterpError {
    /// Type information missing for value.
    MissingType(ValueId),
    /// Type information missing for slot.
    MissingSlotType(SlotId),
    /// Value not initialized.
    UninitializedValue(ValueId),
    /// Slot not initialized.
    UninitializedSlot(SlotId),
    /// Block not found.
    BlockNotFound(BlockId),
    /// Function not found.
    FunctionNotFound(FuncId),
    /// Arithmetic overflow.
    Overflow,
    /// Division by zero.
    DivisionByZero,
    /// Runtime error.
    RuntimeError(String),
    /// Type mismatch.
    TypeMismatch(String),
    /// External unit not found.
    ExternalUnitNotFound(u32),
    /// Module function not found.
    ModuleFunctionNotFound(String),
}

/// Readable value pointer with type descriptor.
#[derive(Copy, Clone, Debug)]
pub struct Value {
    pub ptr: *mut u8,
    pub tydesc: *const TyDesc,
}

/// Write destination with type descriptor.
#[derive(Copy, Clone, Debug)]
pub struct Destination {
    pub ptr: *mut u8,
    pub tydesc: *const TyDesc,
}

impl Destination {
    pub fn to_value(self) -> Value {
        Value { ptr: self.ptr, tydesc: self.tydesc }
    }
}

/// Layout information for a function/unit frame.
///
/// Maps ValueId/SlotId to byte offsets within the frame.
pub struct IrLayout {
    /// Offset for each ValueId.
    pub value_offsets: Vec<u32>,
    /// TyDesc for each ValueId.
    pub value_tydescs: Vec<*const TyDesc>,
    /// Offset for each SlotId.
    pub slot_offsets: Vec<u32>,
    /// TyDesc for each SlotId.
    pub slot_tydescs: Vec<*const TyDesc>,
    /// Total frame size.
    pub frame_size: u32,
    /// Frame alignment.
    pub frame_align: u32,
}

impl IrLayout {
    /// Compute layout from IrType arrays.
    pub fn compute(
        value_types: &[IrType],
        slot_types: &[IrType],
        tydesc_table: &mut IrTyDescTable,
    ) -> Self {
        let mut value_offsets = Vec::with_capacity(value_types.len());
        let mut value_tydescs = Vec::with_capacity(value_types.len());
        let mut slot_offsets = Vec::with_capacity(slot_types.len());
        let mut slot_tydescs = Vec::with_capacity(slot_types.len());

        let mut offset: u32 = 0;
        let mut max_align: u32 = 1;

        // Layout values first.
        for ty in value_types {
            let tydesc = tydesc_table.get_or_create(ty);
            let size = unsafe { (*tydesc).size };
            let align = unsafe { (*tydesc).align };

            offset = align_up(offset, align);
            value_offsets.push(offset);
            value_tydescs.push(tydesc);
            offset += size;
            max_align = max_align.max(align);
        }

        // Then slots.
        for ty in slot_types {
            let tydesc = tydesc_table.get_or_create(ty);
            let size = unsafe { (*tydesc).size };
            let align = unsafe { (*tydesc).align };

            offset = align_up(offset, align);
            slot_offsets.push(offset);
            slot_tydescs.push(tydesc);
            offset += size;
            max_align = max_align.max(align);
        }

        // Final alignment for frame size.
        let frame_size = align_up(offset, max_align);

        Self {
            value_offsets,
            value_tydescs,
            slot_offsets,
            slot_tydescs,
            frame_size,
            frame_align: max_align,
        }
    }
}

/// TyDesc table for IR types.
///
/// Converts IrType to runtime TyDesc pointers.
pub struct IrTyDescTable {
    /// Storage for TyDesc allocations.
    tydescs: Vec<Box<TyDesc>>,
    /// Storage for tuple field arrays.
    tuple_fields: Vec<Vec<rtdt::TyInfoTupleField>>,
    /// Storage for struct field arrays.
    struct_fields: Vec<Vec<rtdt::TyInfoStructField>>,
    /// Storage for field name strings (to keep them alive).
    field_names: Vec<String>,
}

impl IrTyDescTable {
    pub fn new() -> Self {
        Self {
            tydescs: Vec::new(),
            tuple_fields: Vec::new(),
            struct_fields: Vec::new(),
            field_names: Vec::new(),
        }
    }

    /// Get or create a TyDesc for the given IrType.
    pub fn get_or_create(&mut self, ty: &IrType) -> *const TyDesc {
        let tydesc = self.create_tydesc(ty);
        self.tydescs.push(tydesc);
        &**self.tydescs.last().unwrap() as *const TyDesc
    }

    fn create_tydesc(&mut self, ty: &IrType) -> Box<TyDesc> {
        match ty {
            IrType::Unit => Box::new(TyDesc {
                type_tag: rtdt::TyTag::Tuple,
                size: 0,
                align: 1,
                type_info: rtdt::TyInfo {
                    tuple: rtdt::TyInfoTuple {
                        num_fields: 0,
                        fields: std::ptr::null(),
                    },
                },
            }),
            IrType::Bool => Box::new(TyDesc {
                type_tag: rtdt::TyTag::Bool,
                size: 1,
                align: 1,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::U8 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::U8,
                size: 1,
                align: 1,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::U16 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::U16,
                size: 2,
                align: 2,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::U32 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::U32,
                size: 4,
                align: 4,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::U64 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::U64,
                size: 8,
                align: 8,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::I8 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::I8,
                size: 1,
                align: 1,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::I16 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::I16,
                size: 2,
                align: 2,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::I32 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::I32,
                size: 4,
                align: 4,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::I64 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::I64,
                size: 8,
                align: 8,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::Int => Box::new(TyDesc {
                type_tag: rtdt::TyTag::Int,
                size: std::mem::size_of::<rtdt::Int>() as u32,
                align: std::mem::align_of::<rtdt::Int>() as u32,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::F32 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::F32,
                size: 4,
                align: 4,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::String => Box::new(TyDesc {
                type_tag: rtdt::TyTag::String,
                size: std::mem::size_of::<rtdt::String>() as u32,
                align: std::mem::align_of::<rtdt::String>() as u32,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::Data => Box::new(TyDesc {
                type_tag: rtdt::TyTag::Data,
                size: std::mem::size_of::<rtdt::Data>() as u32,
                align: std::mem::align_of::<rtdt::Data>() as u32,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::Error => Box::new(TyDesc {
                type_tag: rtdt::TyTag::Error,
                size: std::mem::size_of::<rtdt::Error>() as u32,
                align: std::mem::align_of::<rtdt::Error>() as u32,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::Tuple(fields) => self.create_tuple_tydesc(fields),
            IrType::Struct(fields) => self.create_struct_tydesc(fields),
            IrType::List(elem) => self.create_list_tydesc(elem),
            IrType::Set(elem) => self.create_set_tydesc(elem),
            IrType::Map(key, val) => self.create_map_tydesc(key, val),
            IrType::Option(inner) => self.create_option_tydesc(inner),
            IrType::Result(inner) => self.create_result_tydesc(inner),
        }
    }

    fn create_tuple_tydesc(&mut self, fields: &[IrType]) -> Box<TyDesc> {
        let field_tydescs: Vec<_> = fields.iter()
            .map(|f| self.get_or_create(f))
            .collect();

        // Create field info with placeholder offsets.
        let mut field_info: Vec<_> = field_tydescs.iter()
            .map(|&tydesc| rtdt::TyInfoTupleField { offset: 0, tydesc })
            .collect();

        // Compute layout.
        let layout = unsafe {
            let temp_tydesc = TyDesc {
                type_tag: rtdt::TyTag::Tuple,
                size: 0,
                align: 1,
                type_info: rtdt::TyInfo {
                    tuple: rtdt::TyInfoTuple {
                        num_fields: fields.len() as u32,
                        fields: field_info.as_ptr(),
                    },
                },
            };
            rtdt::layout::compute_tuple_layout(rtdt::TyDescRef::from_ptr(&temp_tydesc))
        };

        // Update offsets.
        for (i, fi) in field_info.iter_mut().enumerate() {
            fi.offset = layout.field_offsets[i];
        }

        self.tuple_fields.push(field_info);
        let fields_ptr = self.tuple_fields.last().unwrap().as_ptr();

        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Tuple,
            size: layout.size,
            align: layout.align,
            type_info: rtdt::TyInfo {
                tuple: rtdt::TyInfoTuple {
                    num_fields: fields.len() as u32,
                    fields: fields_ptr,
                },
            },
        })
    }

    fn create_struct_tydesc(&mut self, fields: &[(String, IrType)]) -> Box<TyDesc> {
        let field_tydescs: Vec<_> = fields.iter()
            .map(|(_, ty)| self.get_or_create(ty))
            .collect();

        // Store field names.
        let name_start = self.field_names.len();
        for (name, _) in fields {
            self.field_names.push(name.clone());
        }

        // Create field info with placeholder offsets.
        let mut field_info: Vec<_> = fields.iter().enumerate()
            .map(|(i, _)| {
                let name_ref = &self.field_names[name_start + i];
                rtdt::TyInfoStructField {
                    name: name_ref.as_ptr(),
                    name_len: name_ref.len() as u32,
                    offset: 0,
                    tydesc: field_tydescs[i],
                }
            })
            .collect();

        // Compute layout.
        let layout = unsafe {
            let temp_tydesc = TyDesc {
                type_tag: rtdt::TyTag::Struct,
                size: 0,
                align: 1,
                type_info: rtdt::TyInfo {
                    struct_: rtdt::TyInfoStruct {
                        num_fields: fields.len() as u32,
                        fields: field_info.as_ptr(),
                    },
                },
            };
            rtdt::layout::compute_struct_layout(rtdt::TyDescRef::from_ptr(&temp_tydesc))
        };

        // Update offsets.
        for (i, fi) in field_info.iter_mut().enumerate() {
            fi.offset = layout.field_offsets[i];
        }

        self.struct_fields.push(field_info);
        let fields_ptr = self.struct_fields.last().unwrap().as_ptr();

        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Struct,
            size: layout.size,
            align: layout.align,
            type_info: rtdt::TyInfo {
                struct_: rtdt::TyInfoStruct {
                    num_fields: fields.len() as u32,
                    fields: fields_ptr,
                },
            },
        })
    }

    fn create_list_tydesc(&mut self, elem: &IrType) -> Box<TyDesc> {
        let element_tydesc = self.get_or_create(elem);
        Box::new(TyDesc {
            type_tag: rtdt::TyTag::List,
            size: std::mem::size_of::<rtdt::List>() as u32,
            align: std::mem::align_of::<rtdt::List>() as u32,
            type_info: rtdt::TyInfo {
                list: rtdt::TyInfoList { element_tydesc },
            },
        })
    }

    fn create_set_tydesc(&mut self, elem: &IrType) -> Box<TyDesc> {
        let element_tydesc = self.get_or_create(elem);
        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Set,
            size: std::mem::size_of::<rtdt::Set>() as u32,
            align: std::mem::align_of::<rtdt::Set>() as u32,
            type_info: rtdt::TyInfo {
                set: rtdt::TyInfoSet { element_tydesc },
            },
        })
    }

    fn create_map_tydesc(&mut self, key: &IrType, val: &IrType) -> Box<TyDesc> {
        let key_tydesc = self.get_or_create(key);
        let value_tydesc = self.get_or_create(val);
        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Map,
            size: std::mem::size_of::<rtdt::Map>() as u32,
            align: std::mem::align_of::<rtdt::Map>() as u32,
            type_info: rtdt::TyInfo {
                map: rtdt::TyInfoMap { key_tydesc, value_tydesc },
            },
        })
    }

    fn create_option_tydesc(&mut self, inner: &IrType) -> Box<TyDesc> {
        let inner_tydesc = self.get_or_create(inner);

        let temp_tydesc = TyDesc {
            type_tag: rtdt::TyTag::Option,
            size: 0,
            align: 1,
            type_info: rtdt::TyInfo {
                option: rtdt::TyInfoOption { inner_tydesc },
            },
        };

        let layout = unsafe {
            rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(&temp_tydesc))
        };

        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Option,
            size: layout.size,
            align: layout.align,
            type_info: rtdt::TyInfo {
                option: rtdt::TyInfoOption { inner_tydesc },
            },
        })
    }

    fn create_result_tydesc(&mut self, inner: &IrType) -> Box<TyDesc> {
        let ok_tydesc = self.get_or_create(inner);

        let temp_tydesc = TyDesc {
            type_tag: rtdt::TyTag::Result,
            size: 0,
            align: 1,
            type_info: rtdt::TyInfo {
                result: rtdt::TyInfoResult { ok_tydesc },
            },
        };

        let layout = unsafe {
            rtdt::layout::compute_result_layout(rtdt::TyDescRef::from_ptr(&temp_tydesc))
        };

        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Result,
            size: layout.size,
            align: layout.align,
            type_info: rtdt::TyInfo {
                result: rtdt::TyInfoResult { ok_tydesc },
            },
        })
    }
}

/// Execution frame for a function call.
pub struct Frame {
    /// Raw frame data.
    data: Vec<u8>,
    /// Layout information.
    layout: IrLayout,
    /// Track which values are initialized.
    value_initialized: Vec<bool>,
    /// Track which slots are initialized.
    slot_initialized: Vec<bool>,
}

impl Frame {
    /// Create a new frame from layout.
    pub fn new(layout: IrLayout) -> Self {
        let value_count = layout.value_offsets.len();
        let slot_count = layout.slot_offsets.len();
        let data = vec![0u8; layout.frame_size as usize];

        Self {
            data,
            layout,
            value_initialized: vec![false; value_count],
            slot_initialized: vec![false; slot_count],
        }
    }

    /// Get destination for a value.
    pub fn value_dest(&mut self, id: ValueId) -> Result<Destination, InterpError> {
        let idx = id.0 as usize;
        if idx >= self.layout.value_offsets.len() {
            return Err(InterpError::MissingType(id));
        }
        let offset = self.layout.value_offsets[idx] as usize;
        let tydesc = self.layout.value_tydescs[idx];
        let ptr = unsafe { self.data.as_mut_ptr().add(offset) };
        Ok(Destination { ptr, tydesc })
    }

    /// Get value (for reading).
    pub fn value(&self, id: ValueId) -> Result<Value, InterpError> {
        let idx = id.0 as usize;
        if idx >= self.layout.value_offsets.len() {
            return Err(InterpError::MissingType(id));
        }
        if !self.value_initialized[idx] {
            return Err(InterpError::UninitializedValue(id));
        }
        let offset = self.layout.value_offsets[idx] as usize;
        let tydesc = self.layout.value_tydescs[idx];
        let ptr = unsafe { (self.data.as_ptr() as *mut u8).add(offset) };
        Ok(Value { ptr, tydesc })
    }

    /// Mark value as initialized.
    pub fn mark_value_initialized(&mut self, id: ValueId) {
        let idx = id.0 as usize;
        if idx < self.value_initialized.len() {
            self.value_initialized[idx] = true;
        }
    }

    /// Get destination for a slot.
    pub fn slot_dest(&mut self, id: SlotId) -> Result<Destination, InterpError> {
        let idx = id.0 as usize;
        if idx >= self.layout.slot_offsets.len() {
            return Err(InterpError::MissingSlotType(id));
        }
        let offset = self.layout.slot_offsets[idx] as usize;
        let tydesc = self.layout.slot_tydescs[idx];
        let ptr = unsafe { self.data.as_mut_ptr().add(offset) };
        Ok(Destination { ptr, tydesc })
    }

    /// Get slot value (for reading).
    pub fn slot(&self, id: SlotId) -> Result<Value, InterpError> {
        let idx = id.0 as usize;
        if idx >= self.layout.slot_offsets.len() {
            return Err(InterpError::MissingSlotType(id));
        }
        if !self.slot_initialized[idx] {
            return Err(InterpError::UninitializedSlot(id));
        }
        let offset = self.layout.slot_offsets[idx] as usize;
        let tydesc = self.layout.slot_tydescs[idx];
        let ptr = unsafe { (self.data.as_ptr() as *mut u8).add(offset) };
        Ok(Value { ptr, tydesc })
    }

    /// Mark slot as initialized.
    pub fn mark_slot_initialized(&mut self, id: SlotId) {
        let idx = id.0 as usize;
        if idx < self.slot_initialized.len() {
            self.slot_initialized[idx] = true;
        }
    }
}

/// Environment for sequential script unit execution.
/// Registry of functions from modules and previous script units.
///
/// Immutable after setup - can be borrowed while frames are mutated.
pub struct FunctionRegistry {
    /// Functions from each unit, indexed by unit number.
    unit_functions: Vec<Vec<IrFunction>>,
    /// Functions from modules, indexed by name.
    module_functions: HashMap<String, IrFunction>,
}

impl FunctionRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            unit_functions: Vec::new(),
            module_functions: HashMap::new(),
        }
    }

    /// Add a module function.
    pub fn add_module_function(&mut self, name: String, func: IrFunction) {
        self.module_functions.insert(name, func);
    }

    /// Add functions from a completed unit.
    pub fn add_unit_functions(&mut self, functions: Vec<IrFunction>) {
        self.unit_functions.push(functions);
    }

    /// Get a module function by name.
    pub fn get_module_function(&self, name: &str) -> Option<&IrFunction> {
        self.module_functions.get(name)
    }

    /// Look up a function from a previous unit.
    pub fn external_function(&self, unit: u32, func_id: FuncId) -> Result<&IrFunction, InterpError> {
        let functions = self.unit_functions.get(unit as usize)
            .ok_or(InterpError::ExternalUnitNotFound(unit))?;
        functions.iter()
            .find(|f| f.id == func_id)
            .ok_or(InterpError::FunctionNotFound(func_id))
    }
}

impl Default for FunctionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Mutable frame storage for script unit execution.
///
/// Stores frames from previously executed units for external value/slot access.
pub struct FrameStore {
    /// Frames from executed units, indexed by unit number.
    frames: Vec<Frame>,
}

impl FrameStore {
    /// Create a new empty frame store.
    pub fn new() -> Self {
        Self { frames: Vec::new() }
    }

    /// Add a completed unit's frame.
    pub fn add_frame(&mut self, frame: Frame) {
        self.frames.push(frame);
    }

    /// Read a value from a previous unit.
    pub fn external_value(&self, unit: u32, value: ValueId) -> Result<Value, InterpError> {
        let frame = self.frames.get(unit as usize)
            .ok_or(InterpError::ExternalUnitNotFound(unit))?;
        frame.value(value)
    }

    /// Read a slot from a previous unit.
    pub fn external_slot(&self, unit: u32, slot: SlotId) -> Result<Value, InterpError> {
        let frame = self.frames.get(unit as usize)
            .ok_or(InterpError::ExternalUnitNotFound(unit))?;
        frame.slot(slot)
    }

    /// Write a value to a slot in a previous unit.
    pub fn write_external_slot(&mut self, unit: u32, slot: SlotId, value: &Value) -> Result<(), InterpError> {
        let frame = self.frames.get_mut(unit as usize)
            .ok_or(InterpError::ExternalUnitNotFound(unit))?;
        let dest = frame.slot_dest(slot)?;
        unsafe {
            std::ptr::copy_nonoverlapping(value.ptr, dest.ptr, (*value.tydesc).size as usize);
        }
        Ok(())
    }
}

impl Default for FrameStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Combined environment for script execution (convenience wrapper).
pub struct ScriptEnvironment {
    pub registry: FunctionRegistry,
    pub frames: FrameStore,
}

impl ScriptEnvironment {
    pub fn new() -> Self {
        Self {
            registry: FunctionRegistry::new(),
            frames: FrameStore::new(),
        }
    }

    /// Add a module function.
    pub fn add_module_function(&mut self, name: String, func: IrFunction) {
        self.registry.add_module_function(name, func);
    }

    /// Add a completed unit's frame and functions.
    pub fn add_unit(&mut self, frame: Frame, functions: Vec<IrFunction>) {
        self.frames.add_frame(frame);
        self.registry.add_unit_functions(functions);
    }
}

impl Default for ScriptEnvironment {
    fn default() -> Self {
        Self::new()
    }
}

/// Execution context holding available functions.
pub struct ExecutionContext<'a> {
    /// Local functions available for calling (from current unit).
    functions: &'a [IrFunction],
}

impl<'a> ExecutionContext<'a> {
    /// Create a new execution context with the given functions.
    pub fn new(functions: &'a [IrFunction]) -> Self {
        Self { functions }
    }

    /// Look up a function by reference.
    fn get_function<'b>(
        &'b self,
        func_ref: &FuncRef,
        registry: &'b FunctionRegistry,
    ) -> Result<&'b IrFunction, InterpError> {
        match func_ref {
            FuncRef::Local(id) => {
                self.functions.iter()
                    .find(|f| f.id == *id)
                    .ok_or(InterpError::FunctionNotFound(*id))
            }
            FuncRef::External { unit, func } => {
                registry.external_function(*unit, *func)
            }
            FuncRef::Module { name } => {
                registry.get_module_function(name)
                    .ok_or(InterpError::ModuleFunctionNotFound(name.clone()))
            }
        }
    }
}

/// IR function interpreter.
pub struct IrInterpreter {
    runtime: datalove_rt::rust::Runtime,
    tydesc_table: IrTyDescTable,
}

impl IrInterpreter {
    pub fn new() -> Self {
        Self {
            runtime: datalove_rt::rust::Runtime::new(),
            tydesc_table: IrTyDescTable::new(),
        }
    }

    /// Execute a function with arguments, writing result to destination.
    pub fn call(
        &mut self,
        func: &IrFunction,
        args: Vec<Value>,
        ret_dest: Destination,
    ) -> Result<(), InterpError> {
        // For single function execution, create a context with just this function.
        let functions = [func.clone()];
        let ctx = ExecutionContext::new(&functions);
        let registry = FunctionRegistry::new();
        let mut frames = FrameStore::new();
        self.call_in_context(func, args, ret_dest, &ctx, &registry, &mut frames)
    }

    /// Execute a function with arguments in a context with available functions.
    pub fn call_in_context(
        &mut self,
        func: &IrFunction,
        args: Vec<Value>,
        ret_dest: Destination,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        // Compute layout.
        let layout = IrLayout::compute(
            &func.value_types,
            &func.slot_types,
            &mut self.tydesc_table,
        );

        // Create frame.
        let mut frame = Frame::new(layout);

        // Copy arguments into parameter slots.
        for (i, &param_id) in func.params.iter().enumerate() {
            if i < args.len() {
                let dest = frame.value_dest(param_id)?;
                let src = &args[i];
                unsafe {
                    self.copy_value(src, dest)?;
                }
                frame.mark_value_initialized(param_id);
            }
        }

        // Execute blocks, writing return value directly to ret_dest.
        self.execute_blocks(&func.blocks, &mut frame, ret_dest, ctx, registry, frames)
    }

    /// Execute a script unit, optionally returning the result value.
    ///
    /// For expression units, the result is written to ret_dest.
    /// For fragment units, nothing is written.
    ///
    /// Use this for standalone script units that don't reference previous units.
    pub fn execute_script_unit(
        &mut self,
        unit: &IrScriptUnit,
        ret_dest: Destination,
    ) -> Result<(), InterpError> {
        // Compute layout.
        let layout = IrLayout::compute(
            &unit.value_types,
            &unit.slot_types,
            &mut self.tydesc_table,
        );

        // Create frame.
        let mut frame = Frame::new(layout);

        // Create execution context with functions defined in this unit.
        let ctx = ExecutionContext::new(&unit.functions);

        // Execute blocks with empty registry/frames (no external references).
        let registry = FunctionRegistry::new();
        let mut frames = FrameStore::new();
        self.execute_blocks(&unit.blocks, &mut frame, ret_dest, &ctx, &registry, &mut frames)
    }

    /// Execute a script unit with access to previous units' values.
    ///
    /// After execution, the unit's frame and functions are added to the environment
    /// for subsequent units to reference.
    pub fn execute_script_unit_in_env(
        &mut self,
        unit: &IrScriptUnit,
        env: &mut ScriptEnvironment,
        ret_dest: Destination,
    ) -> Result<(), InterpError> {
        // Compute layout.
        let layout = IrLayout::compute(
            &unit.value_types,
            &unit.slot_types,
            &mut self.tydesc_table,
        );

        // Create frame.
        let mut frame = Frame::new(layout);

        // Create execution context with local functions.
        let ctx = ExecutionContext::new(&unit.functions);

        // Execute blocks with registry for function lookups and frames for slot access.
        self.execute_blocks(
            &unit.blocks,
            &mut frame,
            ret_dest,
            &ctx,
            &env.registry,
            &mut env.frames,
        )?;

        // Add this unit's frame and functions to the environment for future units.
        env.add_unit(frame, unit.functions.clone());

        Ok(())
    }

    fn execute_blocks(
        &mut self,
        blocks: &[IrBlock],
        frame: &mut Frame,
        ret_dest: Destination,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        let mut current_block = BlockId(0);

        loop {
            let block = blocks.iter()
                .find(|b| b.id == current_block)
                .ok_or(InterpError::BlockNotFound(current_block))?;

            // Execute instructions.
            for instr in &block.instructions {
                self.execute_instruction(instr, frame, ret_dest, ctx, registry, frames)?;
            }

            // Handle terminator.
            match &block.terminator {
                Terminator::Goto(target) => {
                    current_block = *target;
                }
                Terminator::Branch { cond, then_block, else_block } => {
                    let cond_val = self.read_operand(cond, frame, frames)?;
                    let cond_bool = unsafe { *(cond_val.ptr as *const bool) };
                    current_block = if cond_bool { *then_block } else { *else_block };
                }
                Terminator::Return { value } => {
                    if let Some(op) = value {
                        let val = self.read_operand(op, frame, frames)?;
                        unsafe { self.copy_value(&val, ret_dest)?; }
                    }
                    return Ok(());
                }
                Terminator::TryReturn { value } => {
                    if let Some(op) = value {
                        let val = self.read_operand(op, frame, frames)?;
                        unsafe { self.copy_value(&val, ret_dest)?; }
                    }
                    return Ok(());
                }
                Terminator::UnitEnd { result } => {
                    if let Some(op) = result {
                        let val = self.read_operand(op, frame, frames)?;
                        unsafe { self.copy_value(&val, ret_dest)?; }
                    }
                    return Ok(());
                }
                Terminator::UnitEarlyReturn { value } => {
                    let val = self.read_operand(value, frame, frames)?;
                    unsafe { self.copy_value(&val, ret_dest)?; }
                    return Ok(());
                }
            }
        }
    }

    fn execute_instruction(
        &mut self,
        instr: &Instruction,
        frame: &mut Frame,
        _ret_dest: Destination,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        match instr {
            Instruction::Const { dest, value } => {
                let dest_slot = frame.value_dest(*dest)?;
                self.write_const(value, dest_slot)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::Copy { dest, src } => {
                let src_val = self.read_operand(src, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                unsafe { self.copy_value(&src_val, dest_slot)?; }
                frame.mark_value_initialized(*dest);
            }
            Instruction::Move { dest, src } => {
                let src_val = self.read_operand(src, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                unsafe { self.move_value(&src_val, dest_slot)?; }
                frame.mark_value_initialized(*dest);
            }
            Instruction::BinOp { dest, op, lhs, rhs } => {
                let lhs_val = self.read_operand(lhs, frame, frames)?;
                let rhs_val = self.read_operand(rhs, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_binop(*op, &lhs_val, &rhs_val, dest_slot)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::UnaryOp { dest, op, operand } => {
                let src_val = self.read_operand(operand, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_unaryop(*op, &src_val, dest_slot)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::SlotStore { dest, value } => {
                let src_val = self.read_operand(value, frame, frames)?;
                match dest {
                    SlotDest::Local(slot_id) => {
                        let dest_slot = frame.slot_dest(*slot_id)?;
                        unsafe { self.copy_value(&src_val, dest_slot)?; }
                        frame.mark_slot_initialized(*slot_id);
                    }
                    SlotDest::External { unit, slot } => {
                        frames.write_external_slot(*unit, *slot, &src_val)?;
                    }
                }
            }
            Instruction::SlotLoad { dest, slot } => {
                let slot_val = frame.slot(*slot)?;
                let dest_slot = frame.value_dest(*dest)?;
                unsafe { self.copy_value(&slot_val, dest_slot)?; }
                frame.mark_value_initialized(*dest);
            }
            Instruction::Pack { dest, ty: _, fields } => {
                let field_vals: Vec<Value> = fields.iter()
                    .map(|op| self.read_operand(op, frame, frames))
                    .collect::<Result<_, _>>()?;
                let dest_slot = frame.value_dest(*dest)?;
                // Check type tag to determine if tuple or struct.
                let tag = unsafe { (*dest_slot.tydesc).type_tag };
                match tag {
                    rtdt::TyTag::Tuple => self.execute_pack_tuple(&field_vals, dest_slot)?,
                    rtdt::TyTag::Struct => self.execute_pack_struct(&field_vals, dest_slot)?,
                    _ => return Err(InterpError::TypeMismatch(
                        format!("Pack requires tuple or struct type, got {:?}", tag)
                    )),
                }
                frame.mark_value_initialized(*dest);
            }
            Instruction::Unpack { dests, src } => {
                let src_val = self.read_operand(src, frame, frames)?;
                let tag = unsafe { (*src_val.tydesc).type_tag };
                match tag {
                    rtdt::TyTag::Tuple => {
                        let tuple_info = unsafe { (*src_val.tydesc).type_info.tuple };
                        for (i, &dest_id) in dests.iter().enumerate() {
                            let dest_slot = frame.value_dest(dest_id)?;
                            let field_info = unsafe { &*tuple_info.fields.add(i) };
                            let field_ptr = unsafe { src_val.ptr.add(field_info.offset as usize) };
                            let size = unsafe { (*field_info.tydesc).size as usize };
                            unsafe { std::ptr::copy_nonoverlapping(field_ptr, dest_slot.ptr, size); }
                            frame.mark_value_initialized(dest_id);
                        }
                    }
                    rtdt::TyTag::Struct => {
                        let struct_info = unsafe { (*src_val.tydesc).type_info.struct_ };
                        for (i, &dest_id) in dests.iter().enumerate() {
                            let dest_slot = frame.value_dest(dest_id)?;
                            let field_info = unsafe { &*struct_info.fields.add(i) };
                            let field_ptr = unsafe { src_val.ptr.add(field_info.offset as usize) };
                            let size = unsafe { (*field_info.tydesc).size as usize };
                            unsafe { std::ptr::copy_nonoverlapping(field_ptr, dest_slot.ptr, size); }
                            frame.mark_value_initialized(dest_id);
                        }
                    }
                    _ => return Err(InterpError::TypeMismatch(
                        format!("Unpack requires tuple or struct type, got {:?}", tag)
                    )),
                }
            }
            Instruction::TupleIndex { dest, base, index } => {
                let base_val = self.read_operand(base, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_tuple_index(&base_val, *index, dest_slot)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::FieldAccess { dest, base, field_index } => {
                let base_val = self.read_operand(base, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_field_access(&base_val, *field_index, dest_slot)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::BinOpChecked { dest, overflow, op, lhs, rhs } => {
                // Execute checked arithmetic and set overflow flag.
                let lhs_val = self.read_operand(lhs, frame, frames)?;
                let rhs_val = self.read_operand(rhs, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                let overflow_slot = frame.value_dest(*overflow)?;
                self.execute_binop_checked(*op, &lhs_val, &rhs_val, dest_slot, overflow_slot)?;
                frame.mark_value_initialized(*dest);
                frame.mark_value_initialized(*overflow);
            }
            Instruction::WrapSome { dest, inner } => {
                let inner_val = self.read_operand(inner, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_wrap_some(&inner_val, dest_slot)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::WrapNone { dest } => {
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_wrap_none(dest_slot)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::UnwrapOption { dest, is_some, src } => {
                let src_val = self.read_operand(src, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                let is_some_slot = frame.value_dest(*is_some)?;
                self.execute_unwrap_option(&src_val, dest_slot, is_some_slot)?;
                frame.mark_value_initialized(*dest);
                frame.mark_value_initialized(*is_some);
            }
            Instruction::WrapOk { dest, inner } => {
                let inner_val = self.read_operand(inner, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_wrap_ok(&inner_val, dest_slot)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::WrapErr { dest, inner } => {
                let inner_val = self.read_operand(inner, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_wrap_err(&inner_val, dest_slot)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::UnwrapResult { dest, is_ok, src } => {
                let src_val = self.read_operand(src, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                let is_ok_slot = frame.value_dest(*is_ok)?;
                self.execute_unwrap_result(&src_val, dest_slot, is_ok_slot)?;
                frame.mark_value_initialized(*dest);
                frame.mark_value_initialized(*is_ok);
            }
            Instruction::Call { dest, func, args } => {
                // Look up the function.
                let callee = ctx.get_function(func, registry)?;

                // Evaluate arguments.
                let arg_vals: Vec<Value> = args.iter()
                    .map(|op| self.read_operand(op, frame, frames))
                    .collect::<Result<_, _>>()?;

                // Get destination for return value.
                let dest_slot = frame.value_dest(*dest)?;

                // Call the function, writing result directly to destination.
                self.call_in_context(callee, arg_vals, dest_slot, ctx, registry, frames)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::ListNew { .. } => {
                todo!("ListNew instruction not yet implemented")
            }
            Instruction::SetNew { .. } => {
                todo!("SetNew instruction not yet implemented")
            }
            Instruction::MapNew { .. } => {
                todo!("MapNew instruction not yet implemented")
            }
            Instruction::Phi { .. } => {
                // Phi nodes are handled by the terminator jump logic.
                // This should not be reached during normal execution.
                todo!("Phi instruction should be handled by CFG traversal")
            }
            Instruction::Drop { .. } => {
                // TODO: Run destructor for value.
            }
            Instruction::Nop => {}
        }
        Ok(())
    }

    fn read_operand(
        &self,
        op: &Operand,
        frame: &Frame,
        frames: &FrameStore,
    ) -> Result<Value, InterpError> {
        match op {
            Operand::Value(id) => frame.value(*id),
            Operand::Slot(id) => frame.slot(*id),
            Operand::ExternalValue { unit, value } => {
                frames.external_value(*unit, *value)
            }
            Operand::ExternalSlot { unit, slot } => {
                frames.external_slot(*unit, *slot)
            }
        }
    }

    fn write_const(&self, value: &ConstValue, dest: Destination) -> Result<(), InterpError> {
        unsafe {
            match value {
                ConstValue::Unit => {
                    // Unit is zero-sized, nothing to write.
                }
                ConstValue::Bool(b) => {
                    *(dest.ptr as *mut bool) = *b;
                }
                ConstValue::U8(n) => {
                    *(dest.ptr as *mut u8) = *n;
                }
                ConstValue::U16(n) => {
                    *(dest.ptr as *mut u16) = *n;
                }
                ConstValue::U32(n) => {
                    *(dest.ptr as *mut u32) = *n;
                }
                ConstValue::U64(n) => {
                    *(dest.ptr as *mut u64) = *n;
                }
                ConstValue::I8(n) => {
                    *(dest.ptr as *mut i8) = *n;
                }
                ConstValue::I16(n) => {
                    *(dest.ptr as *mut i16) = *n;
                }
                ConstValue::I32(n) => {
                    *(dest.ptr as *mut i32) = *n;
                }
                ConstValue::I64(n) => {
                    *(dest.ptr as *mut i64) = *n;
                }
            }
        }
        Ok(())
    }

    unsafe fn copy_value(&self, src: &Value, dest: Destination) -> Result<(), InterpError> {
        unsafe {
            let size = (*src.tydesc).size as usize;
            std::ptr::copy_nonoverlapping(src.ptr, dest.ptr, size);
        }
        Ok(())
    }

    unsafe fn move_value(&self, src: &Value, dest: Destination) -> Result<(), InterpError> {
        unsafe {
            let size = (*src.tydesc).size as usize;
            std::ptr::copy_nonoverlapping(src.ptr, dest.ptr, size);
        }
        Ok(())
    }

    fn execute_binop(
        &self,
        op: BinOp,
        lhs: &Value,
        rhs: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        // Macro to generate binop implementations for integer types.
        macro_rules! int_binop {
            ($tag:ident, $ty:ty, $lhs:expr, $rhs:expr, $dest:expr, $op:expr) => {
                if (*$lhs.tydesc).type_tag == rtdt::TyTag::$tag {
                    let a = *($lhs.ptr as *const $ty);
                    let b = *($rhs.ptr as *const $ty);
                    match $op {
                        BinOp::Add => { *($dest.ptr as *mut $ty) = a.wrapping_add(b); return Ok(()); }
                        BinOp::Sub => { *($dest.ptr as *mut $ty) = a.wrapping_sub(b); return Ok(()); }
                        BinOp::Mul => { *($dest.ptr as *mut $ty) = a.wrapping_mul(b); return Ok(()); }
                        BinOp::Div => {
                            if b == 0 { return Err(InterpError::DivisionByZero); }
                            *($dest.ptr as *mut $ty) = a.wrapping_div(b);
                            return Ok(());
                        }
                        BinOp::Mod => {
                            if b == 0 { return Err(InterpError::DivisionByZero); }
                            *($dest.ptr as *mut $ty) = a.wrapping_rem(b);
                            return Ok(());
                        }
                        BinOp::Lt => { *($dest.ptr as *mut bool) = a < b; return Ok(()); }
                        BinOp::Le => { *($dest.ptr as *mut bool) = a <= b; return Ok(()); }
                        BinOp::Gt => { *($dest.ptr as *mut bool) = a > b; return Ok(()); }
                        BinOp::Ge => { *($dest.ptr as *mut bool) = a >= b; return Ok(()); }
                        BinOp::Eq => { *($dest.ptr as *mut bool) = a == b; return Ok(()); }
                        BinOp::Ne => { *($dest.ptr as *mut bool) = a != b; return Ok(()); }
                        BinOp::BitAnd => { *($dest.ptr as *mut $ty) = a & b; return Ok(()); }
                        BinOp::BitOr => { *($dest.ptr as *mut $ty) = a | b; return Ok(()); }
                        BinOp::BitXor => { *($dest.ptr as *mut $ty) = a ^ b; return Ok(()); }
                        BinOp::Shl => { *($dest.ptr as *mut $ty) = a.wrapping_shl(b as u32); return Ok(()); }
                        BinOp::Shr => { *($dest.ptr as *mut $ty) = a.wrapping_shr(b as u32); return Ok(()); }
                        _ => {}
                    }
                }
            };
        }

        unsafe {
            let tag = (*lhs.tydesc).type_tag;

            // Try all integer types.
            int_binop!(I8, i8, lhs, rhs, dest, op);
            int_binop!(I16, i16, lhs, rhs, dest, op);
            int_binop!(I32, i32, lhs, rhs, dest, op);
            int_binop!(I64, i64, lhs, rhs, dest, op);
            int_binop!(U8, u8, lhs, rhs, dest, op);
            int_binop!(U16, u16, lhs, rhs, dest, op);
            int_binop!(U32, u32, lhs, rhs, dest, op);
            int_binop!(U64, u64, lhs, rhs, dest, op);
            // Int type currently stored as i64. TODO: Support bigint.
            int_binop!(Int, i64, lhs, rhs, dest, op);

            // F32 operations.
            if tag == rtdt::TyTag::F32 {
                let a = *(lhs.ptr as *const f32);
                let b = *(rhs.ptr as *const f32);
                match op {
                    BinOp::Add => { *(dest.ptr as *mut f32) = a + b; return Ok(()); }
                    BinOp::Sub => { *(dest.ptr as *mut f32) = a - b; return Ok(()); }
                    BinOp::Mul => { *(dest.ptr as *mut f32) = a * b; return Ok(()); }
                    BinOp::Div => { *(dest.ptr as *mut f32) = a / b; return Ok(()); }
                    BinOp::Lt => { *(dest.ptr as *mut bool) = a < b; return Ok(()); }
                    BinOp::Le => { *(dest.ptr as *mut bool) = a <= b; return Ok(()); }
                    BinOp::Gt => { *(dest.ptr as *mut bool) = a > b; return Ok(()); }
                    BinOp::Ge => { *(dest.ptr as *mut bool) = a >= b; return Ok(()); }
                    BinOp::Eq => { *(dest.ptr as *mut bool) = a == b; return Ok(()); }
                    BinOp::Ne => { *(dest.ptr as *mut bool) = a != b; return Ok(()); }
                    _ => {}
                }
            }

            // Boolean operations.
            if tag == rtdt::TyTag::Bool {
                let a = *(lhs.ptr as *const bool);
                let b = *(rhs.ptr as *const bool);
                match op {
                    BinOp::And => { *(dest.ptr as *mut bool) = a && b; return Ok(()); }
                    BinOp::Or => { *(dest.ptr as *mut bool) = a || b; return Ok(()); }
                    BinOp::Eq => { *(dest.ptr as *mut bool) = a == b; return Ok(()); }
                    BinOp::Ne => { *(dest.ptr as *mut bool) = a != b; return Ok(()); }
                    _ => {}
                }
            }

            Err(InterpError::TypeMismatch(
                format!("unsupported binop {:?} for type {:?}", op, tag)
            ))
        }
    }

    fn execute_unaryop(
        &self,
        op: UnaryOp,
        src: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        // Macro to generate unaryop implementations for signed integer types.
        macro_rules! signed_int_unaryop {
            ($tag:ident, $ty:ty, $src:expr, $dest:expr, $op:expr) => {
                if (*$src.tydesc).type_tag == rtdt::TyTag::$tag {
                    let a = *($src.ptr as *const $ty);
                    match $op {
                        UnaryOp::Neg => { *($dest.ptr as *mut $ty) = a.wrapping_neg(); return Ok(()); }
                        UnaryOp::BitNot => { *($dest.ptr as *mut $ty) = !a; return Ok(()); }
                        UnaryOp::Not => {}
                    }
                }
            };
        }

        // Macro for unsigned integers (only BitNot).
        macro_rules! unsigned_int_unaryop {
            ($tag:ident, $ty:ty, $src:expr, $dest:expr, $op:expr) => {
                if (*$src.tydesc).type_tag == rtdt::TyTag::$tag {
                    let a = *($src.ptr as *const $ty);
                    match $op {
                        UnaryOp::BitNot => { *($dest.ptr as *mut $ty) = !a; return Ok(()); }
                        _ => {}
                    }
                }
            };
        }

        unsafe {
            let tag = (*src.tydesc).type_tag;

            // Signed integer negation and bitnot.
            signed_int_unaryop!(I8, i8, src, dest, op);
            signed_int_unaryop!(I16, i16, src, dest, op);
            signed_int_unaryop!(I32, i32, src, dest, op);
            signed_int_unaryop!(I64, i64, src, dest, op);

            // Unsigned integer bitnot.
            unsigned_int_unaryop!(U8, u8, src, dest, op);
            unsigned_int_unaryop!(U16, u16, src, dest, op);
            unsigned_int_unaryop!(U32, u32, src, dest, op);
            unsigned_int_unaryop!(U64, u64, src, dest, op);

            // F32 negation.
            if tag == rtdt::TyTag::F32 && op == UnaryOp::Neg {
                let a = *(src.ptr as *const f32);
                *(dest.ptr as *mut f32) = -a;
                return Ok(());
            }

            // Boolean not.
            if tag == rtdt::TyTag::Bool && op == UnaryOp::Not {
                let a = *(src.ptr as *const bool);
                *(dest.ptr as *mut bool) = !a;
                return Ok(());
            }

            Err(InterpError::TypeMismatch(
                format!("unsupported unaryop {:?} for type {:?}", op, tag)
            ))
        }
    }

    /// Pack fields into a tuple at destination.
    fn execute_pack_tuple(
        &mut self,
        fields: &[Value],
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let tuple_info = (*dest.tydesc).type_info.tuple;
            for (i, field) in fields.iter().enumerate() {
                let field_info = &*tuple_info.fields.add(i);
                let field_dest = dest.ptr.add(field_info.offset as usize);
                let size = (*field.tydesc).size as usize;
                std::ptr::copy_nonoverlapping(field.ptr, field_dest, size);
            }
        }
        Ok(())
    }

    /// Pack fields into a struct at destination.
    fn execute_pack_struct(
        &mut self,
        fields: &[Value],
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let struct_info = (*dest.tydesc).type_info.struct_;
            for (i, field) in fields.iter().enumerate() {
                let field_info = &*struct_info.fields.add(i);
                let field_dest = dest.ptr.add(field_info.offset as usize);
                let size = (*field.tydesc).size as usize;
                std::ptr::copy_nonoverlapping(field.ptr, field_dest, size);
            }
        }
        Ok(())
    }

    /// Access a tuple field.
    fn execute_tuple_index(
        &self,
        base: &Value,
        index: u32,
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let tuple_info = (*base.tydesc).type_info.tuple;
            if index >= tuple_info.num_fields {
                return Err(InterpError::RuntimeError(
                    format!("tuple index {} out of bounds (len {})", index, tuple_info.num_fields)
                ));
            }
            let field_info = &*tuple_info.fields.add(index as usize);
            let field_ptr = base.ptr.add(field_info.offset as usize);
            let size = (*field_info.tydesc).size as usize;
            std::ptr::copy_nonoverlapping(field_ptr, dest.ptr, size);
        }
        Ok(())
    }

    /// Access a struct field by index.
    fn execute_field_access(
        &self,
        base: &Value,
        field_index: u32,
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let struct_info = (*base.tydesc).type_info.struct_;
            if field_index >= struct_info.num_fields {
                return Err(InterpError::RuntimeError(
                    format!("field index {} out of bounds (len {})", field_index, struct_info.num_fields)
                ));
            }
            let field_info = &*struct_info.fields.add(field_index as usize);
            let field_ptr = base.ptr.add(field_info.offset as usize);
            let size = (*field_info.tydesc).size as usize;
            std::ptr::copy_nonoverlapping(field_ptr, dest.ptr, size);
        }
        Ok(())
    }

    /// Execute checked arithmetic operation.
    fn execute_binop_checked(
        &self,
        op: BinOp,
        lhs: &Value,
        rhs: &Value,
        dest: Destination,
        overflow_dest: Destination,
    ) -> Result<(), InterpError> {
        // Macro to generate checked binop implementations for integer types.
        macro_rules! checked_int_binop {
            ($tag:ident, $ty:ty, $lhs:expr, $rhs:expr, $dest:expr, $overflow:expr, $op:expr) => {
                if (*$lhs.tydesc).type_tag == rtdt::TyTag::$tag {
                    let a = *($lhs.ptr as *const $ty);
                    let b = *($rhs.ptr as *const $ty);
                    let (result, overflowed) = match $op {
                        BinOp::Add => a.overflowing_add(b),
                        BinOp::Sub => a.overflowing_sub(b),
                        BinOp::Mul => a.overflowing_mul(b),
                        _ => return Err(InterpError::TypeMismatch(
                            format!("checked binop only supports Add/Sub/Mul, got {:?}", $op)
                        )),
                    };
                    *($dest.ptr as *mut $ty) = result;
                    *($overflow.ptr as *mut bool) = overflowed;
                    return Ok(());
                }
            };
        }

        unsafe {
            checked_int_binop!(I8, i8, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(I16, i16, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(I32, i32, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(I64, i64, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(U8, u8, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(U16, u16, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(U32, u32, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(U64, u64, lhs, rhs, dest, overflow_dest, op);

            let tag = (*lhs.tydesc).type_tag;
            Err(InterpError::TypeMismatch(
                format!("unsupported checked binop {:?} for type {:?}", op, tag)
            ))
        }
    }

    /// Wrap a value in Some.
    fn execute_wrap_some(
        &self,
        inner: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let option_info = (*dest.tydesc).type_info.option;
            let layout = rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(dest.tydesc));
            // Tag is at offset 0. Set to Some (1).
            *(dest.ptr as *mut u8) = rtdt::OptionTag::Some as u8;
            // Copy inner value.
            let inner_size = (*option_info.inner_tydesc).size as usize;
            std::ptr::copy_nonoverlapping(
                inner.ptr,
                dest.ptr.add(layout.payload_offset as usize),
                inner_size,
            );
        }
        Ok(())
    }

    /// Create a None value.
    fn execute_wrap_none(&self, dest: Destination) -> Result<(), InterpError> {
        unsafe {
            // Tag is at offset 0. Set to None (0).
            *(dest.ptr as *mut u8) = rtdt::OptionTag::None as u8;
        }
        Ok(())
    }

    /// Unwrap an Option, producing (inner_value, is_some).
    fn execute_unwrap_option(
        &self,
        src: &Value,
        dest: Destination,
        is_some_dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let option_info = (*src.tydesc).type_info.option;
            let layout = rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(src.tydesc));
            // Tag is at offset 0.
            let tag = *(src.ptr as *const u8);
            let is_some = tag != rtdt::OptionTag::None as u8;
            *(is_some_dest.ptr as *mut bool) = is_some;
            if is_some {
                let inner_size = (*option_info.inner_tydesc).size as usize;
                std::ptr::copy_nonoverlapping(
                    src.ptr.add(layout.payload_offset as usize),
                    dest.ptr,
                    inner_size,
                );
            }
        }
        Ok(())
    }

    /// Wrap a value in Ok.
    fn execute_wrap_ok(
        &self,
        inner: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let result_info = (*dest.tydesc).type_info.result;
            let layout = rtdt::layout::compute_result_layout(rtdt::TyDescRef::from_ptr(dest.tydesc));
            // Tag is at offset 0. Set to Ok.
            *(dest.ptr as *mut u8) = rtdt::ResultTag::Ok as u8;
            // Copy inner value.
            let inner_size = (*result_info.ok_tydesc).size as usize;
            std::ptr::copy_nonoverlapping(
                inner.ptr,
                dest.ptr.add(layout.payload_offset as usize),
                inner_size,
            );
        }
        Ok(())
    }

    /// Wrap a value in Err.
    fn execute_wrap_err(
        &self,
        inner: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let layout = rtdt::layout::compute_result_layout(rtdt::TyDescRef::from_ptr(dest.tydesc));
            // Tag is at offset 0. Set to Err.
            *(dest.ptr as *mut u8) = rtdt::ResultTag::Err as u8;
            // Copy Error value.
            let inner_size = (*inner.tydesc).size as usize;
            std::ptr::copy_nonoverlapping(
                inner.ptr,
                dest.ptr.add(layout.payload_offset as usize),
                inner_size,
            );
        }
        Ok(())
    }

    /// Unwrap a Result, producing (inner_value, is_ok).
    fn execute_unwrap_result(
        &self,
        src: &Value,
        dest: Destination,
        is_ok_dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let result_info = (*src.tydesc).type_info.result;
            let layout = rtdt::layout::compute_result_layout(rtdt::TyDescRef::from_ptr(src.tydesc));
            // Tag is at offset 0.
            let tag = *(src.ptr as *const u8);
            let is_ok = tag == rtdt::ResultTag::Ok as u8;
            *(is_ok_dest.ptr as *mut bool) = is_ok;
            // Copy payload (ok value or error).
            let inner_size = if is_ok {
                (*result_info.ok_tydesc).size as usize
            } else {
                std::mem::size_of::<rtdt::Error>()
            };
            std::ptr::copy_nonoverlapping(
                src.ptr.add(layout.payload_offset as usize),
                dest.ptr,
                inner_size,
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IrBlock, Terminator, FuncRef, FuncId, TypeRef};

    #[test]
    fn test_tydesc_table_primitives() {
        let mut table = IrTyDescTable::new();

        let bool_td = table.get_or_create(&IrType::Bool);
        unsafe {
            assert_eq!((*bool_td).type_tag, rtdt::TyTag::Bool);
            assert_eq!((*bool_td).size, 1);
        }

        let i64_td = table.get_or_create(&IrType::I64);
        unsafe {
            assert_eq!((*i64_td).type_tag, rtdt::TyTag::I64);
            assert_eq!((*i64_td).size, 8);
        }
    }

    #[test]
    fn test_layout_computation() {
        let mut table = IrTyDescTable::new();

        let value_types = vec![IrType::I64, IrType::Bool, IrType::I64];
        let slot_types = vec![IrType::I64];

        let layout = IrLayout::compute(&value_types, &slot_types, &mut table);

        // i64 at 0, bool at 8, i64 at 16, slot i64 at 24.
        assert_eq!(layout.value_offsets[0], 0);
        assert_eq!(layout.value_offsets[1], 8);
        assert_eq!(layout.value_offsets[2], 16);
        assert_eq!(layout.slot_offsets[0], 24);
        assert_eq!(layout.frame_size, 32);
    }

    /// Helper to create a simple function for testing.
    fn make_add_function() -> IrFunction {
        // fn add(a: i64, b: i64) -> i64 { a + b }
        IrFunction {
            id: FuncId(0),
            name: "add".to_string(),
            params: vec![ValueId(0), ValueId(1)],  // a, b
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::BinOp {
                            dest: ValueId(2),
                            op: BinOp::Add,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(2))),
                    },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, IrType::I64],
            slot_types: vec![],
        }
    }

    #[test]
    fn test_simple_function_call() {
        // Create the add function.
        let add_fn = make_add_function();

        // Create main function that calls add(10, 20).
        // fn main() -> i64 { add(10, 20) }
        let main_fn = IrFunction {
            id: FuncId(1),
            name: "main".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::I64(10),
                        },
                        Instruction::Const {
                            dest: ValueId(1),
                            value: ConstValue::I64(20),
                        },
                        Instruction::Call {
                            dest: ValueId(2),
                            func: FuncRef::Local(FuncId(0)),  // add function
                            args: vec![
                                Operand::Value(ValueId(0)),
                                Operand::Value(ValueId(1)),
                            ],
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(2))),
                    },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        // Create context with both functions.
        let functions = vec![add_fn, main_fn.clone()];
        let ctx = ExecutionContext::new(&functions);

        // Execute main, writing result to our storage.
        let mut interp = IrInterpreter::new();
        let mut result_storage: i64 = 0;
        let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
        let ret_dest = Destination {
            ptr: &mut result_storage as *mut i64 as *mut u8,
            tydesc: ret_tydesc,
        };
        let registry = FunctionRegistry::new();
        let mut frames = FrameStore::new();
        interp.call_in_context(&main_fn, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();

        // Verify result is 30.
        assert_eq!(result_storage, 30);
    }

    #[test]
    fn test_nested_function_calls() {
        // fn double(x: i64) -> i64 { x + x }
        let double_fn = IrFunction {
            id: FuncId(0),
            name: "double".to_string(),
            params: vec![ValueId(0)],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::BinOp {
                            dest: ValueId(1),
                            op: BinOp::Add,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(0)),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(1))),
                    },
                },
            ],
            value_count: 2,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        // fn quadruple(x: i64) -> i64 { double(double(x)) }
        let quadruple_fn = IrFunction {
            id: FuncId(1),
            name: "quadruple".to_string(),
            params: vec![ValueId(0)],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        // First call: double(x)
                        Instruction::Call {
                            dest: ValueId(1),
                            func: FuncRef::Local(FuncId(0)),
                            args: vec![Operand::Value(ValueId(0))],
                        },
                        // Second call: double(result)
                        Instruction::Call {
                            dest: ValueId(2),
                            func: FuncRef::Local(FuncId(0)),
                            args: vec![Operand::Value(ValueId(1))],
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(2))),
                    },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        // Create context with both functions.
        let functions = vec![double_fn, quadruple_fn.clone()];
        let ctx = ExecutionContext::new(&functions);

        // Create argument value: 5.
        let mut interp = IrInterpreter::new();
        let mut arg_storage = 5i64;
        let arg_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
        let arg = Value {
            ptr: &mut arg_storage as *mut i64 as *mut u8,
            tydesc: arg_tydesc,
        };

        // Create destination for result.
        let mut result_storage: i64 = 0;
        let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
        let ret_dest = Destination {
            ptr: &mut result_storage as *mut i64 as *mut u8,
            tydesc: ret_tydesc,
        };

        let registry = FunctionRegistry::new();
        let mut frames = FrameStore::new();
        interp.call_in_context(&quadruple_fn, vec![arg], ret_dest, &ctx, &registry, &mut frames).unwrap();

        // Verify result is 20 (5 * 2 * 2).
        assert_eq!(result_storage, 20);
    }

    /// Helper to run a function and get an i64 result.
    fn run_i64_function(func: &IrFunction) -> i64 {
        let functions = [func.clone()];
        let ctx = ExecutionContext::new(&functions);
        let mut interp = IrInterpreter::new();
        let mut result: i64 = 0;
        let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
        let ret_dest = Destination {
            ptr: &mut result as *mut i64 as *mut u8,
            tydesc: ret_tydesc,
        };
        let registry = FunctionRegistry::new();
        let mut frames = FrameStore::new();
        interp.call_in_context(func, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();
        result
    }

    /// Helper to run a function and get a u32 result.
    fn run_u32_function(func: &IrFunction) -> u32 {
        let functions = [func.clone()];
        let ctx = ExecutionContext::new(&functions);
        let mut interp = IrInterpreter::new();
        let mut result: u32 = 0;
        let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::U32);
        let ret_dest = Destination {
            ptr: &mut result as *mut u32 as *mut u8,
            tydesc: ret_tydesc,
        };
        let registry = FunctionRegistry::new();
        let mut frames = FrameStore::new();
        interp.call_in_context(func, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();
        result
    }

    /// Helper to run a function and get a bool result.
    fn run_bool_function(func: &IrFunction) -> bool {
        let functions = [func.clone()];
        let ctx = ExecutionContext::new(&functions);
        let mut interp = IrInterpreter::new();
        let mut result: bool = false;
        let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::Bool);
        let ret_dest = Destination {
            ptr: &mut result as *mut bool as *mut u8,
            tydesc: ret_tydesc,
        };
        let registry = FunctionRegistry::new();
        let mut frames = FrameStore::new();
        interp.call_in_context(func, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();
        result
    }

    // =========================================================================
    // Const tests for all types
    // =========================================================================

    #[test]
    fn test_const_u8() {
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::U8(42),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(0))),
                    },
                },
            ],
            value_count: 1,
            slot_count: 0,
            value_types: vec![IrType::U8],
            slot_types: vec![],
        };

        let functions = [func.clone()];
        let ctx = ExecutionContext::new(&functions);
        let mut interp = IrInterpreter::new();
        let mut result: u8 = 0;
        let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::U8);
        let ret_dest = Destination {
            ptr: &mut result as *mut u8,
            tydesc: ret_tydesc,
        };
        let registry = FunctionRegistry::new();
        let mut frames = FrameStore::new();
        interp.call_in_context(&func, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_const_i32() {
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::I32(-12345),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(0))),
                    },
                },
            ],
            value_count: 1,
            slot_count: 0,
            value_types: vec![IrType::I32],
            slot_types: vec![],
        };

        let functions = [func.clone()];
        let ctx = ExecutionContext::new(&functions);
        let mut interp = IrInterpreter::new();
        let mut result: i32 = 0;
        let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::I32);
        let ret_dest = Destination {
            ptr: &mut result as *mut i32 as *mut u8,
            tydesc: ret_tydesc,
        };
        let registry = FunctionRegistry::new();
        let mut frames = FrameStore::new();
        interp.call_in_context(&func, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();
        assert_eq!(result, -12345);
    }

    #[test]
    fn test_const_bool() {
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::Bool(true),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(0))),
                    },
                },
            ],
            value_count: 1,
            slot_count: 0,
            value_types: vec![IrType::Bool],
            slot_types: vec![],
        };

        assert!(run_bool_function(&func));
    }

    // =========================================================================
    // BinOp tests
    // =========================================================================

    #[test]
    fn test_binop_sub() {
        // fn test() -> i64 { 100 - 42 }
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(100) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(42) },
                        Instruction::BinOp {
                            dest: ValueId(2),
                            op: BinOp::Sub,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 58);
    }

    #[test]
    fn test_binop_mul() {
        // fn test() -> i64 { 7 * 6 }
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(7) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(6) },
                        Instruction::BinOp {
                            dest: ValueId(2),
                            op: BinOp::Mul,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 42);
    }

    #[test]
    fn test_binop_div() {
        // fn test() -> i64 { 84 / 2 }
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(84) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(2) },
                        Instruction::BinOp {
                            dest: ValueId(2),
                            op: BinOp::Div,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 42);
    }

    #[test]
    fn test_binop_mod() {
        // fn test() -> i64 { 47 % 5 }
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(47) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(5) },
                        Instruction::BinOp {
                            dest: ValueId(2),
                            op: BinOp::Mod,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 2);
    }

    #[test]
    fn test_binop_eq() {
        // fn test() -> bool { 42 == 42 }
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(42) },
                        Instruction::BinOp {
                            dest: ValueId(2),
                            op: BinOp::Eq,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, IrType::Bool],
            slot_types: vec![],
        };

        assert!(run_bool_function(&func));
    }

    #[test]
    fn test_binop_ne() {
        // fn test() -> bool { 1 != 2 }
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(1) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(2) },
                        Instruction::BinOp {
                            dest: ValueId(2),
                            op: BinOp::Ne,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, IrType::Bool],
            slot_types: vec![],
        };

        assert!(run_bool_function(&func));
    }

    #[test]
    fn test_binop_lt() {
        // fn test() -> bool { 1 < 2 }
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(1) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(2) },
                        Instruction::BinOp {
                            dest: ValueId(2),
                            op: BinOp::Lt,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, IrType::Bool],
            slot_types: vec![],
        };

        assert!(run_bool_function(&func));
    }

    #[test]
    fn test_binop_bitand() {
        // fn test() -> i64 { 0b1100 & 0b1010 } = 0b1000 = 8
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(0b1100) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(0b1010) },
                        Instruction::BinOp {
                            dest: ValueId(2),
                            op: BinOp::BitAnd,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 8);
    }

    #[test]
    fn test_binop_bitor() {
        // fn test() -> i64 { 0b1100 | 0b1010 } = 0b1110 = 14
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(0b1100) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(0b1010) },
                        Instruction::BinOp {
                            dest: ValueId(2),
                            op: BinOp::BitOr,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 14);
    }

    #[test]
    fn test_binop_shl() {
        // fn test() -> i64 { 1 << 4 } = 16
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(1) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(4) },
                        Instruction::BinOp {
                            dest: ValueId(2),
                            op: BinOp::Shl,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 16);
    }

    // =========================================================================
    // BinOpChecked tests
    // =========================================================================

    #[test]
    fn test_binop_checked_no_overflow() {
        // fn test() -> (i64, bool) { checked_add(10, 20) }
        // Returns (30, false) - no overflow.
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(20) },
                        Instruction::BinOpChecked {
                            dest: ValueId(2),
                            overflow: ValueId(3),
                            op: BinOp::Add,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
                },
            ],
            value_count: 4,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, IrType::I64, IrType::Bool],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 30);
    }

    #[test]
    fn test_binop_checked_overflow() {
        // fn test() -> bool { let (_, overflow) = checked_add(i64::MAX, 1); overflow }
        // Returns true - overflow occurred.
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(i64::MAX) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(1) },
                        Instruction::BinOpChecked {
                            dest: ValueId(2),
                            overflow: ValueId(3),
                            op: BinOp::Add,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(3))) },
                },
            ],
            value_count: 4,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, IrType::I64, IrType::Bool],
            slot_types: vec![],
        };

        assert!(run_bool_function(&func));
    }

    // =========================================================================
    // UnaryOp tests
    // =========================================================================

    #[test]
    fn test_unaryop_neg() {
        // fn test() -> i64 { -42 }
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                        Instruction::UnaryOp {
                            dest: ValueId(1),
                            op: UnaryOp::Neg,
                            operand: Operand::Value(ValueId(0)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
                },
            ],
            value_count: 2,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), -42);
    }

    #[test]
    fn test_unaryop_not() {
        // fn test() -> bool { !true }
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::Bool(true) },
                        Instruction::UnaryOp {
                            dest: ValueId(1),
                            op: UnaryOp::Not,
                            operand: Operand::Value(ValueId(0)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
                },
            ],
            value_count: 2,
            slot_count: 0,
            value_types: vec![IrType::Bool, IrType::Bool],
            slot_types: vec![],
        };

        assert!(!run_bool_function(&func));
    }

    #[test]
    fn test_unaryop_bitnot() {
        // fn test() -> u32 { ~0u32 } = 0xFFFFFFFF
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::U32(0) },
                        Instruction::UnaryOp {
                            dest: ValueId(1),
                            op: UnaryOp::BitNot,
                            operand: Operand::Value(ValueId(0)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
                },
            ],
            value_count: 2,
            slot_count: 0,
            value_types: vec![IrType::U32, IrType::U32],
            slot_types: vec![],
        };

        assert_eq!(run_u32_function(&func), 0xFFFFFFFF);
    }

    // =========================================================================
    // SlotStore/SlotLoad tests
    // =========================================================================

    #[test]
    fn test_slot_store_load() {
        // var x = 10; x = x + 5; ret x
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        // var x = 10
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                        Instruction::SlotStore { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(0)) },
                        // x + 5
                        Instruction::SlotLoad { dest: ValueId(1), slot: SlotId(0) },
                        Instruction::Const { dest: ValueId(2), value: ConstValue::I64(5) },
                        Instruction::BinOp {
                            dest: ValueId(3),
                            op: BinOp::Add,
                            lhs: Operand::Value(ValueId(1)),
                            rhs: Operand::Value(ValueId(2)),
                        },
                        // x = result
                        Instruction::SlotStore { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(3)) },
                        // ret x
                        Instruction::SlotLoad { dest: ValueId(4), slot: SlotId(0) },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(4))) },
                },
            ],
            value_count: 5,
            slot_count: 1,
            value_types: vec![IrType::I64, IrType::I64, IrType::I64, IrType::I64, IrType::I64],
            slot_types: vec![IrType::I64],
        };

        assert_eq!(run_i64_function(&func), 15);
    }

    #[test]
    fn test_slot_multiple_updates() {
        // var x = 1; x = x * 2; x = x * 2; x = x * 2; ret x
        // 1 -> 2 -> 4 -> 8
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(1) },
                        Instruction::SlotStore { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(0)) },
                        // x = x * 2
                        Instruction::SlotLoad { dest: ValueId(1), slot: SlotId(0) },
                        Instruction::Const { dest: ValueId(2), value: ConstValue::I64(2) },
                        Instruction::BinOp { dest: ValueId(3), op: BinOp::Mul, lhs: Operand::Value(ValueId(1)), rhs: Operand::Value(ValueId(2)) },
                        Instruction::SlotStore { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(3)) },
                        // x = x * 2
                        Instruction::SlotLoad { dest: ValueId(4), slot: SlotId(0) },
                        Instruction::Const { dest: ValueId(5), value: ConstValue::I64(2) },
                        Instruction::BinOp { dest: ValueId(6), op: BinOp::Mul, lhs: Operand::Value(ValueId(4)), rhs: Operand::Value(ValueId(5)) },
                        Instruction::SlotStore { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(6)) },
                        // x = x * 2
                        Instruction::SlotLoad { dest: ValueId(7), slot: SlotId(0) },
                        Instruction::Const { dest: ValueId(8), value: ConstValue::I64(2) },
                        Instruction::BinOp { dest: ValueId(9), op: BinOp::Mul, lhs: Operand::Value(ValueId(7)), rhs: Operand::Value(ValueId(8)) },
                        Instruction::SlotStore { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(9)) },
                        // ret x
                        Instruction::SlotLoad { dest: ValueId(10), slot: SlotId(0) },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(10))) },
                },
            ],
            value_count: 11,
            slot_count: 1,
            value_types: vec![IrType::I64; 11],
            slot_types: vec![IrType::I64],
        };

        assert_eq!(run_i64_function(&func), 8);
    }

    // =========================================================================
    // Control flow tests
    // =========================================================================

    #[test]
    fn test_branch_true() {
        // fn test() -> i64 { if true { 1 } else { 2 } }
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::Bool(true) },
                    ],
                    terminator: Terminator::Branch {
                        cond: Operand::Value(ValueId(0)),
                        then_block: BlockId(1),
                        else_block: BlockId(2),
                    },
                },
                IrBlock {
                    id: BlockId(1),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(1) },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
                },
                IrBlock {
                    id: BlockId(2),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(2), value: ConstValue::I64(2) },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::Bool, IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 1);
    }

    #[test]
    fn test_branch_false() {
        // fn test() -> i64 { if false { 1 } else { 2 } }
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::Bool(false) },
                    ],
                    terminator: Terminator::Branch {
                        cond: Operand::Value(ValueId(0)),
                        then_block: BlockId(1),
                        else_block: BlockId(2),
                    },
                },
                IrBlock {
                    id: BlockId(1),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(1) },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
                },
                IrBlock {
                    id: BlockId(2),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(2), value: ConstValue::I64(2) },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::Bool, IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 2);
    }

    #[test]
    fn test_goto_chain() {
        // block0 -> block1 -> block2 (return)
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(1) },
                    ],
                    terminator: Terminator::Goto(BlockId(1)),
                },
                IrBlock {
                    id: BlockId(1),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(2) },
                        Instruction::BinOp {
                            dest: ValueId(2),
                            op: BinOp::Add,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Goto(BlockId(2)),
                },
                IrBlock {
                    id: BlockId(2),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(3), value: ConstValue::I64(3) },
                        Instruction::BinOp {
                            dest: ValueId(4),
                            op: BinOp::Add,
                            lhs: Operand::Value(ValueId(2)),
                            rhs: Operand::Value(ValueId(3)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(4))) },
                },
            ],
            value_count: 5,
            slot_count: 0,
            value_types: vec![IrType::I64; 5],
            slot_types: vec![],
        };

        // 1 + 2 + 3 = 6
        assert_eq!(run_i64_function(&func), 6);
    }

    // =========================================================================
    // Copy/Move tests
    // =========================================================================

    #[test]
    fn test_copy() {
        // fn test() -> i64 { let a = 42; let b = a; b }
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                        Instruction::Copy { dest: ValueId(1), src: Operand::Value(ValueId(0)) },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
                },
            ],
            value_count: 2,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 42);
    }

    #[test]
    fn test_move() {
        // fn test() -> i64 { let a = 42; let b = move a; b }
        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                        Instruction::Move { dest: ValueId(1), src: Operand::Value(ValueId(0)) },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
                },
            ],
            value_count: 2,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 42);
    }

    // =========================================================================
    // Pack/Unpack tests
    // =========================================================================

    #[test]
    fn test_pack_tuple() {
        // fn test() -> (i64, i64) { (10, 20) }
        // Returns packed tuple, then extract first element.
        let tuple_ty = IrType::Tuple(vec![IrType::I64, IrType::I64]);

        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(20) },
                        Instruction::Pack {
                            dest: ValueId(2),
                            ty: TypeRef::Tuple(0),
                            fields: vec![Operand::Value(ValueId(0)), Operand::Value(ValueId(1))],
                        },
                        Instruction::TupleIndex {
                            dest: ValueId(3),
                            base: Operand::Value(ValueId(2)),
                            index: 0,
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(3))) },
                },
            ],
            value_count: 4,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, tuple_ty, IrType::I64],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 10);
    }

    #[test]
    fn test_unpack_tuple() {
        // fn test() -> i64 { let (a, b) = (10, 20); a + b }
        let tuple_ty = IrType::Tuple(vec![IrType::I64, IrType::I64]);

        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        // Create tuple (10, 20).
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(20) },
                        Instruction::Pack {
                            dest: ValueId(2),
                            ty: TypeRef::Tuple(0),
                            fields: vec![Operand::Value(ValueId(0)), Operand::Value(ValueId(1))],
                        },
                        // Unpack to (a, b).
                        Instruction::Unpack {
                            dests: vec![ValueId(3), ValueId(4)],
                            src: Operand::Value(ValueId(2)),
                        },
                        // a + b
                        Instruction::BinOp {
                            dest: ValueId(5),
                            op: BinOp::Add,
                            lhs: Operand::Value(ValueId(3)),
                            rhs: Operand::Value(ValueId(4)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(5))) },
                },
            ],
            value_count: 6,
            slot_count: 0,
            value_types: vec![
                IrType::I64, IrType::I64, tuple_ty,
                IrType::I64, IrType::I64, IrType::I64
            ],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 30);
    }

    #[test]
    fn test_tuple_index_second() {
        // fn test() -> i64 { let t = (10, 20, 30); t.1 }
        let tuple_ty = IrType::Tuple(vec![IrType::I64, IrType::I64, IrType::I64]);

        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(20) },
                        Instruction::Const { dest: ValueId(2), value: ConstValue::I64(30) },
                        Instruction::Pack {
                            dest: ValueId(3),
                            ty: TypeRef::Tuple(0),
                            fields: vec![
                                Operand::Value(ValueId(0)),
                                Operand::Value(ValueId(1)),
                                Operand::Value(ValueId(2)),
                            ],
                        },
                        Instruction::TupleIndex {
                            dest: ValueId(4),
                            base: Operand::Value(ValueId(3)),
                            index: 1,
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(4))) },
                },
            ],
            value_count: 5,
            slot_count: 0,
            value_types: vec![
                IrType::I64, IrType::I64, IrType::I64, tuple_ty, IrType::I64
            ],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 20);
    }

    // =========================================================================
    // Option tests
    // =========================================================================

    #[test]
    fn test_wrap_some_unwrap() {
        // fn test() -> i64 { let opt = Some(42); opt.unwrap() }
        let opt_ty = IrType::Option(Box::new(IrType::I64));

        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                        Instruction::WrapSome {
                            dest: ValueId(1),
                            inner: Operand::Value(ValueId(0)),
                        },
                        Instruction::UnwrapOption {
                            dest: ValueId(2),
                            is_some: ValueId(3),
                            src: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
                },
            ],
            value_count: 4,
            slot_count: 0,
            value_types: vec![IrType::I64, opt_ty, IrType::I64, IrType::Bool],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 42);
    }

    #[test]
    fn test_is_some() {
        // fn test() -> bool { let opt = Some(42); opt.is_some() }
        let opt_ty = IrType::Option(Box::new(IrType::I64));

        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                        Instruction::WrapSome {
                            dest: ValueId(1),
                            inner: Operand::Value(ValueId(0)),
                        },
                        Instruction::UnwrapOption {
                            dest: ValueId(2),
                            is_some: ValueId(3),
                            src: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(3))) },
                },
            ],
            value_count: 4,
            slot_count: 0,
            value_types: vec![IrType::I64, opt_ty, IrType::I64, IrType::Bool],
            slot_types: vec![],
        };

        assert!(run_bool_function(&func));
    }

    #[test]
    fn test_is_none() {
        // fn test() -> bool { let opt: ?i64 = none; !opt.is_some() }
        let opt_ty = IrType::Option(Box::new(IrType::I64));

        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::WrapNone { dest: ValueId(0) },
                        Instruction::UnwrapOption {
                            dest: ValueId(1),
                            is_some: ValueId(2),
                            src: Operand::Value(ValueId(0)),
                        },
                        Instruction::UnaryOp {
                            dest: ValueId(3),
                            op: UnaryOp::Not,
                            operand: Operand::Value(ValueId(2)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(3))) },
                },
            ],
            value_count: 4,
            slot_count: 0,
            value_types: vec![opt_ty, IrType::I64, IrType::Bool, IrType::Bool],
            slot_types: vec![],
        };

        assert!(run_bool_function(&func));
    }

    #[test]
    fn test_option_branch() {
        // fn test() -> i64 { if Some(42).is_some() { 1 } else { 0 } }
        let opt_ty = IrType::Option(Box::new(IrType::I64));

        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                        Instruction::WrapSome {
                            dest: ValueId(1),
                            inner: Operand::Value(ValueId(0)),
                        },
                        Instruction::UnwrapOption {
                            dest: ValueId(2),
                            is_some: ValueId(3),
                            src: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Branch {
                        cond: Operand::Value(ValueId(3)),
                        then_block: BlockId(1),
                        else_block: BlockId(2),
                    },
                },
                IrBlock {
                    id: BlockId(1),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(4), value: ConstValue::I64(1) },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(4))) },
                },
                IrBlock {
                    id: BlockId(2),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(5), value: ConstValue::I64(0) },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(5))) },
                },
            ],
            value_count: 6,
            slot_count: 0,
            value_types: vec![IrType::I64, opt_ty, IrType::I64, IrType::Bool, IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 1);
    }

    // =========================================================================
    // Result tests
    // =========================================================================

    #[test]
    fn test_wrap_ok_unwrap() {
        // fn test() -> i64 { let res = Ok(42); res.unwrap() }
        let res_ty = IrType::Result(Box::new(IrType::I64));

        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                        Instruction::WrapOk {
                            dest: ValueId(1),
                            inner: Operand::Value(ValueId(0)),
                        },
                        Instruction::UnwrapResult {
                            dest: ValueId(2),
                            is_ok: ValueId(3),
                            src: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
                },
            ],
            value_count: 4,
            slot_count: 0,
            value_types: vec![IrType::I64, res_ty, IrType::I64, IrType::Bool],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 42);
    }

    #[test]
    fn test_is_ok() {
        // fn test() -> bool { let res = Ok(42); res.is_ok() }
        let res_ty = IrType::Result(Box::new(IrType::I64));

        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                        Instruction::WrapOk {
                            dest: ValueId(1),
                            inner: Operand::Value(ValueId(0)),
                        },
                        Instruction::UnwrapResult {
                            dest: ValueId(2),
                            is_ok: ValueId(3),
                            src: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(3))) },
                },
            ],
            value_count: 4,
            slot_count: 0,
            value_types: vec![IrType::I64, res_ty, IrType::I64, IrType::Bool],
            slot_types: vec![],
        };

        assert!(run_bool_function(&func));
    }

    #[test]
    fn test_is_err() {
        // fn test() -> bool { let res: !i64 = err; !res.is_ok() }
        let res_ty = IrType::Result(Box::new(IrType::I64));

        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        // Create an error value for WrapErr.
                        // Error is represented as IrType::Error.
                        Instruction::Const { dest: ValueId(0), value: ConstValue::Unit },
                        Instruction::WrapErr {
                            dest: ValueId(1),
                            inner: Operand::Value(ValueId(0)),
                        },
                        Instruction::UnwrapResult {
                            dest: ValueId(2),
                            is_ok: ValueId(3),
                            src: Operand::Value(ValueId(1)),
                        },
                        Instruction::UnaryOp {
                            dest: ValueId(4),
                            op: UnaryOp::Not,
                            operand: Operand::Value(ValueId(3)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(4))) },
                },
            ],
            value_count: 5,
            slot_count: 0,
            value_types: vec![IrType::Unit, res_ty, IrType::I64, IrType::Bool, IrType::Bool],
            slot_types: vec![],
        };

        assert!(run_bool_function(&func));
    }

    #[test]
    fn test_result_branch() {
        // fn test() -> i64 { if Ok(42).is_ok() { 1 } else { 0 } }
        let res_ty = IrType::Result(Box::new(IrType::I64));

        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                        Instruction::WrapOk {
                            dest: ValueId(1),
                            inner: Operand::Value(ValueId(0)),
                        },
                        Instruction::UnwrapResult {
                            dest: ValueId(2),
                            is_ok: ValueId(3),
                            src: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Branch {
                        cond: Operand::Value(ValueId(3)),
                        then_block: BlockId(1),
                        else_block: BlockId(2),
                    },
                },
                IrBlock {
                    id: BlockId(1),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(4), value: ConstValue::I64(1) },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(4))) },
                },
                IrBlock {
                    id: BlockId(2),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(5), value: ConstValue::I64(0) },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(5))) },
                },
            ],
            value_count: 6,
            slot_count: 0,
            value_types: vec![IrType::I64, res_ty, IrType::I64, IrType::Bool, IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 1);
    }

    // =========================================================================
    // Struct tests
    // =========================================================================

    #[test]
    fn test_pack_struct() {
        // fn test() -> i64 { let s = { x: 10, y: 20 }; s.x }
        let struct_ty = IrType::Struct(vec![
            ("x".to_string(), IrType::I64),
            ("y".to_string(), IrType::I64),
        ]);

        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(20) },
                        Instruction::Pack {
                            dest: ValueId(2),
                            ty: TypeRef::AnonStruct(0),
                            fields: vec![Operand::Value(ValueId(0)), Operand::Value(ValueId(1))],
                        },
                        Instruction::FieldAccess {
                            dest: ValueId(3),
                            base: Operand::Value(ValueId(2)),
                            field_index: 0,
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(3))) },
                },
            ],
            value_count: 4,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, struct_ty, IrType::I64],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 10);
    }

    #[test]
    fn test_field_access_second() {
        // fn test() -> i64 { let s = { a: 10, b: 20, c: 30 }; s.b }
        let struct_ty = IrType::Struct(vec![
            ("a".to_string(), IrType::I64),
            ("b".to_string(), IrType::I64),
            ("c".to_string(), IrType::I64),
        ]);

        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(20) },
                        Instruction::Const { dest: ValueId(2), value: ConstValue::I64(30) },
                        Instruction::Pack {
                            dest: ValueId(3),
                            ty: TypeRef::AnonStruct(0),
                            fields: vec![
                                Operand::Value(ValueId(0)),
                                Operand::Value(ValueId(1)),
                                Operand::Value(ValueId(2)),
                            ],
                        },
                        Instruction::FieldAccess {
                            dest: ValueId(4),
                            base: Operand::Value(ValueId(3)),
                            field_index: 1,
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(4))) },
                },
            ],
            value_count: 5,
            slot_count: 0,
            value_types: vec![
                IrType::I64, IrType::I64, IrType::I64, struct_ty, IrType::I64
            ],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 20);
    }

    #[test]
    fn test_unpack_struct() {
        // fn test() -> i64 { let { x, y } = { x: 10, y: 20 }; x + y }
        let struct_ty = IrType::Struct(vec![
            ("x".to_string(), IrType::I64),
            ("y".to_string(), IrType::I64),
        ]);

        let func = IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        // Create struct { x: 10, y: 20 }.
                        Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                        Instruction::Const { dest: ValueId(1), value: ConstValue::I64(20) },
                        Instruction::Pack {
                            dest: ValueId(2),
                            ty: TypeRef::AnonStruct(0),
                            fields: vec![Operand::Value(ValueId(0)), Operand::Value(ValueId(1))],
                        },
                        // Unpack to (x, y).
                        Instruction::Unpack {
                            dests: vec![ValueId(3), ValueId(4)],
                            src: Operand::Value(ValueId(2)),
                        },
                        // x + y
                        Instruction::BinOp {
                            dest: ValueId(5),
                            op: BinOp::Add,
                            lhs: Operand::Value(ValueId(3)),
                            rhs: Operand::Value(ValueId(4)),
                        },
                    ],
                    terminator: Terminator::Return { value: Some(Operand::Value(ValueId(5))) },
                },
            ],
            value_count: 6,
            slot_count: 0,
            value_types: vec![
                IrType::I64, IrType::I64, struct_ty,
                IrType::I64, IrType::I64, IrType::I64
            ],
            slot_types: vec![],
        };

        assert_eq!(run_i64_function(&func), 30);
    }

    // ==========================================================================
    // Cross-unit reference tests
    // ==========================================================================

    #[test]
    fn test_crossunit_external_value() {
        // Unit 0: let x = 42
        let unit0 = IrScriptUnit {
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::I64(42),
                        },
                    ],
                    terminator: Terminator::UnitEnd { result: None },
                },
            ],
            value_count: 1,
            slot_count: 0,
            value_types: vec![IrType::I64],
            slot_types: vec![],
            functions: vec![],
            symbols: crate::ir::SymbolTable::new(),
            result: None,
            exports: vec![("x".to_string(), crate::ir::ExportBinding::Value(ValueId(0)))],
        };

        // Unit 1: return x (from unit 0)
        let unit1 = IrScriptUnit {
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        // Copy external value to local for return.
                        Instruction::Copy {
                            dest: ValueId(0),
                            src: Operand::ExternalValue { unit: 0, value: ValueId(0) },
                        },
                    ],
                    terminator: Terminator::UnitEnd { result: Some(Operand::Value(ValueId(0))) },
                },
            ],
            value_count: 1,
            slot_count: 0,
            value_types: vec![IrType::I64],
            slot_types: vec![],
            functions: vec![],
            symbols: crate::ir::SymbolTable::new(),
            result: Some(ValueId(0)),
            exports: vec![],
        };

        // Execute both units.
        let mut interp = IrInterpreter::new();
        let mut env = ScriptEnvironment::new();

        // Execute unit 0 (no result).
        let unit_tydesc = interp.tydesc_table.get_or_create(&IrType::Unit);
        let mut dummy = [0u8; 0];
        let dummy_dest = Destination { ptr: dummy.as_mut_ptr(), tydesc: unit_tydesc };
        interp.execute_script_unit_in_env(&unit0, &mut env, dummy_dest).unwrap();

        // Execute unit 1 (returns x).
        let mut result: i64 = 0;
        let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
        let ret_dest = Destination {
            ptr: &mut result as *mut i64 as *mut u8,
            tydesc: ret_tydesc,
        };
        interp.execute_script_unit_in_env(&unit1, &mut env, ret_dest).unwrap();

        assert_eq!(result, 42);
    }

    #[test]
    fn test_crossunit_external_slot() {
        // Unit 0: var y = 10
        let unit0 = IrScriptUnit {
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::I64(10),
                        },
                        Instruction::SlotStore {
                            dest: SlotDest::Local(SlotId(0)),
                            value: Operand::Value(ValueId(0)),
                        },
                    ],
                    terminator: Terminator::UnitEnd { result: None },
                },
            ],
            value_count: 1,
            slot_count: 1,
            value_types: vec![IrType::I64],
            slot_types: vec![IrType::I64],
            functions: vec![],
            symbols: crate::ir::SymbolTable::new(),
            result: None,
            exports: vec![("y".to_string(), crate::ir::ExportBinding::Slot(SlotId(0)))],
        };

        // Unit 1: return y (from unit 0's slot)
        let unit1 = IrScriptUnit {
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        // Copy external slot to local value for return.
                        Instruction::Copy {
                            dest: ValueId(0),
                            src: Operand::ExternalSlot { unit: 0, slot: SlotId(0) },
                        },
                    ],
                    terminator: Terminator::UnitEnd { result: Some(Operand::Value(ValueId(0))) },
                },
            ],
            value_count: 1,
            slot_count: 0,
            value_types: vec![IrType::I64],
            slot_types: vec![],
            functions: vec![],
            symbols: crate::ir::SymbolTable::new(),
            result: Some(ValueId(0)),
            exports: vec![],
        };

        // Execute both units.
        let mut interp = IrInterpreter::new();
        let mut env = ScriptEnvironment::new();

        // Execute unit 0.
        let unit_tydesc = interp.tydesc_table.get_or_create(&IrType::Unit);
        let mut dummy = [0u8; 0];
        let dummy_dest = Destination { ptr: dummy.as_mut_ptr(), tydesc: unit_tydesc };
        interp.execute_script_unit_in_env(&unit0, &mut env, dummy_dest).unwrap();

        // Execute unit 1.
        let mut result: i64 = 0;
        let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
        let ret_dest = Destination {
            ptr: &mut result as *mut i64 as *mut u8,
            tydesc: ret_tydesc,
        };
        interp.execute_script_unit_in_env(&unit1, &mut env, ret_dest).unwrap();

        assert_eq!(result, 10);
    }

    #[test]
    fn test_crossunit_external_function() {
        // Unit 0: fn double(x: i64) -> i64 { x + x }
        let double_fn = IrFunction {
            id: FuncId(0),
            name: "double".to_string(),
            params: vec![ValueId(0)],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::BinOp {
                            dest: ValueId(1),
                            op: BinOp::Add,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(0)),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(1))),
                    },
                },
            ],
            value_count: 2,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64],
            slot_types: vec![],
        };

        let unit0 = IrScriptUnit {
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![],
                    terminator: Terminator::UnitEnd { result: None },
                },
            ],
            value_count: 0,
            slot_count: 0,
            value_types: vec![],
            slot_types: vec![],
            functions: vec![double_fn],
            symbols: crate::ir::SymbolTable::new(),
            result: None,
            exports: vec![("double".to_string(), crate::ir::ExportBinding::Function(FuncId(0)))],
        };

        // Unit 1: return double(7)
        let unit1 = IrScriptUnit {
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::I64(7),
                        },
                        Instruction::Call {
                            dest: ValueId(1),
                            func: FuncRef::External { unit: 0, func: FuncId(0) },
                            args: vec![Operand::Value(ValueId(0))],
                        },
                    ],
                    terminator: Terminator::UnitEnd { result: Some(Operand::Value(ValueId(1))) },
                },
            ],
            value_count: 2,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64],
            slot_types: vec![],
            functions: vec![],
            symbols: crate::ir::SymbolTable::new(),
            result: Some(ValueId(1)),
            exports: vec![],
        };

        // Execute both units.
        let mut interp = IrInterpreter::new();
        let mut env = ScriptEnvironment::new();

        // Execute unit 0.
        let unit_tydesc = interp.tydesc_table.get_or_create(&IrType::Unit);
        let mut dummy = [0u8; 0];
        let dummy_dest = Destination { ptr: dummy.as_mut_ptr(), tydesc: unit_tydesc };
        interp.execute_script_unit_in_env(&unit0, &mut env, dummy_dest).unwrap();

        // Execute unit 1.
        let mut result: i64 = 0;
        let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
        let ret_dest = Destination {
            ptr: &mut result as *mut i64 as *mut u8,
            tydesc: ret_tydesc,
        };
        interp.execute_script_unit_in_env(&unit1, &mut env, ret_dest).unwrap();

        assert_eq!(result, 14);  // 7 + 7 = 14
    }

    #[test]
    fn test_crossunit_chain() {
        // Unit 0: let a = 5
        let unit0 = IrScriptUnit {
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::I64(5),
                        },
                    ],
                    terminator: Terminator::UnitEnd { result: None },
                },
            ],
            value_count: 1,
            slot_count: 0,
            value_types: vec![IrType::I64],
            slot_types: vec![],
            functions: vec![],
            symbols: crate::ir::SymbolTable::new(),
            result: None,
            exports: vec![("a".to_string(), crate::ir::ExportBinding::Value(ValueId(0)))],
        };

        // Unit 1: let b = a + 3
        let unit1 = IrScriptUnit {
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        // Load a from unit 0.
                        Instruction::Copy {
                            dest: ValueId(0),
                            src: Operand::ExternalValue { unit: 0, value: ValueId(0) },
                        },
                        Instruction::Const {
                            dest: ValueId(1),
                            value: ConstValue::I64(3),
                        },
                        Instruction::BinOp {
                            dest: ValueId(2),
                            op: BinOp::Add,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::UnitEnd { result: None },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I64, IrType::I64, IrType::I64],
            slot_types: vec![],
            functions: vec![],
            symbols: crate::ir::SymbolTable::new(),
            result: None,
            exports: vec![("b".to_string(), crate::ir::ExportBinding::Value(ValueId(2)))],
        };

        // Unit 2: return b
        let unit2 = IrScriptUnit {
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        // Load b from unit 1.
                        Instruction::Copy {
                            dest: ValueId(0),
                            src: Operand::ExternalValue { unit: 1, value: ValueId(2) },
                        },
                    ],
                    terminator: Terminator::UnitEnd { result: Some(Operand::Value(ValueId(0))) },
                },
            ],
            value_count: 1,
            slot_count: 0,
            value_types: vec![IrType::I64],
            slot_types: vec![],
            functions: vec![],
            symbols: crate::ir::SymbolTable::new(),
            result: Some(ValueId(0)),
            exports: vec![],
        };

        // Execute all units.
        let mut interp = IrInterpreter::new();
        let mut env = ScriptEnvironment::new();

        let unit_tydesc = interp.tydesc_table.get_or_create(&IrType::Unit);
        let mut dummy = [0u8; 0];
        let dummy_dest = Destination { ptr: dummy.as_mut_ptr(), tydesc: unit_tydesc };

        interp.execute_script_unit_in_env(&unit0, &mut env, dummy_dest).unwrap();
        interp.execute_script_unit_in_env(&unit1, &mut env, dummy_dest).unwrap();

        let mut result: i64 = 0;
        let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
        let ret_dest = Destination {
            ptr: &mut result as *mut i64 as *mut u8,
            tydesc: ret_tydesc,
        };
        interp.execute_script_unit_in_env(&unit2, &mut env, ret_dest).unwrap();

        assert_eq!(result, 8);  // 5 + 3 = 8
    }
}
