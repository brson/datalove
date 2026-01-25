//! Compile-time function evaluation (CTFE) support.
//!
//! Implements the `CtfeEvaluator` trait for the interpreter, allowing
//! const expressions to be evaluated at compile time.

use datalove_datafun_ir::{ConstValue, CtfeError, CtfeEvaluator, IrScriptUnit, IrType};
use crate::{IrInterpreter, ScriptEnvironment, UnitCompletion, Destination};

/// CTFE evaluator backed by the IR interpreter.
pub struct InterpCtfeEvaluator {
    interp: IrInterpreter,
}

impl InterpCtfeEvaluator {
    /// Create a new CTFE evaluator.
    pub fn new() -> Self {
        Self {
            interp: IrInterpreter::new(),
        }
    }
}

impl Default for InterpCtfeEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl CtfeEvaluator for InterpCtfeEvaluator {
    fn evaluate(&mut self, unit: &IrScriptUnit, result_type: &IrType) -> Result<ConstValue, CtfeError> {
        let mut env = ScriptEnvironment::new();

        // Allocate space for the result.
        let result_tydesc = self.interp.tydesc_table_mut().get_or_create(result_type);
        let result_size = unsafe { (*result_tydesc).size as usize };
        let mut result_buffer = vec![0u8; result_size.max(8)];
        let result_dest = Destination {
            ptr: result_buffer.as_mut_ptr(),
            tydesc: result_tydesc,
        };

        // Allocate space for early return (Result<(), Error>).
        let ret_type = IrType::Result(Box::new(IrType::Unit));
        let ret_tydesc = self.interp.tydesc_table_mut().get_or_create(&ret_type);
        let ret_size = unsafe { (*ret_tydesc).size as usize };
        let mut ret_buffer = vec![0u8; ret_size.max(8)];
        let ret_dest = Destination {
            ptr: ret_buffer.as_mut_ptr(),
            tydesc: ret_tydesc,
        };

        // Execute the unit.
        let completion = self.interp
            .execute_script_unit_in_env(unit, &mut env, ret_dest, Some(result_dest))
            .map_err(|e| CtfeError::InterpError(format!("{:?}", e)))?;

        // Clean up the environment (frames from executed units).
        // This must happen before env is dropped to free any heap allocations.
        env.destroy_live_values(self.interp.runtime_handle());

        let result = match completion {
            UnitCompletion::Normal => {
                // Extract the result value into a ConstValue.
                extract_const_value(result_buffer.as_ptr(), result_type)
            }
            UnitCompletion::EarlyReturn => {
                Err(CtfeError::EarlyReturn(
                    "const expression returned early via ! or ?".to_string()
                ))
            }
        };

        // Destroy the result value in the buffer to free heap allocations (e.g., bigint limbs).
        // This must happen after extract_const_value since it reads from the buffer.
        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                self.interp.runtime_handle(),
                result_buffer.as_mut_ptr(),
                result_tydesc,
            );
        }

        result
    }
}

/// Get the alignment of an IR type.
///
/// Used for computing payload offsets in composite types like Option.
fn align_of_ir_type(ir_type: &IrType) -> u32 {
    match ir_type {
        IrType::Unit => 1,
        IrType::Bool => 1,
        IrType::U8 | IrType::I8 => 1,
        IrType::U16 | IrType::I16 => 2,
        IrType::U32 | IrType::I32 | IrType::F32 => 4,
        IrType::U64 | IrType::I64 | IrType::F64 => 8,
        IrType::Usize | IrType::Isize => std::mem::size_of::<usize>() as u32,
        // Int, String, collections are pointer types - align to pointer size.
        IrType::Int | IrType::String | IrType::Data | IrType::Error => 8,
        IrType::List(_) | IrType::Set(_) | IrType::Map(_, _) => 8,
        IrType::Tuple(_) | IrType::Struct(_) | IrType::Enum(_) => 8,
        IrType::Option(inner) => align_of_ir_type(inner).max(1),
        IrType::Result(inner) => align_of_ir_type(inner).max(8), // Error is pointer-sized
        IrType::Tensor(_, _) => 8,
        IrType::Ref(_) => 8,
        IrType::Table(_) => 8,
    }
}

/// Extract a ConstValue from raw memory.
///
/// Reads bytes from the given pointer and converts to a ConstValue based on type.
fn extract_const_value(ptr: *const u8, ir_type: &IrType) -> Result<ConstValue, CtfeError> {
    unsafe {
        match ir_type {
            IrType::Unit => Ok(ConstValue::Unit),
            IrType::Bool => {
                let val = *(ptr as *const bool);
                Ok(ConstValue::Bool(val))
            }
            IrType::U8 => {
                let val = *(ptr as *const u8);
                Ok(ConstValue::U8(val))
            }
            IrType::U16 => {
                let val = *(ptr as *const u16);
                Ok(ConstValue::U16(val))
            }
            IrType::U32 => {
                let val = *(ptr as *const u32);
                Ok(ConstValue::U32(val))
            }
            IrType::U64 => {
                let val = *(ptr as *const u64);
                Ok(ConstValue::U64(val))
            }
            IrType::I8 => {
                let val = *(ptr as *const i8);
                Ok(ConstValue::I8(val))
            }
            IrType::I16 => {
                let val = *(ptr as *const i16);
                Ok(ConstValue::I16(val))
            }
            IrType::I32 => {
                let val = *(ptr as *const i32);
                Ok(ConstValue::I32(val))
            }
            IrType::I64 => {
                let val = *(ptr as *const i64);
                Ok(ConstValue::I64(val))
            }
            IrType::Usize => {
                let val = *(ptr as *const datalove_rtdt::UsizeRepr);
                Ok(ConstValue::Usize(val))
            }
            IrType::Isize => {
                let val = *(ptr as *const datalove_rtdt::IsizeRepr);
                Ok(ConstValue::Isize(val))
            }
            IrType::F32 => {
                let val = *(ptr as *const f32);
                Ok(ConstValue::F32(val))
            }
            IrType::F64 => {
                let val = *(ptr as *const f64);
                Ok(ConstValue::F64(val))
            }
            IrType::Int => {
                // Bigint is stored as: data pointer, size_and_sign, capacity.
                let int_ptr = ptr as *const datalove_rtdt::Int;
                let int_val = &*int_ptr;

                let num_limbs = int_val.size_and_sign.unsigned_abs() as usize;
                let negative = int_val.size_and_sign < 0;

                let limbs = if num_limbs == 0 {
                    Vec::new()
                } else {
                    std::slice::from_raw_parts(int_val.data, num_limbs).to_vec()
                };

                Ok(ConstValue::Int { limbs, negative })
            }
            IrType::Option(inner) => {
                // Option layout: tag (u8) at offset 0, payload at aligned offset.
                let tag = *(ptr as *const u8);
                match tag {
                    1 => Ok(ConstValue::OptionNone),
                    2 => {
                        // Compute payload offset based on inner type alignment.
                        let inner_align = align_of_ir_type(inner);
                        let payload_offset = datalove_rtdt::layout::option_payload_offset(inner_align);
                        let payload_ptr = ptr.add(payload_offset as usize);
                        let inner_value = extract_const_value(payload_ptr, inner)?;
                        Ok(ConstValue::OptionSome(Box::new(inner_value)))
                    }
                    _ => Err(CtfeError::InterpError(format!(
                        "invalid option tag: {}", tag
                    ))),
                }
            }
            // TODO: Add support for aggregates and collections.
            _ => Err(CtfeError::UnsupportedType(format!("{:?}", ir_type))),
        }
    }
}
