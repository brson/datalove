//! Expression generation using datalit's type-directed generation.

use rand::Rng;
use datalove_datalit::ast::TypeHint;
use datalove_datalit::ast_gen;
use crate::config::WorldGenConfig;
use crate::context::{GenContext, FunctionSig, Param, ParamMode, is_linear_type, types_match};
use crate::gen_type::{gen_bool_type, gen_u32_type};
use crate::pretty::{pretty_expr, pretty_type_hint};

/// Check if a type supports bare arithmetic operators.
///
/// Floats and bigints support bare arithmetic.
/// Float literals need type hints to avoid f32/f64 inference issues.
fn supports_bare_arithmetic(type_hint: TypeHint<'_>) -> bool {
    matches!(type_hint, TypeHint::F32 | TypeHint::F64 | TypeHint::Int)
}

/// Whether a type is one of the ten fixed-width integers.
fn is_fixed_int(type_hint: &TypeHint<'_>) -> bool {
    matches!(
        type_hint,
        TypeHint::U8 | TypeHint::I8 | TypeHint::U16 | TypeHint::I16
            | TypeHint::U32 | TypeHint::I32 | TypeHint::U64 | TypeHint::I64
            | TypeHint::Index | TypeHint::Offset
    )
}

/// Whether a fixed-width integer is signed, which unary `-!` and `-?` need.
fn is_signed_fixed_int(type_hint: &TypeHint<'_>) -> bool {
    matches!(
        type_hint,
        TypeHint::I8 | TypeHint::I16 | TypeHint::I32 | TypeHint::I64 | TypeHint::Offset
    )
}

/// Which of the two overflow-handling forms a body may write, if either.
///
/// A fixed integer has no bare arithmetic. What it has is checked -- `+!`,
/// which early-returns an error where it overflows -- and optional -- `+?`,
/// which early-returns `none`. Each is an early return of its own shape, so
/// which one is available is decided by what the function it sits in returns,
/// and in a script fragment, which returns nothing, neither is.
fn overflow_form(return_type: &Option<TypeHint<'_>>) -> Option<&'static str> {
    match return_type {
        Some(TypeHint::Result(_)) => Some("!"),
        Some(TypeHint::Option(_)) => Some("?"),
        _ => Option::None,
    }
}

/// Check if a type supports unary negation.
///
/// Only floats and bigints support bare unary `-`.
/// Fixed ints do NOT support unary negation (use `-?` or `-!` instead).
fn supports_unary_negation(type_hint: &TypeHint<'_>) -> bool {
    matches!(type_hint, TypeHint::F32 | TypeHint::F64 | TypeHint::Int)
}

/// Generate an expression matching the given type.
///
/// May be a literal, variable reference, function call, or arithmetic expression.
/// When a variable is used it gets marked as consumed because ownership
/// is transferred (moved).
pub fn gen_expr<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    type_hint: TypeHint<'db>,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
) -> String {
    // What could stand here, each rolled for on its own.
    let has_matching_vars = !ctx.variables_of_type(db, type_hint.clone()).is_empty();
    let can_use_var = has_matching_vars && rng.gen_bool(0.4);

    // Clone the matching functions to avoid borrow issues.
    let type_hint_for_filter = type_hint.clone();
    let matching_fns: Vec<FunctionSig<'db>> = ctx.callable_functions()
        .filter(|f| {
            if let Some(ret_type) = &f.return_type {
                types_match(db, ret_type.clone(), type_hint_for_filter.clone())
            } else {
                false
            }
        })
        // And whose parameters this body can supply. A `ref`, a `mut` or an
        // `out` wants a binding the caller already has.
        .filter(|f| can_call(db, f, ctx))
        .cloned()
        .collect();
    let can_call_fn = !matching_fns.is_empty()
        && config.check_probability(rng, config.function_call_probability);

    let can_arith = supports_bare_arithmetic(type_hint.clone())
        && config.check_probability(rng, config.arithmetic_probability);

    // A fixed integer has no bare arithmetic. What it has is the checked kind
    // and the optional kind, each of which early-returns through the enclosing
    // function, so which one is available -- if either -- is decided by what
    // that function returns.
    let overflow_mark = if is_fixed_int(&type_hint)
        && config.check_probability(rng, config.arithmetic_probability)
    {
        overflow_form(&ctx.return_type)
    } else {
        Option::None
    };

    // A field of a struct or a tuple already in scope. Only a copy field can
    // be projected -- taking a heap one out would move it, and the typechecker
    // says so -- which is the whole of the rule.
    let projections = if is_linear_type(&type_hint) {
        Vec::new()
    } else {
        projection_candidates(db, &type_hint, ctx)
    };

    // An element of a collection already in scope, by the same copy rule, and
    // the same early return as the overflow arithmetic: `l[i]?` leaves through
    // the function's return where the index is out of bounds.
    //
    // What decides the mark here is only what the function returns -- unlike
    // the arithmetic above, which also wants a fixed integer.
    let early_return_mark = overflow_form(&ctx.return_type);
    let index_candidates = match (is_linear_type(&type_hint), early_return_mark) {
        (false, Some(_)) => index_candidates(db, &type_hint, ctx),
        _ => Vec::new(),
    };

    // Something in scope that has to be unwrapped to get at what is inside it,
    // which early-returns through the function the same way. `v?` moves out of
    // `v`, so whatever is picked is consumed.
    let unwrappable = match early_return_mark {
        Some(mark) => unwrap_candidates(db, &type_hint, mark, ctx),
        Option::None => Vec::new(),
    };

    enum Choice {
        Variable,
        Call,
        Arithmetic,
        OverflowArithmetic,
        Projection,
        Index,
        Unwrap,
    }

    let mut choices = Vec::new();
    if can_use_var { choices.push(Choice::Variable); }
    if can_call_fn { choices.push(Choice::Call); }
    if can_arith { choices.push(Choice::Arithmetic); }
    if overflow_mark.is_some() { choices.push(Choice::OverflowArithmetic); }
    if !projections.is_empty() && config.check_probability(rng, config.projection_probability) {
        choices.push(Choice::Projection);
    }
    if !index_candidates.is_empty() && config.check_probability(rng, config.projection_probability) {
        choices.push(Choice::Index);
    }
    if !unwrappable.is_empty() && config.check_probability(rng, config.projection_probability) {
        choices.push(Choice::Unwrap);
    }

    if choices.is_empty() {
        return gen_literal(db, rng, type_hint, config);
    }

    match choices.remove(rng.gen_range(0..choices.len())) {
        Choice::Variable => {
            let matching_vars = ctx.variables_of_type(db, type_hint.clone());
            if matching_vars.is_empty() {
                return gen_literal(db, rng, type_hint, config);
            }
            let var_name = matching_vars[rng.gen_range(0..matching_vars.len())].name.clone();
            ctx.consume_variable(&var_name);
            var_name
        }
        Choice::Call => {
            let func = &matching_fns[rng.gen_range(0..matching_fns.len())];
            gen_function_call(db, rng, func, config, ctx)
        }
        Choice::Arithmetic => gen_arithmetic_expr(db, rng, type_hint, config, ctx),
        Choice::OverflowArithmetic => {
            let mark = overflow_mark.expect("a choice only offered when there is a mark");
            gen_overflow_arith_expr(db, rng, type_hint, mark, config, ctx)
        }
        Choice::Projection => {
            projections[rng.gen_range(0..projections.len())].clone()
        }
        Choice::Index => {
            let mark = early_return_mark.expect("a choice only offered when there is a mark");
            let (container, key) =
                index_candidates[rng.gen_range(0..index_candidates.len())].clone();
            format!("{}[{}]{}", container, key, mark)
        }
        Choice::Unwrap => {
            let mark = early_return_mark.expect("a choice only offered when there is a mark");
            let name = unwrappable[rng.gen_range(0..unwrappable.len())].clone();
            ctx.consume_variable(&name);
            format!("{}{}", name, mark)
        }
    }
}

/// Every binding in scope that `?` or `!` would take the wanted type out of.
///
/// Which of the two is decided by what the enclosing function returns, and it
/// has to be the matching one: `?` unwraps an option and leaves through a
/// `none`, `!` unwraps a result and leaves through an error.
fn unwrap_candidates<'db>(
    db: &'db dyn salsa::Database,
    wanted: &TypeHint<'db>,
    mark: &str,
    ctx: &GenContext<'db>,
) -> Vec<String> {
    ctx.variables
        .iter()
        .filter(|v| !ctx.is_consumed(&v.name) && !ctx.is_loop_protected(&v.name))
        .filter_map(|v| match (&v.type_hint, mark) {
            (TypeHint::Option(o), "?") => Some((v.name.clone(), (*o.inner_type).clone())),
            (TypeHint::Result(r), "!") => Some((v.name.clone(), (*r.inner_type).clone())),
            _ => Option::None,
        })
        .filter(|(_, inner)| types_match(db, inner.clone(), wanted.clone()))
        .map(|(name, _)| name)
        .collect()
}

/// Every `v.field` in scope that has the wanted type.
///
/// Written out rather than picked from, because whether a field has the type
/// wanted is only known by looking at every field of every struct and tuple in
/// reach, and there are few enough of each that listing them is the clearest
/// way to say it.
fn projection_candidates<'db>(
    db: &'db dyn salsa::Database,
    wanted: &TypeHint<'db>,
    ctx: &GenContext<'db>,
) -> Vec<String> {
    let mut found = Vec::new();
    for var in ctx.variables.iter().filter(|v| !ctx.is_consumed(&v.name)) {
        match &var.type_hint {
            TypeHint::AnonTuple(t) => {
                for (i, field) in t.fields.iter().enumerate() {
                    if types_match(db, field.clone(), wanted.clone()) {
                        found.push(format!("{}.{}", var.name, i));
                    }
                }
            }
            TypeHint::AnonStruct(t) => {
                for field in t.fields.iter() {
                    if types_match(db, (*field.type_hint).clone(), wanted.clone()) {
                        found.push(format!("{}.{}", var.name, field.name.text(db)));
                    }
                }
            }
            _ => {}
        }
    }
    found
}

/// Every collection in scope holding the wanted type, with something to look
/// it up by.
///
/// A list and a tensor are indexed by an `index`, which may be past the end --
/// that is the point of the early return. A map is indexed by a key, and this
/// only offers one where the key type is a copy type, so that writing the key
/// costs nothing and moves nothing.
fn index_candidates<'db>(
    db: &'db dyn salsa::Database,
    wanted: &TypeHint<'db>,
    ctx: &GenContext<'db>,
) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for var in ctx.variables.iter().filter(|v| !ctx.is_consumed(&v.name)) {
        let (element, key) = match &var.type_hint {
            TypeHint::List(t) => ((*t.element_type).clone(), Option::None),
            TypeHint::Tensor(t) => ((*t.element_type).clone(), Option::None),
            TypeHint::Map(t) => ((*t.value_type).clone(), Some((*t.key_type).clone())),
            _ => continue,
        };
        if !types_match(db, element, wanted.clone()) {
            continue;
        }
        match key {
            // An index past the end is as interesting as one inside it.
            Option::None => found.push((var.name.clone(), ": index / 0".to_string())),
            Some(key_ty) if !is_linear_type(&key_ty) => {
                let written = pretty_type_hint(db, key_ty.clone());
                found.push((var.name.clone(), format!(": {} / 0", written)));
            }
            Some(_) => {}
        }
    }
    found
}

/// Generate an arithmetic expression (binary or unary).
///
/// Supports floats (f32, f64) and bigints (int).
/// Division only works for floats; bigints must use /! or /?.
/// Unary negation works for floats and bigints.
///
/// For bigints, may use existing variables as operands since operators
/// use ref semantics (borrow, not move).
fn gen_arithmetic_expr<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    type_hint: TypeHint<'db>,
    config: &WorldGenConfig,
    ctx: &GenContext<'db>,
) -> String {
    let is_float = matches!(type_hint, TypeHint::F32 | TypeHint::F64);
    let is_bigint = matches!(type_hint, TypeHint::Int);

    // 20% chance to generate unary negation instead of binary op.
    if supports_unary_negation(&type_hint) && rng.gen_bool(0.2) {
        return gen_unary_negation(db, rng, type_hint, config, ctx);
    }

    // Floats support all four operators; bigints don't support bare /.
    let ops: &[&str] = if is_float {
        &["+", "-", "*", "/"]
    } else {
        &["+", "-", "*"]
    };
    let op = ops[rng.gen_range(0..ops.len())];

    // Generate operands - for bigints, may use variables (operators borrow).
    let lhs = gen_arith_operand(db, rng, type_hint.clone(), config, ctx, is_bigint);
    let rhs = gen_arith_operand(db, rng, type_hint.clone(), config, ctx, is_bigint);

    if is_float || is_bigint {
        // Float and bigint literals need type hints to avoid inference issues.
        // Variables already have known types, so no hint needed.
        let type_str = pretty_type_hint(db, type_hint.clone());
        let lhs_str = match lhs {
            ArithOperand::Literal(s) => format!("(: {} / {})", type_str, s),
            ArithOperand::Variable(s) => s,
        };
        let rhs_str = match rhs {
            ArithOperand::Literal(s) => format!("(: {} / {})", type_str, s),
            ArithOperand::Variable(s) => s,
        };
        format!("{} {} {}", lhs_str, op, rhs_str)
    } else {
        let lhs_str = match lhs {
            ArithOperand::Literal(s) | ArithOperand::Variable(s) => s,
        };
        let rhs_str = match rhs {
            ArithOperand::Literal(s) | ArithOperand::Variable(s) => s,
        };
        format!("{} {} {}", lhs_str, op, rhs_str)
    }
}

/// Write a checked or optional arithmetic expression on a fixed integer.
///
/// `mark` is `!` or `?`. Both forms have the type of their operands -- the
/// overflow leaves through the function's return rather than through the
/// expression -- so this answers for the type it was asked for.
fn gen_overflow_arith_expr<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    type_hint: TypeHint<'db>,
    mark: &str,
    config: &WorldGenConfig,
    ctx: &GenContext<'db>,
) -> String {
    // Unary negation is for the signed ones only.
    if is_signed_fixed_int(&type_hint) && rng.gen_bool(0.35) {
        let operand = gen_overflow_operand(db, rng, type_hint, config, ctx);
        return format!("-{}{}", mark, operand);
    }

    let op = ["+", "-", "*", "/"][rng.gen_range(0..4)];
    let lhs = gen_overflow_operand(db, rng, type_hint.clone(), config, ctx);
    let rhs = gen_overflow_operand(db, rng, type_hint, config, ctx);
    format!("{} {}{} {}", lhs, op, mark, rhs)
}

/// An operand for the above, parenthesized so it can sit beside an operator.
///
/// A variable where there is one of the right type, since a fixed integer is
/// copied rather than moved and using one costs nothing.
fn gen_overflow_operand<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    type_hint: TypeHint<'db>,
    config: &WorldGenConfig,
    ctx: &GenContext<'db>,
) -> String {
    let borrowable = ctx.variables_of_type_for_borrow(db, type_hint.clone());
    if !borrowable.is_empty() && rng.gen_bool(0.4) {
        return borrowable[rng.gen_range(0..borrowable.len())].name.clone();
    }
    let type_str = pretty_type_hint(db, type_hint.clone());
    let value = ast_gen::gen_expr_matching_type(db, rng, type_hint, &config.type_config, 0);
    format!("(: {} / {})", type_str, pretty_expr(db, value))
}

/// Arithmetic operand - either a literal or a variable name.
enum ArithOperand {
    Literal(String),
    Variable(String),
}

/// Generate an operand for arithmetic expressions.
///
/// For bigints, may use an existing variable (operators use ref semantics).
/// Returns whether the operand is a variable (which already has a known type).
fn gen_arith_operand<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    type_hint: TypeHint<'db>,
    config: &WorldGenConfig,
    ctx: &GenContext<'db>,
    allow_variables: bool,
) -> ArithOperand {
    if allow_variables {
        // For bigints, 40% chance to use an existing variable if available.
        let borrowable_vars = ctx.variables_of_type_for_borrow(db, type_hint.clone());
        if !borrowable_vars.is_empty() && rng.gen_bool(0.4) {
            let var = borrowable_vars[rng.gen_range(0..borrowable_vars.len())];
            return ArithOperand::Variable(var.name.clone());
        }
    }
    ArithOperand::Literal(gen_literal(db, rng, type_hint, config))
}

/// Generate a unary negation expression.
///
/// Only works for floats and bigints.
/// For bigints, may use an existing variable as operand.
fn gen_unary_negation<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    type_hint: TypeHint<'db>,
    config: &WorldGenConfig,
    ctx: &GenContext<'db>,
) -> String {
    let is_bigint = matches!(type_hint, TypeHint::Int);
    let operand = gen_arith_operand(db, rng, type_hint.clone(), config, ctx, is_bigint);

    // Literals need type hints for unary negation.
    // Variables already have known types, so no hint needed.
    let type_str = pretty_type_hint(db, type_hint);

    match operand {
        ArithOperand::Literal(s) => format!("-(: {} / {})", type_str, s),
        ArithOperand::Variable(s) => format!("-{}", s),
    }
}

/// Check if a type hint is a fixed-width integer type that needs a type hint.
///
/// With datafun's `int` synthesis for bare literals, fixed-width integers
/// need explicit type hints to ensure the correct type.
fn needs_integer_type_hint(type_hint: &TypeHint<'_>) -> bool {
    matches!(
        type_hint,
        TypeHint::U8 | TypeHint::I8 |
        TypeHint::U16 | TypeHint::I16 |
        TypeHint::U32 | TypeHint::I32 |
        TypeHint::U64 | TypeHint::I64 |
        TypeHint::Index | TypeHint::Offset
    )
}

/// Generate a literal expression matching the given type using datalit.
fn gen_literal<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    type_hint: TypeHint<'db>,
    config: &WorldGenConfig,
) -> String {
    let expr = ast_gen::gen_expr_matching_type(
        db,
        rng,
        type_hint.clone(),
        &config.type_config,
        0,
    );

    let expr_str = pretty_expr(db, expr);

    // Fixed-width integers need type hints because bare literals synthesize as `int`.
    if needs_integer_type_hint(&type_hint) {
        let type_str = pretty_type_hint(db, type_hint);
        format!(": {} / {}", type_str, expr_str)
    } else {
        expr_str
    }
}

/// Generate a function call expression.
/// Whether this call can be written from here.
///
/// An `in` parameter takes whatever the expression generator makes, so a
/// function of those is always callable. The other three want something the
/// caller already has: a `ref` wants a binding to borrow, and a `mut` or an
/// `out` wants a `var` to write through. Two of them wanting the same one is
/// refused as well, so each takes a different name.
pub fn can_call<'db>(
    db: &'db dyn salsa::Database,
    func: &FunctionSig<'db>,
    ctx: &GenContext<'db>,
) -> bool {
    let mut spoken_for: Vec<String> = Vec::new();
    for param in &func.params {
        if param.mode == ParamMode::In {
            continue;
        }
        let candidate = argument_candidates(db, param, ctx)
            .into_iter()
            .find(|name| !spoken_for.contains(name));
        match candidate {
            Some(name) => spoken_for.push(name),
            None => return false,
        }
    }
    true
}

/// The bindings a parameter of this mode would take, by name.
fn argument_candidates<'db>(
    db: &'db dyn salsa::Database,
    param: &Param<'db>,
    ctx: &GenContext<'db>,
) -> Vec<String> {
    ctx.variables
        .iter()
        .filter(|v| !ctx.is_consumed(&v.name))
        .filter(|v| !param.mode.wants_a_mutable_binding() || v.is_mutable)
        .filter(|v| types_match(db, v.type_hint.clone(), param.type_hint.clone()))
        .map(|v| v.name.clone())
        .collect()
}

fn gen_function_call<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    func: &FunctionSig<'db>,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
) -> String {
    // The borrowed and written ones are picked first, because which binding
    // each takes has to be settled before an `in` argument is allowed to
    // consume one of them.
    let mut spoken_for: Vec<(usize, String)> = Vec::new();
    for (i, param) in func.params.iter().enumerate() {
        if param.mode == ParamMode::In {
            continue;
        }
        let taken: Vec<String> = spoken_for.iter().map(|(_, n)| n.clone()).collect();
        let candidates: Vec<String> = argument_candidates(db, param, ctx)
            .into_iter()
            .filter(|name| !taken.contains(name))
            .collect();
        let name = candidates[rng.gen_range(0..candidates.len())].clone();
        spoken_for.push((i, name));
    }

    let args: Vec<String> = func
        .params
        .iter()
        .enumerate()
        .map(|(i, param)| match spoken_for.iter().find(|(j, _)| *j == i) {
            Some((_, name)) => format!("{}{}", param.mode.marker(), name),
            Option::None => gen_expr(db, rng, param.type_hint.clone(), config, ctx),
        })
        .collect();

    format!("{}({})", func.name, args.join(", "))
}

/// Generate an atomic boolean expression (literal or variable only).
///
/// Used as operand for `not` to avoid precedence issues.
/// Note: bool is a local heap type, so no ownership tracking needed.
fn gen_bool_atom<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    _rng: &mut R,
    _config: &WorldGenConfig,
    ctx: &GenContext<'db>,
) -> String {
    let bool_type = gen_bool_type(db);
    let bool_vars = ctx.variables_of_type(db, bool_type);

    if !bool_vars.is_empty() && _rng.gen_bool(0.3) {
        // Variable reference. Bool is local heap, no consume needed.
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
    ctx: &mut GenContext<'db>,
) -> String {
    let bool_type = gen_bool_type(db);
    let bool_vars = ctx.variables_of_type(db, bool_type.clone());
    let has_bool_var = !bool_vars.is_empty();

    let choice = rng.gen_range(0..10);
    match choice {
        0..=4 => {
            // Simple literal.
            if rng.gen_bool(0.5) { "true".to_string() } else { "false".to_string() }
        }
        5..=6 if has_bool_var => {
            // Variable reference.
            let bool_vars = ctx.variables_of_type(db, bool_type.clone());
            if bool_vars.is_empty() {
                if rng.gen_bool(0.5) { "true".to_string() } else { "false".to_string() }
            } else {
                let var = bool_vars[rng.gen_range(0..bool_vars.len())];
                var.name.clone()
            }
        }
        7..=9 => {
            // Comparison expression.
            let ty = gen_u32_type(db);
            let lhs = gen_expr(db, rng, ty.clone(), config, ctx);
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
    ctx: &mut GenContext<'db>,
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
    use datalove_datalit::ast::TypeHint;
    use datalove_datalit::Database;
    use crate::context::Variable;
    use rand::SeedableRng;

    #[test]
    fn test_gen_expr_literal_u32() {
        let db = Database::default();
        test_gen_expr_literal_u32_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_gen_expr_literal_u32_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let mut ctx = GenContext::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let ty = TypeHint::U32;
        let expr = gen_expr(db, &mut rng, ty, &config, &mut ctx);

        // Should produce a u32 literal with type hint (e.g., ": u32 / 42").
        assert!(expr.starts_with(": u32 / "), "Should have u32 type hint: {}", expr);
    }

    #[test]
    fn test_gen_expr_literal_bool() {
        let db = Database::default();
        test_gen_expr_literal_bool_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_gen_expr_literal_bool_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let mut ctx = GenContext::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let ty = TypeHint::Bool;
        let expr = gen_expr(db, &mut rng, ty, &config, &mut ctx);

        // Should produce true or false.
        assert!(
            expr == "true" || expr == "false",
            "bool expression should be true or false: {}",
            expr
        );
    }

    #[test]
    fn test_gen_expr_literal_string() {
        let db = Database::default();
        test_gen_expr_literal_string_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_gen_expr_literal_string_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let mut ctx = GenContext::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let ty = TypeHint::String;
        let expr = gen_expr(db, &mut rng, ty, &config, &mut ctx);

        // Should produce "..." string.
        assert!(expr.starts_with("\""), "string should start with \": {}", expr);
        assert!(expr.ends_with("\""), "string should end with \": {}", expr);
    }

    #[test]
    fn test_gen_expr_uses_variable() {
        let db = Database::default();
        test_gen_expr_uses_variable_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_gen_expr_uses_variable_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let ty = TypeHint::U32;

        let mut ctx = GenContext::new();
        ctx.variables.push(Variable {
            name: "my_var".to_string(),
            type_hint: ty.clone(),
            is_mutable: false,
        });

        // Generate many expressions - some should use the variable.
        let mut used_var = false;
        for seed in 0..100 {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let expr = gen_expr(db, &mut rng, ty.clone(), &config, &mut ctx);
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

    #[salsa::tracked(returns(copy))]
    fn test_gen_expr_function_call_inner<'db>(db: &'db dyn salsa::Database) {
        let mut config = WorldGenConfig::default();
        config.function_call_probability = 100; // Always call if available.

        let ret_ty = TypeHint::U32;
        let param_ty = TypeHint::Bool;

        let mut ctx = GenContext::new();
        ctx.functions.push(FunctionSig {
            name: "get_value".to_string(),
            params: vec![Param { name: "flag".to_string(), type_hint: param_ty, mode: ParamMode::In }],
            return_type: Some(ret_ty.clone()),
        });

        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let expr = gen_expr(db, &mut rng, ret_ty, &config, &mut ctx);

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

    #[salsa::tracked(returns(copy))]
    fn test_gen_bool_expr_literals_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let mut ctx = GenContext::new();

        let mut found_true = false;
        let mut found_false = false;

        for seed in 0..100 {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let expr = gen_bool_expr(db, &mut rng, &config, &mut ctx);
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

    #[salsa::tracked(returns(copy))]
    fn test_gen_bool_expr_comparison_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let mut ctx = GenContext::new();

        let mut found_comparison = false;

        for seed in 0..100 {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let expr = gen_bool_expr(db, &mut rng, &config, &mut ctx);
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

    #[salsa::tracked(returns(copy))]
    fn test_gen_bool_expr_logical_operators_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let mut ctx = GenContext::new();

        let mut found_not = false;
        let mut found_and = false;
        let mut found_or = false;
        let mut found_xor = false;

        for seed in 0..200 {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let expr = gen_bool_expr(db, &mut rng, &config, &mut ctx);
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

    #[salsa::tracked(returns(copy))]
    fn test_gen_expr_type_variety_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let mut ctx = GenContext::new();
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

        for ty in types {
            let expr = gen_expr(db, &mut rng, ty, &config, &mut ctx);
            // Expressions should not be empty.
            assert!(!expr.is_empty(), "Expression should not be empty");
        }
    }

    #[test]
    fn test_gen_expr_u32() {
        let db = Database::default();
        test_gen_expr_u32_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_gen_expr_u32_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let mut ctx = GenContext::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let ty = TypeHint::U32;
        let expr = gen_expr(db, &mut rng, ty, &config, &mut ctx);

        // Should produce a u32 literal with type hint (e.g., ": u32 / 42").
        assert!(expr.starts_with(": u32 / "), "Should have u32 type hint: {}", expr);
    }

    #[test]
    fn test_gen_expr_deterministic() {
        let db = Database::default();
        test_gen_expr_deterministic_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_gen_expr_deterministic_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let mut ctx = GenContext::new();

        let ty = TypeHint::U32;

        // Same seed should produce same expression.
        let mut rng1 = rand::rngs::StdRng::seed_from_u64(42);
        let mut rng2 = rand::rngs::StdRng::seed_from_u64(42);

        let expr1 = gen_expr(db, &mut rng1, ty.clone(), &config, &mut ctx);
        let expr2 = gen_expr(db, &mut rng2, ty, &config, &mut ctx);

        assert_eq!(expr1, expr2, "Same seed should produce same expression");
    }
}
