//! Evaluators for datafun expressions.

use rmx::prelude::*;
use crate::ast::*;
use crate::interp::{InterpContext, InterpResult, InterpError};
use crate::value::Value;

/// Evaluate a datafun expression.
pub fn eval_expr<'db>(ctx: &mut InterpContext<'db>, expr: ExprFun<'db>) -> InterpResult {
    match expr.expr(ctx.db) {
        ExprFunKind::Datalit(datalit_expr) => {
            crate::eval_datalit::eval_datalit(ctx, datalit_expr)
        }

        ExprFunKind::Name(name) => eval_name(ctx, name),

        ExprFunKind::BinOp(binop) => eval_binop(ctx, binop),

        ExprFunKind::ParseError(err) => {
            let message = err.message(ctx.db);
            Err(InterpError::RuntimeError(
                format!("Parse error: {}", message.as_str(ctx.db)),
            ))
        }
    }
}

/// Evaluate a name (variable reference).
fn eval_name<'db>(
    ctx: &mut InterpContext<'db>,
    name: bct::text::InternedText<'db>,
) -> InterpResult {
    use datalove_rt as rt;

    // Look up the variable and extract the necessary information.
    // We need to do this in two stages to avoid borrow checker issues.

    // Stage 1: Determine the value type and extract ptr/tydesc if needed.
    enum ValueCloneInfo {
        Bool(bool),
        U32(u32),
        F32(f32),
        HeapAllocated {
            ptr: *const u8,
            tydesc: *const datalove_rtdt::TyDesc,
            kind: HeapValueKind,
        },
        Unimplemented,
    }

    #[derive(Debug)]
    enum HeapValueKind {
        Int,
        String,
        Tuple,
        Struct,
        Enum,
        List,
        Map,
        Set,
        Option,
        Result,
    }

    let value_info = {
        let value = ctx.lookup_variable(name)?;

        match value {
            Value::Bool(b) => ValueCloneInfo::Bool(*b),
            Value::U32(n) => ValueCloneInfo::U32(*n),
            Value::F32(f) => ValueCloneInfo::F32(*f),

            Value::Int { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::Int,
            },

            Value::String { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::String,
            },

            Value::Tuple { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::Tuple,
            },

            Value::Struct { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::Struct,
            },

            Value::Enum { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::Enum,
            },

            Value::List { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::List,
            },

            Value::Map { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::Map,
            },

            Value::Set { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::Set,
            },

            Value::Option { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::Option,
            },

            Value::Result { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::Result,
            },

            Value::Data { .. } | Value::Error { .. } => ValueCloneInfo::Unimplemented,
        }
    };

    // Stage 2: Clone the value.
    match value_info {
        ValueCloneInfo::Bool(b) => Ok(Value::Bool(b)),
        ValueCloneInfo::U32(n) => Ok(Value::U32(n)),
        ValueCloneInfo::F32(f) => Ok(Value::F32(f)),

        ValueCloneInfo::HeapAllocated { ptr, tydesc, kind } => {
            // Allocate new value based on kind.
            let mut new_value = unsafe {
                match kind {
                    HeapValueKind::Int => Value::alloc_int(&mut ctx.rt, tydesc),
                    HeapValueKind::String => Value::alloc_string(&mut ctx.rt, tydesc),
                    HeapValueKind::Tuple => Value::alloc_tuple(&mut ctx.rt, tydesc),
                    HeapValueKind::Struct => Value::alloc_struct(&mut ctx.rt, tydesc),
                    HeapValueKind::Enum => Value::alloc_enum(&mut ctx.rt, tydesc),
                    HeapValueKind::List => Value::alloc_list(&mut ctx.rt, tydesc),
                    HeapValueKind::Map => Value::alloc_map(&mut ctx.rt, tydesc),
                    HeapValueKind::Set => Value::alloc_set(&mut ctx.rt, tydesc),
                    HeapValueKind::Option => Value::alloc_option(&mut ctx.rt, tydesc),
                    HeapValueKind::Result => Value::alloc_result(&mut ctx.rt, tydesc),
                }
            };

            // Get handle to runtime.
            let rt_handle = Box::as_mut(&mut ctx.rt) as *mut _ as rt::LocalRtHandle;

            // Clone the value.
            let status = unsafe {
                rt::dtlv_rti_clone_local(
                    rt_handle,
                    ptr,
                    tydesc,
                    new_value.as_mut_ptr(),
                )
            };

            if status != rt::RtStatus::Ok {
                return Err(InterpError::RuntimeError(
                    format!("Failed to clone {:?} value", kind),
                ));
            }

            Ok(new_value)
        }

        ValueCloneInfo::Unimplemented => {
            Err(InterpError::NotImplemented(
                "cloning Data/Error values".to_string(),
            ))
        }
    }
}

/// Evaluate a binary operation.
fn eval_binop<'db>(ctx: &mut InterpContext<'db>, binop: ExprBinOp<'db>) -> InterpResult {
    let op = binop.op(ctx.db);
    let lhs = binop.lhs(ctx.db);
    let rhs = binop.rhs(ctx.db);

    // Evaluate operands.
    let lhs_value = eval_expr(ctx, lhs)?;
    let rhs_value = eval_expr(ctx, rhs)?;

    use BinOp::*;

    match op {
        // Basic arithmetic.
        Add => eval_add(ctx, lhs_value, rhs_value),
        Sub => eval_sub(ctx, lhs_value, rhs_value),
        Mul => eval_mul(ctx, lhs_value, rhs_value),
        Div => eval_div(ctx, lhs_value, rhs_value),

        // Checked arithmetic.
        AddChecked => eval_add_checked(ctx, lhs_value, rhs_value),
        SubChecked => eval_sub_checked(ctx, lhs_value, rhs_value),
        MulChecked => eval_mul_checked(ctx, lhs_value, rhs_value),
        DivChecked => eval_div_checked(ctx, lhs_value, rhs_value),

        // Optional arithmetic.
        AddOptional => eval_add_optional(ctx, lhs_value, rhs_value),
        SubOptional => eval_sub_optional(ctx, lhs_value, rhs_value),
        MulOptional => eval_mul_optional(ctx, lhs_value, rhs_value),
        DivOptional => eval_div_optional(ctx, lhs_value, rhs_value),

        // Saturating arithmetic.
        AddSaturating => eval_add_saturating(ctx, lhs_value, rhs_value),
        SubSaturating => eval_sub_saturating(ctx, lhs_value, rhs_value),
        MulSaturating => eval_mul_saturating(ctx, lhs_value, rhs_value),
        DivSaturating => eval_div_saturating(ctx, lhs_value, rhs_value),

        // Comparison operators.
        Lt => eval_lt(ctx, lhs_value, rhs_value),
        Gt => eval_gt(ctx, lhs_value, rhs_value),
        Le => eval_le(ctx, lhs_value, rhs_value),
        Ge => eval_ge(ctx, lhs_value, rhs_value),
        Eq => eval_eq(ctx, lhs_value, rhs_value),
        Ne => eval_ne(ctx, lhs_value, rhs_value),
    }
}

/// Evaluate addition.
fn eval_add(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => {
            // Wrapping addition.
            Ok(Value::from_u32(a.wrapping_add(b)))
        }
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_f32(a + b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for addition".to_string(),
        )),
    }
}

/// Evaluate subtraction.
fn eval_sub(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => {
            // Wrapping subtraction.
            Ok(Value::from_u32(a.wrapping_sub(b)))
        }
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_f32(a - b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for subtraction".to_string(),
        )),
    }
}

/// Evaluate multiplication.
fn eval_mul(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => {
            // Wrapping multiplication.
            Ok(Value::from_u32(a.wrapping_mul(b)))
        }
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_f32(a * b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for multiplication".to_string(),
        )),
    }
}

/// Evaluate division.
fn eval_div(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => {
            if b == 0 {
                Err(InterpError::DivisionByZero)
            } else {
                Ok(Value::from_u32(a / b))
            }
        }
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_f32(a / b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for division".to_string(),
        )),
    }
}

/// Evaluate checked addition.
fn eval_add_checked(ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => {
            // Return Result<u32>.
            match a.checked_add(b) {
                Some(result) => {
                    // TODO: Create Result::Ok value.
                    Err(InterpError::NotImplemented("Result type".to_string()))
                }
                None => {
                    // TODO: Create Result::Err value.
                    Err(InterpError::NotImplemented("Result type".to_string()))
                }
            }
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for checked addition".to_string(),
        )),
    }
}

/// Evaluate checked subtraction.
fn eval_sub_checked(_ctx: &mut InterpContext<'_>, _lhs: Value, _rhs: Value) -> InterpResult {
    Err(InterpError::NotImplemented("checked subtraction".to_string()))
}

/// Evaluate checked multiplication.
fn eval_mul_checked(_ctx: &mut InterpContext<'_>, _lhs: Value, _rhs: Value) -> InterpResult {
    Err(InterpError::NotImplemented("checked multiplication".to_string()))
}

/// Evaluate checked division.
fn eval_div_checked(_ctx: &mut InterpContext<'_>, _lhs: Value, _rhs: Value) -> InterpResult {
    Err(InterpError::NotImplemented("checked division".to_string()))
}

/// Evaluate optional addition.
fn eval_add_optional(_ctx: &mut InterpContext<'_>, _lhs: Value, _rhs: Value) -> InterpResult {
    Err(InterpError::NotImplemented("optional addition".to_string()))
}

/// Evaluate optional subtraction.
fn eval_sub_optional(_ctx: &mut InterpContext<'_>, _lhs: Value, _rhs: Value) -> InterpResult {
    Err(InterpError::NotImplemented("optional subtraction".to_string()))
}

/// Evaluate optional multiplication.
fn eval_mul_optional(_ctx: &mut InterpContext<'_>, _lhs: Value, _rhs: Value) -> InterpResult {
    Err(InterpError::NotImplemented("optional multiplication".to_string()))
}

/// Evaluate optional division.
fn eval_div_optional(_ctx: &mut InterpContext<'_>, _lhs: Value, _rhs: Value) -> InterpResult {
    Err(InterpError::NotImplemented("optional division".to_string()))
}

/// Evaluate saturating addition.
fn eval_add_saturating(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_u32(a.saturating_add(b))),
        _ => Err(InterpError::TypeError(
            "Unsupported types for saturating addition".to_string(),
        )),
    }
}

/// Evaluate saturating subtraction.
fn eval_sub_saturating(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_u32(a.saturating_sub(b))),
        _ => Err(InterpError::TypeError(
            "Unsupported types for saturating subtraction".to_string(),
        )),
    }
}

/// Evaluate saturating multiplication.
fn eval_mul_saturating(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_u32(a.saturating_mul(b))),
        _ => Err(InterpError::TypeError(
            "Unsupported types for saturating multiplication".to_string(),
        )),
    }
}

/// Evaluate saturating division.
fn eval_div_saturating(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => {
            if b == 0 {
                Err(InterpError::DivisionByZero)
            } else {
                Ok(Value::from_u32(a / b))
            }
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for saturating division".to_string(),
        )),
    }
}

/// Evaluate less than.
fn eval_lt(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a < b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a < b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for comparison".to_string(),
        )),
    }
}

/// Evaluate greater than.
fn eval_gt(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a > b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a > b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for comparison".to_string(),
        )),
    }
}

/// Evaluate less than or equal.
fn eval_le(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a <= b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a <= b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for comparison".to_string(),
        )),
    }
}

/// Evaluate greater than or equal.
fn eval_ge(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a >= b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a >= b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for comparison".to_string(),
        )),
    }
}

/// Evaluate equality.
fn eval_eq(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::Bool(a), Value::Bool(b)) => Ok(Value::from_bool(a == b)),
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a == b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a == b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for equality".to_string(),
        )),
    }
}

/// Evaluate inequality.
fn eval_ne(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::Bool(a), Value::Bool(b)) => Ok(Value::from_bool(a != b)),
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a != b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a != b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for inequality".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_table::TypeTable;
    use crate::tycheck::type_check;
    use rmx::prelude::*;

    #[test]
    fn test_variable_reference_u32() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @42\nlet y = x"));
        let script = crate::parser::parse(&db, source);

        // Type check the script.
        let tycheck_result = type_check(&db, script);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        // Build type table.
        let type_table = TypeTable::build(&db, script, tycheck_result)
            .expect("Failed to build type table");

        // Create interpreter context.
        let mut ctx = InterpContext::new(&db, type_table);

        // Execute the script.
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        // Verify that y has the same value as x.
        let x_name = bct::text::InternedText::new(&db, S("x"));
        let y_name = bct::text::InternedText::new(&db, S("y"));

        let x_value = ctx.lookup_variable(x_name).expect("x not found");
        let y_value = ctx.lookup_variable(y_name).expect("y not found");

        match (x_value, y_value) {
            (Value::U32(x), Value::U32(y)) => {
                assert_eq!(x, y, "x and y should have the same value");
                assert_eq!(*x, 42);
            }
            _ => panic!("Expected U32 values for x and y"),
        }
    }

    #[test]
    fn test_variable_reference_in_expression() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @10\nlet y = x + @5"));
        let script = crate::parser::parse(&db, source);

        // Type check the script.
        let tycheck_result = type_check(&db, script);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        // Build type table.
        let type_table = TypeTable::build(&db, script, tycheck_result)
            .expect("Failed to build type table");

        // Create interpreter context.
        let mut ctx = InterpContext::new(&db, type_table);

        // Execute the script.
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        // Verify that y = x + 5 = 15.
        let y_name = bct::text::InternedText::new(&db, S("y"));
        let y_value = ctx.lookup_variable(y_name).expect("y not found");

        match y_value {
            Value::U32(y) => {
                assert_eq!(*y, 15);
            }
            _ => panic!("Expected U32 value for y"),
        }
    }

    #[test]
    fn test_multiple_variable_references() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let a = @100\nlet b = a\nlet c = a + b"));
        let script = crate::parser::parse(&db, source);

        // Type check the script.
        let tycheck_result = type_check(&db, script);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        // Build type table.
        let type_table = TypeTable::build(&db, script, tycheck_result)
            .expect("Failed to build type table");

        // Create interpreter context.
        let mut ctx = InterpContext::new(&db, type_table);

        // Execute the script.
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        // Verify that c = a + b = 100 + 100 = 200.
        let c_name = bct::text::InternedText::new(&db, S("c"));
        let c_value = ctx.lookup_variable(c_name).expect("c not found");

        match c_value {
            Value::U32(c) => {
                assert_eq!(*c, 200);
            }
            _ => panic!("Expected U32 value for c"),
        }
    }

    // Tests for complex datalit expressions enabled by direct token parsing

    #[test]
    fn test_eval_datalit_tuple() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @(1, 2, 3)"));
        let script = crate::parser::parse(&db, source);

        let tycheck_result = type_check(&db, script);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        let type_table = TypeTable::build(&db, script, tycheck_result)
            .expect("Failed to build type table");

        let mut ctx = InterpContext::new(&db, type_table);
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        let x_name = bct::text::InternedText::new(&db, S("x"));
        let x_value = ctx.lookup_variable(x_name).expect("x not found");

        // Verify it's a tuple value.
        match x_value {
            Value::Tuple { .. } => {
                // Successfully created tuple
            }
            _ => panic!("Expected Tuple value for x"),
        }
    }

    #[test]
    fn test_eval_datalit_list() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @[1, 2, 3]"));
        let script = crate::parser::parse(&db, source);

        let tycheck_result = type_check(&db, script);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        let type_table = TypeTable::build(&db, script, tycheck_result)
            .expect("Failed to build type table");

        let mut ctx = InterpContext::new(&db, type_table);
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        let x_name = bct::text::InternedText::new(&db, S("x"));
        let x_value = ctx.lookup_variable(x_name).expect("x not found");

        // Verify it's a list value.
        match x_value {
            Value::List { .. } => {
                // Successfully created list
            }
            _ => panic!("Expected List value for x"),
        }
    }

    #[test]
    fn test_eval_datalit_map() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @map { @1 = @10, @2 = @20 }"));
        let script = crate::parser::parse(&db, source);

        let tycheck_result = type_check(&db, script);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        let type_table = TypeTable::build(&db, script, tycheck_result)
            .expect("Failed to build type table");

        let mut ctx = InterpContext::new(&db, type_table);
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        let x_name = bct::text::InternedText::new(&db, S("x"));
        let x_value = ctx.lookup_variable(x_name).expect("x not found");

        // Verify it's a map value.
        match x_value {
            Value::Map { .. } => {
                // Successfully created map
            }
            _ => panic!("Expected Map value for x"),
        }
    }

    #[test]
    fn test_eval_datalit_nested_tuple_in_list() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @[(1, 2), (3, 4)]"));
        let script = crate::parser::parse(&db, source);

        let tycheck_result = type_check(&db, script);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        let type_table = TypeTable::build(&db, script, tycheck_result)
            .expect("Failed to build type table");

        let mut ctx = InterpContext::new(&db, type_table);
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        let x_name = bct::text::InternedText::new(&db, S("x"));
        let x_value = ctx.lookup_variable(x_name).expect("x not found");

        // Verify it's a list value containing tuples.
        match x_value {
            Value::List { .. } => {
                // Successfully created nested structure
            }
            _ => panic!("Expected List value for x"),
        }
    }

    #[test]
    fn test_eval_datalit_nested_list_in_tuple() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @(@[@1, @2, @3], @100)"));
        let script = crate::parser::parse(&db, source);

        let tycheck_result = type_check(&db, script);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        let type_table = TypeTable::build(&db, script, tycheck_result)
            .expect("Failed to build type table");

        let mut ctx = InterpContext::new(&db, type_table);
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        let x_name = bct::text::InternedText::new(&db, S("x"));
        let x_value = ctx.lookup_variable(x_name).expect("x not found");

        // Verify it's a tuple value with nested list.
        match x_value {
            Value::Tuple { .. } => {
                // Successfully created nested tuple with list
            }
            _ => panic!("Expected Tuple value for x"),
        }
    }

    #[test]
    fn test_eval_datalit_set() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @set { @1, @2, @3 }"));
        let script = crate::parser::parse(&db, source);

        let tycheck_result = type_check(&db, script);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        let type_table = TypeTable::build(&db, script, tycheck_result)
            .expect("Failed to build type table");

        let mut ctx = InterpContext::new(&db, type_table);
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        let x_name = bct::text::InternedText::new(&db, S("x"));
        let x_value = ctx.lookup_variable(x_name).expect("x not found");

        // Verify it's a set value.
        match x_value {
            Value::Set { .. } => {
                // Successfully created set
            }
            _ => panic!("Expected Set value for x"),
        }
    }

    #[test]
    fn test_eval_datalit_deeply_nested() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @(@[@(@1, @2)], @[@(@3, @4)])"));
        let script = crate::parser::parse(&db, source);

        let tycheck_result = type_check(&db, script);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        let type_table = TypeTable::build(&db, script, tycheck_result)
            .expect("Failed to build type table");

        let mut ctx = InterpContext::new(&db, type_table);
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        let x_name = bct::text::InternedText::new(&db, S("x"));
        let x_value = ctx.lookup_variable(x_name).expect("x not found");

        // Verify it's a tuple with deeply nested structure.
        match x_value {
            Value::Tuple { .. } => {
                // Successfully created deeply nested structure
            }
            _ => panic!("Expected Tuple value for x"),
        }
    }

    #[test]
    fn test_eval_variable_reference_tuple() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @(1, 2, 3)\nlet y = x"));
        let script = crate::parser::parse(&db, source);

        let tycheck_result = type_check(&db, script);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        let type_table = TypeTable::build(&db, script, tycheck_result)
            .expect("Failed to build type table");

        let mut ctx = InterpContext::new(&db, type_table);
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        let x_name = bct::text::InternedText::new(&db, S("x"));
        let y_name = bct::text::InternedText::new(&db, S("y"));

        let x_value = ctx.lookup_variable(x_name).expect("x not found");
        let y_value = ctx.lookup_variable(y_name).expect("y not found");

        // Both should be tuples.
        match (x_value, y_value) {
            (Value::Tuple { .. }, Value::Tuple { .. }) => {
                // Successfully cloned tuple value
            }
            _ => panic!("Expected Tuple values for x and y"),
        }
    }
}
