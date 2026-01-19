//! Expression generation using datalit's type-directed generation.

use rand::Rng;
use datalove_datalit::ast::TypeHintAndHeap;
use datalove_datalit::ast_gen;
use crate::config::WorldGenConfig;
use crate::context::{GenContext, FunctionSig, types_match};
use crate::gen_type::{gen_bool_type, gen_u32_type};
use crate::pretty::pretty_expr_with_heap;

/// Generate an expression matching the given type.
///
/// May be a literal, variable reference, or function call.
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

    if can_use_var && !can_call_fn {
        // Use a variable.
        let var = matching_vars[rng.gen_range(0..matching_vars.len())];
        var.name.clone()
    } else if can_call_fn && !can_use_var {
        // Call a function.
        let func = matching_fns[rng.gen_range(0..matching_fns.len())];
        gen_function_call(db, rng, func, config, ctx)
    } else if can_use_var && can_call_fn {
        // Choose randomly.
        if rng.gen_bool(0.5) {
            let var = matching_vars[rng.gen_range(0..matching_vars.len())];
            var.name.clone()
        } else {
            let func = matching_fns[rng.gen_range(0..matching_fns.len())];
            gen_function_call(db, rng, func, config, ctx)
        }
    } else {
        // Generate a literal using datalit.
        gen_literal(db, rng, type_hint, config)
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

/// Generate a boolean expression.
pub fn gen_bool_expr<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &GenContext<'db>,
) -> String {
    let bool_type = gen_bool_type(db);

    // Check for bool variables.
    let bool_vars = ctx.variables_of_type(db, bool_type);
    let has_bool_var = !bool_vars.is_empty();

    let choice = rng.gen_range(0..10);
    match choice {
        0..=3 => {
            // Simple literal.
            if rng.gen_bool(0.5) { "true".to_string() } else { "false".to_string() }
        }
        4..=5 if has_bool_var => {
            // Variable reference.
            let var = bool_vars[rng.gen_range(0..bool_vars.len())];
            var.name.clone()
        }
        6..=8 => {
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
            // All expressions should start with @ for local heap.
            assert!(
                expr.starts_with("@"),
                "Expression should start with @: {}",
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
