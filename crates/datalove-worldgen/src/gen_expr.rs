//! Expression generation using datalit's type-directed generation.

use rand::Rng;
use datalove_datalit::ast::{TypeHint, TypeHintAndHeap};
use datalove_datalit::ast_gen;
use crate::config::WorldGenConfig;
use crate::context::{GenContext, FunctionSig, types_match};
use crate::gen_type::{gen_bool_type, gen_u32_type};
use crate::pretty::{pretty_expr_with_heap, pretty_type_hint_and_heap};

/// Check if a type supports bare arithmetic operators.
///
/// Floats and bigints support bare arithmetic.
/// Float literals need type hints to avoid f32/f64 inference issues.
fn supports_bare_arithmetic(type_hint: TypeHint<'_>) -> bool {
    matches!(type_hint, TypeHint::F32 | TypeHint::F64 | TypeHint::Int)
}

/// Generate an expression matching the given type.
///
/// May be a literal, variable reference, function call, or arithmetic expression.
pub fn gen_expr<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    type_hint: TypeHintAndHeap<'db>,
    config: &WorldGenConfig,
    ctx: &GenContext<'db>,
) -> String {
    // Check if we can use a variable.
    let matching_vars = ctx.variables_of_type(db, type_hint);
    let can_use_var = !matching_vars.is_empty() && rng.gen_bool(0.4);

    // Check if we can call a function.
    let matching_fns: Vec<_> = ctx.callable_functions()
        .filter(|f| {
            if let Some(ret_type) = f.return_type {
                types_match(db, ret_type, type_hint)
            } else {
                false
            }
        })
        .collect();
    let can_call_fn = !matching_fns.is_empty()
        && config.check_probability(rng, config.function_call_probability);

    // Check if we can generate an arithmetic expression.
    let can_arith = supports_bare_arithmetic(type_hint.type_hint(db))
        && config.check_probability(rng, config.arithmetic_probability);

    if can_use_var && !can_call_fn && !can_arith {
        // Use a variable.
        let var = matching_vars[rng.gen_range(0..matching_vars.len())];
        var.name.clone()
    } else if can_call_fn && !can_use_var && !can_arith {
        // Call a function.
        let func = matching_fns[rng.gen_range(0..matching_fns.len())];
        gen_function_call(db, rng, func, config, ctx)
    } else if can_arith && !can_use_var && !can_call_fn {
        // Generate arithmetic expression.
        gen_arithmetic_expr(db, rng, type_hint, config)
    } else if can_use_var || can_call_fn || can_arith {
        // Multiple options available, choose randomly.
        let mut options = Vec::new();
        if can_use_var { options.push(0); }
        if can_call_fn { options.push(1); }
        if can_arith { options.push(2); }

        match options[rng.gen_range(0..options.len())] {
            0 => {
                let var = matching_vars[rng.gen_range(0..matching_vars.len())];
                var.name.clone()
            }
            1 => {
                let func = matching_fns[rng.gen_range(0..matching_fns.len())];
                gen_function_call(db, rng, func, config, ctx)
            }
            2 => gen_arithmetic_expr(db, rng, type_hint, config),
            _ => unreachable!(),
        }
    } else {
        // Generate a literal using datalit.
        gen_literal(db, rng, type_hint, config)
    }
}

/// Generate an arithmetic expression (operand op operand).
///
/// Supports floats (f32, f64) and bigints (int).
/// Division only works for floats; bigints must use /! or /?.
fn gen_arithmetic_expr<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    type_hint: TypeHintAndHeap<'db>,
    config: &WorldGenConfig,
) -> String {
    let inner_type = type_hint.type_hint(db);
    let is_float = matches!(inner_type, TypeHint::F32 | TypeHint::F64);

    // Floats support all four operators; bigints don't support bare /.
    let ops: &[&str] = if is_float {
        &["+", "-", "*", "/"]
    } else {
        &["+", "-", "*"]
    };
    let op = ops[rng.gen_range(0..ops.len())];

    // Generate literal operands.
    let lhs = gen_literal(db, rng, type_hint, config);
    let rhs = gen_literal(db, rng, type_hint, config);

    if is_float {
        // Float operands need type hints to avoid f32/f64 inference issues.
        // Syntax: (: @f64 / @123.4) + (: @f64 / @56.7)
        let type_str = pretty_type_hint_and_heap(db, type_hint);
        format!("(: {} / {}) {} (: {} / {})", type_str, lhs, op, type_str, rhs)
    } else {
        // Bigints don't need type hints.
        format!("{} {} {}", lhs, op, rhs)
    }
}

/// Generate a literal expression matching the given type using datalit.
fn gen_literal<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    type_hint: TypeHintAndHeap<'db>,
    config: &WorldGenConfig,
) -> String {
    let type_hint_inner = type_hint.type_hint(db);
    let heap = type_hint.heap(db);

    let (expr, expr_heap) = ast_gen::gen_expr_matching_type(
        db,
        rng,
        type_hint_inner,
        heap,
        &config.type_config,
        0,
    );

    pretty_expr_with_heap(db, expr, expr_heap)
}

/// Generate a function call expression.
fn gen_function_call<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    func: &FunctionSig<'db>,
    config: &WorldGenConfig,
    ctx: &GenContext<'db>,
) -> String {
    let args: Vec<String> = func
        .params
        .iter()
        .map(|(_, param_type)| gen_expr(db, rng, *param_type, config, ctx))
        .collect();

    format!("{}({})", func.name, args.join(", "))
}

/// Generate an atomic boolean expression (literal or variable only).
///
/// Used as operand for `not` to avoid precedence issues.
fn gen_bool_atom<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    _rng: &mut R,
    _config: &WorldGenConfig,
    ctx: &GenContext<'db>,
) -> String {
    let bool_type = gen_bool_type(db);
    let bool_vars = ctx.variables_of_type(db, bool_type);

    if !bool_vars.is_empty() && _rng.gen_bool(0.3) {
        // Variable reference.
        let var = bool_vars[_rng.gen_range(0..bool_vars.len())];
        var.name.clone()
    } else {
        // Literal.
        if _rng.gen_bool(0.5) { "true".to_string() } else { "false".to_string() }
    }
}

/// Generate a simple boolean expression (no logical operators).
///
/// Used as operands for logical operators to avoid deep recursion.
fn gen_simple_bool_expr<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &GenContext<'db>,
) -> String {
    let bool_type = gen_bool_type(db);
    let bool_vars = ctx.variables_of_type(db, bool_type);
    let has_bool_var = !bool_vars.is_empty();

    let choice = rng.gen_range(0..10);
    match choice {
        0..=4 => {
            // Simple literal.
            if rng.gen_bool(0.5) { "true".to_string() } else { "false".to_string() }
        }
        5..=6 if has_bool_var => {
            // Variable reference.
            let var = bool_vars[rng.gen_range(0..bool_vars.len())];
            var.name.clone()
        }
        7..=9 => {
            // Comparison expression.
            let ty = gen_u32_type(db);
            let lhs = gen_expr(db, rng, ty, config, ctx);
            let rhs = gen_expr(db, rng, ty, config, ctx);
            let ops = [".<", ".>", "<=", ">=", "==", "!="];
            let op = ops[rng.gen_range(0..ops.len())];
            format!("{} {} {}", lhs, op, rhs)
        }
        _ => {
            // Simple literal fallback.
            if rng.gen_bool(0.5) { "true".to_string() } else { "false".to_string() }
        }
    }
}

/// Generate a boolean expression.
///
/// May include logical operators (not, and, or, xor).
pub fn gen_bool_expr<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &GenContext<'db>,
) -> String {
    let choice = rng.gen_range(0..15);
    match choice {
        0..=7 => {
            // Simple expression (literal, variable, comparison).
            gen_simple_bool_expr(db, rng, config, ctx)
        }
        8..=9 => {
            // Unary not - use atom to avoid precedence issues.
            // `not` has higher precedence than comparison, so `not a < b` parses as `(not a) < b`.
            let operand = gen_bool_atom(db, rng, config, ctx);
            format!("not {}", operand)
        }
        10..=14 => {
            // Binary logical operator.
            // and/or/xor have lower precedence than comparison, so this is safe.
            let lhs = gen_simple_bool_expr(db, rng, config, ctx);
            let rhs = gen_simple_bool_expr(db, rng, config, ctx);
            let ops = ["and", "or", "xor"];
            let op = ops[rng.gen_range(0..ops.len())];
            format!("{} {} {}", lhs, op, rhs)
        }
        _ => {
            // Fallback.
            gen_simple_bool_expr(db, rng, config, ctx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datalove_datalit::ast::{Heap, TypeHint, TypeHintAndHeap};
    use datalove_datalit::Database;
    use crate::context::Variable;
    use rand::SeedableRng;

    #[test]
    fn test_gen_expr_literal_u32() {
        let db = Database::default();
        test_gen_expr_literal_u32_inner(&db);
    }

    #[salsa::tracked]
    fn test_gen_expr_literal_u32_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let ctx = GenContext::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let ty = TypeHintAndHeap::new(db, Heap::Local, TypeHint::U32);
        let expr = gen_expr(db, &mut rng, ty, &config, &ctx);

        // Should produce a u32 literal with @ prefix.
        assert!(expr.starts_with("@"), "u32 expression should start with @: {}", expr);
        // Should be parseable as a number (after stripping @).
        let num_str = &expr[1..];
        assert!(num_str.parse::<u32>().is_ok(), "Should be valid u32: {}", expr);
    }

    #[test]
    fn test_gen_expr_literal_bool() {
        let db = Database::default();
        test_gen_expr_literal_bool_inner(&db);
    }

    #[salsa::tracked]
    fn test_gen_expr_literal_bool_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let ctx = GenContext::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let ty = TypeHintAndHeap::new(db, Heap::Local, TypeHint::Bool);
        let expr = gen_expr(db, &mut rng, ty, &config, &ctx);

        // Should produce @true or @false.
        assert!(
            expr == "@true" || expr == "@false",
            "bool expression should be @true or @false: {}",
            expr
        );
    }

    #[test]
    fn test_gen_expr_literal_string() {
        let db = Database::default();
        test_gen_expr_literal_string_inner(&db);
    }

    #[salsa::tracked]
    fn test_gen_expr_literal_string_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let ctx = GenContext::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let ty = TypeHintAndHeap::new(db, Heap::Local, TypeHint::String);
        let expr = gen_expr(db, &mut rng, ty, &config, &ctx);

        // Should produce @"..." string.
        assert!(expr.starts_with("@\""), "string should start with @\": {}", expr);
        assert!(expr.ends_with("\""), "string should end with \": {}", expr);
    }

    #[test]
    fn test_gen_expr_uses_variable() {
        let db = Database::default();
        test_gen_expr_uses_variable_inner(&db);
    }

    #[salsa::tracked]
    fn test_gen_expr_uses_variable_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let ty = TypeHintAndHeap::new(db, Heap::Local, TypeHint::U32);

        let mut ctx = GenContext::new();
        ctx.variables.push(Variable {
            name: "my_var".to_string(),
            type_hint: ty,
            is_mutable: false,
        });

        // Generate many expressions - some should use the variable.
        let mut used_var = false;
        for seed in 0..100 {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let expr = gen_expr(db, &mut rng, ty, &config, &ctx);
            if expr == "my_var" {
                used_var = true;
                break;
            }
        }

        assert!(used_var, "Should use variable at least once in 100 attempts");
    }

    #[test]
    fn test_gen_expr_function_call() {
        let db = Database::default();
        test_gen_expr_function_call_inner(&db);
    }

    #[salsa::tracked]
    fn test_gen_expr_function_call_inner<'db>(db: &'db dyn salsa::Database) {
        let mut config = WorldGenConfig::default();
        config.function_call_probability = 100; // Always call if available.

        let ret_ty = TypeHintAndHeap::new(db, Heap::Local, TypeHint::U32);
        let param_ty = TypeHintAndHeap::new(db, Heap::Local, TypeHint::Bool);

        let mut ctx = GenContext::new();
        ctx.functions.push(FunctionSig {
            name: "get_value".to_string(),
            params: vec![("flag".to_string(), param_ty)],
            return_type: Some(ret_ty),
        });

        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let expr = gen_expr(db, &mut rng, ret_ty, &config, &ctx);

        // Should generate function call.
        assert!(
            expr.contains("get_value("),
            "Should generate function call: {}",
            expr
        );
    }

    #[test]
    fn test_gen_bool_expr_literals() {
        let db = Database::default();
        test_gen_bool_expr_literals_inner(&db);
    }

    #[salsa::tracked]
    fn test_gen_bool_expr_literals_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let ctx = GenContext::new();

        let mut found_true = false;
        let mut found_false = false;

        for seed in 0..100 {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let expr = gen_bool_expr(db, &mut rng, &config, &ctx);
            if expr == "true" {
                found_true = true;
            }
            if expr == "false" {
                found_false = true;
            }
        }

        assert!(found_true, "Should generate 'true' literal");
        assert!(found_false, "Should generate 'false' literal");
    }

    #[test]
    fn test_gen_bool_expr_comparison() {
        let db = Database::default();
        test_gen_bool_expr_comparison_inner(&db);
    }

    #[salsa::tracked]
    fn test_gen_bool_expr_comparison_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let ctx = GenContext::new();

        let mut found_comparison = false;

        for seed in 0..100 {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let expr = gen_bool_expr(db, &mut rng, &config, &ctx);
            if expr.contains(".<") || expr.contains(".>")
                || expr.contains("<=") || expr.contains(">=")
                || expr.contains("==") || expr.contains("!=")
            {
                found_comparison = true;
                break;
            }
        }

        assert!(found_comparison, "Should generate comparison expressions");
    }

    #[test]
    fn test_gen_bool_expr_logical_operators() {
        let db = Database::default();
        test_gen_bool_expr_logical_operators_inner(&db);
    }

    #[salsa::tracked]
    fn test_gen_bool_expr_logical_operators_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let ctx = GenContext::new();

        let mut found_not = false;
        let mut found_and = false;
        let mut found_or = false;
        let mut found_xor = false;

        for seed in 0..200 {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let expr = gen_bool_expr(db, &mut rng, &config, &ctx);
            if expr.starts_with("not ") {
                found_not = true;
            }
            if expr.contains(" and ") {
                found_and = true;
            }
            if expr.contains(" or ") {
                found_or = true;
            }
            if expr.contains(" xor ") {
                found_xor = true;
            }
        }

        assert!(found_not, "Should generate 'not' expressions");
        assert!(found_and, "Should generate 'and' expressions");
        assert!(found_or, "Should generate 'or' expressions");
        assert!(found_xor, "Should generate 'xor' expressions");
    }

    #[test]
    fn test_gen_expr_type_variety() {
        let db = Database::default();
        test_gen_expr_type_variety_inner(&db);
    }

    #[salsa::tracked]
    fn test_gen_expr_type_variety_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let ctx = GenContext::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        // Test various primitive types.
        let types = [
            TypeHint::Bool,
            TypeHint::U8,
            TypeHint::I8,
            TypeHint::U16,
            TypeHint::I16,
            TypeHint::U32,
            TypeHint::I32,
            TypeHint::U64,
            TypeHint::I64,
            TypeHint::F32,
            TypeHint::F64,
            TypeHint::String,
        ];

        for ty_hint in types {
            let ty = TypeHintAndHeap::new(db, Heap::Local, ty_hint);
            let expr = gen_expr(db, &mut rng, ty, &config, &ctx);
            // Expressions should start with @ for local heap, or (: for typed arithmetic.
            assert!(
                expr.starts_with("@") || expr.starts_with("(: @"),
                "Expression should start with @ or (: @: {}",
                expr
            );
        }
    }

    #[test]
    fn test_gen_expr_global_heap() {
        let db = Database::default();
        test_gen_expr_global_heap_inner(&db);
    }

    #[salsa::tracked]
    fn test_gen_expr_global_heap_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let ctx = GenContext::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let ty = TypeHintAndHeap::new(db, Heap::Global, TypeHint::U32);
        let expr = gen_expr(db, &mut rng, ty, &config, &ctx);

        // Should produce #... for global heap.
        assert!(expr.starts_with("#"), "Global heap should use #: {}", expr);
    }

    #[test]
    fn test_gen_expr_deterministic() {
        let db = Database::default();
        test_gen_expr_deterministic_inner(&db);
    }

    #[salsa::tracked]
    fn test_gen_expr_deterministic_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let ctx = GenContext::new();

        let ty = TypeHintAndHeap::new(db, Heap::Local, TypeHint::U32);

        // Same seed should produce same expression.
        let mut rng1 = rand::rngs::StdRng::seed_from_u64(42);
        let mut rng2 = rand::rngs::StdRng::seed_from_u64(42);

        let expr1 = gen_expr(db, &mut rng1, ty, &config, &ctx);
        let expr2 = gen_expr(db, &mut rng2, ty, &config, &ctx);

        assert_eq!(expr1, expr2, "Same seed should produce same expression");
    }
}
