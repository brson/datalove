//! Expression generation.

use rand::Rng;
use crate::config::WorldGenConfig;
use crate::context::GenContext;
use crate::gen_type::gen_heap;

/// Generate a literal expression matching the given type.
pub fn gen_literal<R: Rng>(rng: &mut R, type_hint: &str, ctx: &GenContext) -> String {
    // Strip heap annotation if present.
    let ty = type_hint
        .trim_start_matches('@')
        .trim_start_matches('#');

    // Check if this is a type alias.
    if let Some(alias) = ctx.type_aliases.iter().find(|a| a.name == ty) {
        // Generate a value matching the underlying type.
        return gen_literal(rng, &alias.type_hint, ctx);
    }

    match ty {
        "bool" => if rng.gen_bool(0.5) { "true".to_string() } else { "false".to_string() },
        "u8" => format!("{}", rng.gen_range(0u8..=255)),
        "i8" => format!("{}", rng.gen_range(-128i8..=127)),
        "u16" => format!("{}", rng.gen_range(0u16..=1000)),
        "i16" => format!("{}", rng.gen_range(-1000i16..=1000)),
        "u32" => format!("{}", rng.gen_range(0u32..=10000)),
        "i32" => format!("{}", rng.gen_range(-10000i32..=10000)),
        "u64" => format!("{}", rng.gen_range(0u64..=100000)),
        "i64" => format!("{}", rng.gen_range(-100000i64..=100000)),
        "f32" => {
            // Must have exactly one decimal place and suffix to ensure f32.
            let val: f32 = rng.gen_range(-100.0f32..=100.0);
            format!("{:.2}", val)
        }
        "f64" => {
            // Must have decimal places for f64.
            let val: f64 = rng.gen_range(-100.0f64..=100.0);
            format!("{:.2}", val)
        }
        "string" => {
            let strings = ["\"hello\"", "\"world\"", "\"test\"", "\"data\"", "\"\""];
            strings[rng.gen_range(0..strings.len())].to_string()
        }
        "int" => format!("{}", rng.gen_range(-10000i64..=10000)),
        _ => {
            // Handle composite types.
            if ty.starts_with('[') && ty.ends_with(']') {
                // List type.
                let elem_ty = &ty[1..ty.len()-1];
                let count = rng.gen_range(0..=3);
                let elems: Vec<String> = (0..count)
                    .map(|_| gen_literal(rng, elem_ty, ctx))
                    .collect();
                format!("[{}]", elems.join(", "))
            } else if ty.starts_with('?') {
                // Option type.
                let inner_ty = &ty[1..];
                if rng.gen_bool(0.5) {
                    format!(": ?{} / none", inner_ty)
                } else {
                    format!("some {}", gen_literal(rng, inner_ty, ctx))
                }
            } else if ty.starts_with('!') {
                // Result type.
                let inner_ty = &ty[1..];
                format!("ok {}", gen_literal(rng, inner_ty, ctx))
            } else if ty.starts_with('(') && ty.ends_with(')') {
                // Tuple type.
                let inner = &ty[1..ty.len()-1];
                let fields = parse_type_list(inner);
                let elems: Vec<String> = fields
                    .iter()
                    .map(|f| gen_literal(rng, f, ctx))
                    .collect();
                format!("({})", elems.join(", "))
            } else if ty.starts_with('{') && ty.ends_with('}') {
                // Struct type.
                let inner = &ty[1..ty.len()-1];
                if inner.is_empty() {
                    "{}".to_string()
                } else {
                    let fields = parse_struct_fields(inner);
                    let elems: Vec<String> = fields
                        .iter()
                        .map(|(name, field_ty)| {
                            format!("{} = {}", name, gen_literal(rng, field_ty, ctx))
                        })
                        .collect();
                    format!("{{{}}}", elems.join(", "))
                }
            } else {
                // Unknown type (likely a type alias we can't resolve), return simple value.
                "0".to_string()
            }
        }
    }
}

/// Parse a comma-separated list of types, respecting nested brackets.
fn parse_type_list(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut depth = 0;

    for ch in s.chars() {
        match ch {
            '(' | '[' | '{' | '<' => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' | '}' | '>' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    result.push(trimmed);
                }
                current.clear();
            }
            _ => {
                current.push(ch);
            }
        }
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        result.push(trimmed);
    }

    result
}

/// Parse struct fields like "f0: u32, f1: string".
fn parse_struct_fields(s: &str) -> Vec<(String, String)> {
    let fields_list = parse_type_list(s);
    let mut result = Vec::new();

    for field in fields_list {
        if let Some((name, ty)) = field.split_once(": ") {
            result.push((name.trim().to_string(), ty.trim().to_string()));
        }
    }

    result
}

/// Generate an expression matching the given type.
///
/// May be a literal, variable reference, or function call.
pub fn gen_expr<R: Rng>(
    rng: &mut R,
    type_hint: &str,
    config: &WorldGenConfig,
    ctx: &GenContext,
) -> String {
    // Check if we can use a variable.
    let matching_vars = ctx.variables_of_type(type_hint);
    let can_use_var = !matching_vars.is_empty() && rng.gen_bool(0.4);

    // Check if we can call a function.
    let matching_fns: Vec<_> = ctx
        .callable_functions()
        .filter(|f| f.return_type.as_deref() == Some(type_hint))
        .collect();
    let can_call_fn = !matching_fns.is_empty()
        && rng.gen_bool(config.function_call_probability);

    if can_use_var && !can_call_fn {
        // Use a variable.
        let var = matching_vars[rng.gen_range(0..matching_vars.len())];
        var.name.clone()
    } else if can_call_fn && !can_use_var {
        // Call a function.
        let func = matching_fns[rng.gen_range(0..matching_fns.len())];
        gen_function_call(rng, func, config, ctx)
    } else if can_use_var && can_call_fn {
        // Choose randomly.
        if rng.gen_bool(0.5) {
            let var = matching_vars[rng.gen_range(0..matching_vars.len())];
            var.name.clone()
        } else {
            let func = matching_fns[rng.gen_range(0..matching_fns.len())];
            gen_function_call(rng, func, config, ctx)
        }
    } else {
        // Generate a literal with heap annotation.
        let heap = gen_heap(rng);
        format!("{}{}", heap, gen_literal(rng, type_hint, ctx))
    }
}

/// Generate a function call expression.
fn gen_function_call<R: Rng>(
    rng: &mut R,
    func: &crate::context::FunctionSig,
    config: &WorldGenConfig,
    ctx: &GenContext,
) -> String {
    let args: Vec<String> = func
        .params
        .iter()
        .map(|(_, param_type)| gen_expr(rng, param_type, config, ctx))
        .collect();

    format!("{}({})", func.name, args.join(", "))
}

/// Generate a boolean expression.
pub fn gen_bool_expr<R: Rng>(
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &GenContext,
) -> String {
    // Check for bool variables.
    let bool_vars = ctx.variables_of_type("@bool");
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
            let ty = "@u32";
            let lhs = gen_expr(rng, ty, config, ctx);
            let rhs = gen_expr(rng, ty, config, ctx);
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
