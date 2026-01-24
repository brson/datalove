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
            // TODO: Add support for aggregates and collections.
            _ => Err(CtfeError::UnsupportedType(format!("{:?}", ir_type))),
        }
    }
}
