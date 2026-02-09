use rmx::prelude::*;
use bct::text::InternedText;
use rand::{Rng, SeedableRng};

use crate::ast::*;

/// Configuration for AST generation.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct AstGenConfig {
    /// Maximum nesting depth for recursive types.
    pub max_depth: usize,

    /// Minimum number of elements in collections.
    pub min_collection_size: usize,

    /// Maximum number of elements in collections.
    pub max_collection_size: usize,

    /// Whether to include type hints in generated ExprFull.
    pub include_type_hints: bool,

    /// Relative weights for different type constructors.
    pub type_weights: TypeWeights,

    /// Numeric generation strategy.
    pub numeric_strategy: NumericStrategy,

    /// Tensor generation configuration.
    pub tensor_config: TensorConfig,
}

impl Default for AstGenConfig {
    fn default() -> Self {
        AstGenConfig {
            max_depth: 3,
            min_collection_size: 0,
            max_collection_size: 5,
            include_type_hints: true,
            type_weights: TypeWeights::default(),
            numeric_strategy: NumericStrategy::Mixed,
            tensor_config: TensorConfig::default(),
        }
    }
}

/// Relative weights for different type constructors.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct TypeWeights {
    pub bool_type: u32,
    pub u8_type: u32,
    pub i8_type: u32,
    pub u16_type: u32,
    pub i16_type: u32,
    pub u32_type: u32,
    pub i32_type: u32,
    pub u64_type: u32,
    pub i64_type: u32,
    pub usize_type: u32,
    pub isize_type: u32,
    pub f32_type: u32,
    pub f64_type: u32,
    pub int_type: u32,
    pub string_type: u32,
    pub list_type: u32,
    pub map_type: u32,
    pub set_type: u32,
    pub option_type: u32,
    pub result_type: u32,
    pub tensor_type: u32,
    pub anon_tuple_type: u32,
    pub named_tuple_type: u32,
    pub anon_struct_type: u32,
    pub named_struct_type: u32,
    pub anon_enum_type: u32,
    pub named_enum_type: u32,
    pub data_type: u32,
    pub error_type: u32,
}

impl Default for TypeWeights {
    fn default() -> Self {
        TypeWeights {
            bool_type: 10,
            u8_type: 3,
            i8_type: 3,
            u16_type: 3,
            i16_type: 3,
            u32_type: 10,
            i32_type: 10,
            u64_type: 5,
            i64_type: 5,
            usize_type: 5,
            isize_type: 5,
            f32_type: 5,
            f64_type: 5,
            int_type: 5,
            string_type: 10,
            list_type: 5,
            map_type: 3,
            set_type: 3,
            option_type: 4,
            result_type: 3,
            tensor_type: 2,
            anon_tuple_type: 4,
            named_tuple_type: 2,
            anon_struct_type: 3,
            named_struct_type: 2,
            anon_enum_type: 2,
            named_enum_type: 1,
            data_type: 2,
            error_type: 2,
        }
    }
}

impl TypeWeights {
    /// Create weights that only allow leaf types (no containers or structured types).
    pub fn leaf_only() -> Self {
        TypeWeights {
            bool_type: 10,
            u8_type: 5,
            i8_type: 5,
            u16_type: 5,
            i16_type: 5,
            u32_type: 10,
            i32_type: 10,
            u64_type: 5,
            i64_type: 5,
            usize_type: 5,
            isize_type: 5,
            f32_type: 5,
            f64_type: 5,
            int_type: 5,
            string_type: 10,
            list_type: 0,
            map_type: 0,
            set_type: 0,
            option_type: 0,
            result_type: 0,
            tensor_type: 0,
            anon_tuple_type: 0,
            named_tuple_type: 0,
            anon_struct_type: 0,
            named_struct_type: 0,
            anon_enum_type: 0,
            named_enum_type: 0,
            data_type: 0,
            error_type: 0,
        }
    }
}

/// Numeric generation strategy.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum NumericStrategy {
    /// Generate corner cases: min, max, zero, near-zero, near-limits.
    CornerCases,

    /// Generate random values across full range.
    Random,

    /// Mix of corner cases and random values (80% random, 20% corner).
    Mixed,

    /// Generate small non-negative values (0..=255) suitable for typechecking without hints.
    SmallNonNegative,
}

/// Tensor generation configuration.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct TensorConfig {
    /// Maximum tensor rank (number of dimensions).
    pub max_rank: u32,

    /// Maximum size per dimension.
    pub max_dim_size: u32,
}

impl Default for TensorConfig {
    fn default() -> Self {
        TensorConfig {
            max_rank: 3,
            max_dim_size: 5,
        }
    }
}

/// Generate a random identifier (for type names).
pub fn gen_identifier<R: Rng>(rng: &mut R) -> String {
    let prefixes = ["My", "Test", "Gen", "Data", "Value", "Item", "Element"];
    let suffixes = ["Type", "Data", "Value", "Struct", "Enum", ""];

    let prefix = prefixes[rng.gen_range(0..prefixes.len())];
    let suffix = suffixes[rng.gen_range(0..suffixes.len())];
    let num = rng.gen_range(0..100);

    format!("{}{}{}", prefix, suffix, num)
}

/// Field name base strings for struct field generation.
const FIELD_NAME_BASES: &[&str] = &["x", "y", "z", "name", "value", "data", "field", "item"];
/// Maximum unique field names possible: 8 bases × 10 numbers.
const MAX_UNIQUE_FIELD_NAMES: usize = FIELD_NAME_BASES.len() * 10;

/// Generate a random field name.
pub fn gen_field_name<R: Rng>(rng: &mut R) -> String {
    let name = FIELD_NAME_BASES[rng.gen_range(0..FIELD_NAME_BASES.len())];
    let num = rng.gen_range(0..10);

    format!("{}{}", name, num)
}

/// Generate a random TypeHint based on configuration.
pub fn gen_type_hint<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &AstGenConfig,
    depth: usize,
) -> TypeHint<'db> {
    let weights = if depth >= config.max_depth {
        // At max depth, use leaf types only but respect user's disabled types.
        let mut leaf = TypeWeights::leaf_only();
        // Zero out any types the user explicitly disabled.
        if config.type_weights.bool_type == 0 { leaf.bool_type = 0; }
        if config.type_weights.u8_type == 0 { leaf.u8_type = 0; }
        if config.type_weights.i8_type == 0 { leaf.i8_type = 0; }
        if config.type_weights.u16_type == 0 { leaf.u16_type = 0; }
        if config.type_weights.i16_type == 0 { leaf.i16_type = 0; }
        if config.type_weights.u32_type == 0 { leaf.u32_type = 0; }
        if config.type_weights.i32_type == 0 { leaf.i32_type = 0; }
        if config.type_weights.u64_type == 0 { leaf.u64_type = 0; }
        if config.type_weights.i64_type == 0 { leaf.i64_type = 0; }
        if config.type_weights.usize_type == 0 { leaf.usize_type = 0; }
        if config.type_weights.isize_type == 0 { leaf.isize_type = 0; }
        if config.type_weights.f32_type == 0 { leaf.f32_type = 0; }
        if config.type_weights.f64_type == 0 { leaf.f64_type = 0; }
        if config.type_weights.int_type == 0 { leaf.int_type = 0; }
        if config.type_weights.string_type == 0 { leaf.string_type = 0; }
        leaf
    } else {
        config.type_weights.clone()
    };

    // Build weighted choices.
    let mut choices = Vec::new();
    let mut add_choice = |weight: u32, idx: usize| {
        for _ in 0..weight {
            choices.push(idx);
        }
    };

    add_choice(weights.bool_type, 0);
    add_choice(weights.u8_type, 1);
    add_choice(weights.i8_type, 2);
    add_choice(weights.u16_type, 3);
    add_choice(weights.i16_type, 4);
    add_choice(weights.u32_type, 5);
    add_choice(weights.i32_type, 6);
    add_choice(weights.u64_type, 7);
    add_choice(weights.i64_type, 8);
    add_choice(weights.f32_type, 9);
    add_choice(weights.f64_type, 10);
    add_choice(weights.int_type, 11);
    add_choice(weights.string_type, 12);
    add_choice(weights.list_type, 13);
    add_choice(weights.map_type, 14);
    add_choice(weights.set_type, 15);
    add_choice(weights.option_type, 16);
    add_choice(weights.result_type, 17);
    add_choice(weights.tensor_type, 18);
    add_choice(weights.anon_tuple_type, 19);
    add_choice(weights.anon_struct_type, 20);
    add_choice(weights.anon_enum_type, 21);
    add_choice(weights.data_type, 22);
    add_choice(weights.error_type, 23);
    add_choice(weights.usize_type, 24);
    add_choice(weights.isize_type, 25);

    if choices.is_empty() {
        return TypeHint::Bool;
    }

    let choice = choices[rng.gen_range(0..choices.len())];

    match choice {
        0 => TypeHint::Bool,
        1 => TypeHint::U8,
        2 => TypeHint::I8,
        3 => TypeHint::U16,
        4 => TypeHint::I16,
        5 => TypeHint::U32,
        6 => TypeHint::I32,
        7 => TypeHint::U64,
        8 => TypeHint::I64,
        9 => TypeHint::F32,
        10 => TypeHint::F64,
        11 => TypeHint::Int,
        12 => TypeHint::String,
        13 => {
            let element_type = gen_type_hint(db, rng, config, depth + 1);
            TypeHint::List(TypeHintList { element_type: Box::new(element_type) })
        }
        14 => {
            let key_type = gen_type_hint(db, rng, config, depth + 1);
            let value_type = gen_type_hint(db, rng, config, depth + 1);
            TypeHint::Map(TypeHintMap { key_type: Box::new(key_type), value_type: Box::new(value_type) })
        }
        15 => {
            let element_type = gen_type_hint(db, rng, config, depth + 1);
            TypeHint::Set(TypeHintSet { element_type: Box::new(element_type) })
        }
        16 => {
            let inner_type = gen_type_hint(db, rng, config, depth + 1);
            TypeHint::Option(TypeHintOption { inner_type: Box::new(inner_type) })
        }
        17 => {
            let inner_type = gen_type_hint(db, rng, config, depth + 1);
            TypeHint::Result(TypeHintResult { inner_type: Box::new(inner_type) })
        }
        18 => {
            let element_type = gen_type_hint(db, rng, config, depth + 1);
            let rank = rng.gen_range(1..=config.tensor_config.max_rank);
            TypeHint::Tensor(TypeHintTensor { element_type: Box::new(element_type), rank })
        }
        19 => {
            let count = rng.gen_range(config.min_collection_size..=config.max_collection_size);
            let fields: Vec<_> = (0..count)
                .map(|_| gen_type_hint(db, rng, config, depth + 1))
                .collect();
            TypeHint::AnonTuple(TypeHintAnonTuple { fields })
        }
        20 => {
            // Cap count to available unique field names to prevent infinite loops.
            let max_count = config.max_collection_size.min(MAX_UNIQUE_FIELD_NAMES);
            let count = rng.gen_range(config.min_collection_size..=max_count);
            let mut used_names = std::collections::HashSet::new();
            let fields: Vec<_> = (0..count)
                .map(|_| {
                    // Generate unique field name.
                    let name = loop {
                        let candidate = gen_field_name(rng);
                        if used_names.insert(candidate.clone()) {
                            break InternedText::new(db, &candidate);
                        }
                    };
                    let type_hint = gen_type_hint(db, rng, config, depth + 1);
                    TypeHintNamedField { name, type_hint: Box::new(type_hint) }
                })
                .collect();
            TypeHint::AnonStruct(TypeHintAnonStruct { fields })
        }

        22 => TypeHint::Data,
        23 => TypeHint::Error,
        24 => TypeHint::Index,
        25 => TypeHint::Offset,
        _ => TypeHint::Bool,
    }
}

/// Generate a TypeHint with nested types (internal helper).
fn gen_type_hint_inner<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &AstGenConfig,
    depth: usize,
) -> TypeHint<'db> {
    let weights = if depth >= config.max_depth {
        let mut leaf = TypeWeights::leaf_only();
        if config.type_weights.bool_type == 0 { leaf.bool_type = 0; }
        if config.type_weights.u8_type == 0 { leaf.u8_type = 0; }
        if config.type_weights.i8_type == 0 { leaf.i8_type = 0; }
        if config.type_weights.u16_type == 0 { leaf.u16_type = 0; }
        if config.type_weights.i16_type == 0 { leaf.i16_type = 0; }
        if config.type_weights.u32_type == 0 { leaf.u32_type = 0; }
        if config.type_weights.i32_type == 0 { leaf.i32_type = 0; }
        if config.type_weights.u64_type == 0 { leaf.u64_type = 0; }
        if config.type_weights.i64_type == 0 { leaf.i64_type = 0; }
        if config.type_weights.usize_type == 0 { leaf.usize_type = 0; }
        if config.type_weights.isize_type == 0 { leaf.isize_type = 0; }
        if config.type_weights.f32_type == 0 { leaf.f32_type = 0; }
        if config.type_weights.f64_type == 0 { leaf.f64_type = 0; }
        if config.type_weights.int_type == 0 { leaf.int_type = 0; }
        if config.type_weights.string_type == 0 { leaf.string_type = 0; }
        leaf
    } else {
        config.type_weights.clone()
    };

    let mut choices = Vec::new();
    let mut add_choice = |weight: u32, idx: usize| {
        for _ in 0..weight {
            choices.push(idx);
        }
    };

    add_choice(weights.bool_type, 0);
    add_choice(weights.u8_type, 1);
    add_choice(weights.i8_type, 2);
    add_choice(weights.u16_type, 3);
    add_choice(weights.i16_type, 4);
    add_choice(weights.u32_type, 5);
    add_choice(weights.i32_type, 6);
    add_choice(weights.u64_type, 7);
    add_choice(weights.i64_type, 8);
    add_choice(weights.f32_type, 9);
    add_choice(weights.f64_type, 10);
    add_choice(weights.int_type, 11);
    add_choice(weights.string_type, 12);
    add_choice(weights.list_type, 13);
    add_choice(weights.map_type, 14);
    add_choice(weights.set_type, 15);
    add_choice(weights.option_type, 16);
    add_choice(weights.result_type, 17);
    add_choice(weights.tensor_type, 18);
    add_choice(weights.anon_tuple_type, 19);
    add_choice(weights.anon_struct_type, 20);
    add_choice(weights.anon_enum_type, 21);
    add_choice(weights.data_type, 22);
    add_choice(weights.error_type, 23);
    add_choice(weights.usize_type, 24);
    add_choice(weights.isize_type, 25);

    if choices.is_empty() {
        return TypeHint::Bool;
    }

    let choice = choices[rng.gen_range(0..choices.len())];

    match choice {
        0 => TypeHint::Bool,
        1 => TypeHint::U8,
        2 => TypeHint::I8,
        3 => TypeHint::U16,
        4 => TypeHint::I16,
        5 => TypeHint::U32,
        6 => TypeHint::I32,
        7 => TypeHint::U64,
        8 => TypeHint::I64,
        9 => TypeHint::F32,
        10 => TypeHint::F64,
        11 => TypeHint::Int,
        12 => TypeHint::String,
        13 => {
            let element_type = gen_type_hint_inner(db, rng, config, depth + 1);
            TypeHint::List(TypeHintList { element_type: Box::new(element_type) })
        }
        14 => {
            let key_type = gen_type_hint_inner(db, rng, config, depth + 1);
            let value_type = gen_type_hint_inner(db, rng, config, depth + 1);
            TypeHint::Map(TypeHintMap { key_type: Box::new(key_type), value_type: Box::new(value_type) })
        }
        15 => {
            let element_type = gen_type_hint_inner(db, rng, config, depth + 1);
            TypeHint::Set(TypeHintSet { element_type: Box::new(element_type) })
        }
        16 => {
            let inner_type = gen_type_hint_inner(db, rng, config, depth + 1);
            TypeHint::Option(TypeHintOption { inner_type: Box::new(inner_type) })
        }
        17 => {
            let inner_type = gen_type_hint_inner(db, rng, config, depth + 1);
            TypeHint::Result(TypeHintResult { inner_type: Box::new(inner_type) })
        }
        18 => {
            let element_type = gen_type_hint_inner(db, rng, config, depth + 1);
            let rank = rng.gen_range(1..=config.tensor_config.max_rank);
            TypeHint::Tensor(TypeHintTensor { element_type: Box::new(element_type), rank })
        }
        19 => {
            let count = rng.gen_range(config.min_collection_size..=config.max_collection_size);
            let fields: Vec<_> = (0..count)
                .map(|_| gen_type_hint_inner(db, rng, config, depth + 1))
                .collect();
            TypeHint::AnonTuple(TypeHintAnonTuple { fields })
        }
        20 => {
            // Cap count to available unique field names to prevent infinite loops.
            let max_count = config.max_collection_size.min(MAX_UNIQUE_FIELD_NAMES);
            let count = rng.gen_range(config.min_collection_size..=max_count);
            let mut used_names = std::collections::HashSet::new();
            let fields: Vec<_> = (0..count)
                .map(|_| {
                    // Generate unique field name.
                    let name = loop {
                        let candidate = gen_field_name(rng);
                        if used_names.insert(candidate.clone()) {
                            break InternedText::new(db, &candidate);
                        }
                    };
                    let type_hint = gen_type_hint_inner(db, rng, config, depth + 1);
                    TypeHintNamedField { name, type_hint: Box::new(type_hint) }
                })
                .collect();
            TypeHint::AnonStruct(TypeHintAnonStruct { fields })
        }
        21 => TypeHint::Data,
        23 => TypeHint::Error,
        24 => TypeHint::Index,
        25 => TypeHint::Offset,
        _ => TypeHint::Bool,
    }
}

/// Generate a value matching the given type hint.
pub fn gen_expr_matching_type<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    type_hint: TypeHint<'db>,
    config: &AstGenConfig,
    depth: usize,
) -> Expr<'db> {
    match type_hint {
        TypeHint::Bool => {
            if rng.gen_bool(0.5) {
                Expr::True
            } else {
                Expr::False
            }
        }
        TypeHint::U8 => gen_uint_expr(db, rng, config, 0u8, u8::MAX),
        TypeHint::I8 => gen_int_expr(db, rng, config, i8::MIN, i8::MAX),
        TypeHint::U16 => gen_uint_expr(db, rng, config, 0u16, u16::MAX),
        TypeHint::I16 => gen_int_expr(db, rng, config, i16::MIN, i16::MAX),
        TypeHint::U32 => gen_uint_expr(db, rng, config, 0u32, u32::MAX),
        TypeHint::I32 => gen_int_expr(db, rng, config, i32::MIN, i32::MAX),
        TypeHint::U64 => gen_uint_expr(db, rng, config, 0u64, u64::MAX),
        TypeHint::I64 => gen_int_expr(db, rng, config, i64::MIN, i64::MAX),
        // For usize/isize, generate 32-bit values (default configuration).
        TypeHint::Index => gen_uint_expr(db, rng, config, 0u32, u32::MAX),
        TypeHint::Offset => gen_int_expr(db, rng, config, i32::MIN, i32::MAX),
        TypeHint::F32 => gen_f32_expr(db, rng, config),
        TypeHint::F64 => gen_f64_expr(db, rng, config),
        TypeHint::Int => gen_int_expr(db, rng, config, i64::MIN, i64::MAX),
        TypeHint::String => gen_string_expr(db, rng),
        TypeHint::AnonTuple(th) => {
            let elements: Vec<_> = th
                .fields
                .iter()
                .map(|field_type| {
                    gen_expr_full_inner(db, rng, field_type.clone(), config, depth + 1)
                })
                .collect();
            Expr::AnonTuple(ExprAnonTuple { elements })
        }
        TypeHint::AnonStruct(th) => {
            let fields: Vec<_> = th
                .fields
                .iter()
                .map(|field| {
                    let name = field.name;
                    let value = gen_expr_full_inner(db, rng, *field.type_hint.clone(), config, depth + 1);
                    ExprStructField { name, value }
                })
                .collect();
            Expr::AnonStruct(ExprAnonStruct { fields })
        }

        TypeHint::List(th) => {
            let element_type = *th.element_type.clone();
            let count = rng.gen_range(config.min_collection_size..=config.max_collection_size);
            let elements: Vec<_> = (0..count)
                .map(|_| gen_expr_full_inner(db, rng, element_type.clone(), config, depth + 1))
                .collect();
            Expr::List(ExprList { elements })
        }
        TypeHint::Map(th) => {
            let key_type = *th.key_type.clone();
            let value_type = *th.value_type.clone();
            let count = rng.gen_range(config.min_collection_size..=config.max_collection_size);
            let entries: Vec<_> = (0..count)
                .map(|_| {
                    let key = gen_expr_full_inner(db, rng, key_type.clone(), config, depth + 1);
                    let value = gen_expr_full_inner(db, rng, value_type.clone(), config, depth + 1);
                    ExprMapEntry { key, value }
                })
                .collect();
            Expr::Map(ExprMap { entries })
        }
        TypeHint::Set(th) => {
            let element_type = *th.element_type.clone();
            let count = rng.gen_range(config.min_collection_size..=config.max_collection_size);
            let elements: Vec<_> = (0..count)
                .map(|_| gen_expr_full_inner(db, rng, element_type.clone(), config, depth + 1))
                .collect();
            Expr::Set(ExprSet { elements })
        }
        TypeHint::Option(th) => {
            let inner_type = *th.inner_type.clone();
            if rng.gen_bool(0.5) {
                Expr::None
            } else {
                let payload = gen_expr_full_inner(db, rng, inner_type, config, depth + 1);
                Expr::Some(ExprSome { payload })
            }
        }
        TypeHint::Result(th) => {
            let inner_type = *th.inner_type.clone();
            if rng.gen_bool(0.5) {
                let payload = gen_expr_full_inner(db, rng, inner_type, config, depth + 1);
                Expr::Ok(ExprOk { payload })
            } else {
                // Error case: generate Expr::Er with Expr::Error payload.
                let error_inner_type = gen_type_hint_inner(db, rng, config, depth + 1);
                let error_inner_value = gen_expr_full_inner(db, rng, error_inner_type, config, depth + 1);
                let error_expr = Expr::Error(ExprError { value: error_inner_value });
                let er_payload = ExprFull::new(db, None, error_expr);
                Expr::Er(ExprEr { payload: er_payload })
            }
        }
        TypeHint::Tensor(th) => {
            let element_type = *th.element_type.clone();
            let rank = th.rank;
            // Generate shape ensuring total elements don't exceed max_collection_size.
            let mut shape: Vec<u32> = Vec::with_capacity(rank as usize);
            let mut total_elements = 1usize;
            for i in 0..rank {
                // Calculate max dimension size respecting both tensor config and collection budget.
                let budget_dim = config.max_collection_size / total_elements.max(1);
                let max_dim = budget_dim.min(config.tensor_config.max_dim_size as usize);
                let dim_size = rng.gen_range(1..=max_dim.max(1)) as u32;
                shape.push(dim_size);
                total_elements *= dim_size as usize;
                // If we've hit the limit, make remaining dimensions size 1.
                if total_elements >= config.max_collection_size {
                    for _ in (i + 1)..rank {
                        shape.push(1);
                    }
                    total_elements = shape.iter().map(|&d| d as usize).product();
                    break;
                }
            }
            // Generate exactly the number of elements dictated by the shape.
            let elements: Vec<_> = (0..total_elements)
                .map(|_| gen_expr_full_inner(db, rng, element_type.clone(), config, depth + 1))
                .collect();
            Expr::Tensor(ExprTensor { shape, elements })
        }
        TypeHint::Data => {
            let inner_type = gen_type_hint_inner(db, rng, config, depth + 1);
            let value = gen_expr_full_inner(db, rng, inner_type, config, depth + 1);
            Expr::Data(ExprData { value })
        }
        TypeHint::Error => {
            let inner_type = gen_type_hint_inner(db, rng, config, depth + 1);
            let value = gen_expr_full_inner(db, rng, inner_type, config, depth + 1);
            Expr::Error(ExprError { value })
        }
        TypeHint::ParseError(_) => Expr::None,
        TypeHint::Table(_) => todo!("table expression generation not yet implemented"),
        TypeHint::Alias(_) => Expr::None,
        TypeHint::Atom(_) | TypeHint::Term(_) | TypeHint::Enum(_) => {
            todo!("atom/term/enum expression generation not yet implemented")
        }
    }
}

/// Generate an unsigned integer expression.
fn gen_uint_expr<'db, R: Rng, T>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &AstGenConfig,
    min: T,
    max: T,
) -> Expr<'db>
where
    T: std::fmt::Display + num_traits::Bounded + num_traits::One + std::ops::Sub<Output = T> + Copy + rand::distributions::uniform::SampleUniform + PartialOrd + TryFrom<u8>,
{
    let value_str = match &config.numeric_strategy {
        NumericStrategy::CornerCases => {
            let choices = vec![
                min.to_string(),
                T::one().to_string(),
                (max - T::one()).to_string(),
                max.to_string(),
            ];
            choices[rng.gen_range(0..choices.len())].clone()
        }
        NumericStrategy::Random => {
            rng.gen_range(min..=max).to_string()
        }
        NumericStrategy::Mixed => {
            if rng.gen_bool(0.2) {
                let choices = vec![
                    min.to_string(),
                    T::one().to_string(),
                    (max - T::one()).to_string(),
                    max.to_string(),
                ];
                choices[rng.gen_range(0..choices.len())].clone()
            } else {
                rng.gen_range(min..=max).to_string()
            }
        }
        NumericStrategy::SmallNonNegative => {
            // Clamp to 0..=255 range.
            let small_max = T::try_from(255u8).unwrap_or(max);
            let effective_max = if small_max < max { small_max } else { max };
            rng.gen_range(min..=effective_max).to_string()
        }
    };

    Expr::Int(ExprInt { value: InternedText::new(db, &value_str) })
}

/// Generate a signed integer expression.
fn gen_int_expr<'db, R: Rng, T>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &AstGenConfig,
    min: T,
    max: T,
) -> Expr<'db>
where
    T: std::fmt::Display + num_traits::Bounded + num_traits::Zero + num_traits::One + std::ops::Sub<Output = T> + std::ops::Add<Output = T> + Copy + rand::distributions::uniform::SampleUniform + PartialOrd + TryFrom<u8>,
{
    let value_str = match &config.numeric_strategy {
        NumericStrategy::CornerCases => {
            let choices = vec![
                min.to_string(),
                (min + T::one()).to_string(),
                T::zero().to_string(),
                (max - T::one()).to_string(),
                max.to_string(),
            ];
            choices[rng.gen_range(0..choices.len())].clone()
        }
        NumericStrategy::Random => {
            rng.gen_range(min..=max).to_string()
        }
        NumericStrategy::Mixed => {
            if rng.gen_bool(0.2) {
                let choices = vec![
                    min.to_string(),
                    (min + T::one()).to_string(),
                    T::zero().to_string(),
                    (max - T::one()).to_string(),
                    max.to_string(),
                ];
                choices[rng.gen_range(0..choices.len())].clone()
            } else {
                rng.gen_range(min..=max).to_string()
            }
        }
        NumericStrategy::SmallNonNegative => {
            // Generate 0..=127 to fit in all signed types without negation.
            let small_max = T::try_from(127u8).unwrap_or(max);
            let effective_max = if small_max < max { small_max } else { max };
            rng.gen_range(T::zero()..=effective_max).to_string()
        }
    };

    Expr::Int(ExprInt { value: InternedText::new(db, &value_str) })
}

/// Special f32 values represented as hex bit patterns.
///
/// Note: NaN is excluded because NaN != NaN in IEEE semantics, which breaks
/// property tests that assume clone == original.
const F32_HEX_SPECIAL: &[(&str, &str)] = &[
    ("0x7F800000", "+infinity"),
    ("0xFF800000", "-infinity"),
    ("0x7F7FFFFF", "f32::MAX"),
    ("0xFF7FFFFF", "f32::MIN"),
    ("0x00000001", "smallest positive subnormal"),
    ("0x80000001", "smallest negative subnormal"),
    ("0x80000000", "-0.0"),
];

/// Generate a float expression.
///
/// Uses hex literals for special values (infinity, MIN, MAX, subnormals) that
/// cannot be represented via decimal float syntax. NaN is excluded because
/// NaN != NaN breaks property tests.
fn gen_f32_expr<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &AstGenConfig,
) -> Expr<'db> {
    match &config.numeric_strategy {
        NumericStrategy::CornerCases => {
            // Mix of decimal floats and hex for special values.
            let decimal_choices = vec![
                "0.0".to_string(),
                "-0.0".to_string(),
                "1.0".to_string(),
                "-1.0".to_string(),
                "123.456".to_string(),
                "-123.456".to_string(),
            ];

            // 50% chance of hex special value, 50% decimal.
            if rng.gen_bool(0.5) {
                let (hex, _desc) = F32_HEX_SPECIAL[rng.gen_range(0..F32_HEX_SPECIAL.len())];
                Expr::Hex(ExprHex { value: InternedText::new(db, hex) })
            } else {
                let value_str = &decimal_choices[rng.gen_range(0..decimal_choices.len())];
                Expr::Float(ExprFloat { value: InternedText::new(db, value_str) })
            }
        }
        NumericStrategy::Random => {
            // 20% chance of special values via hex.
            if rng.gen_bool(0.2) {
                let (hex, _desc) = F32_HEX_SPECIAL[rng.gen_range(0..F32_HEX_SPECIAL.len())];
                Expr::Hex(ExprHex { value: InternedText::new(db, hex) })
            } else {
                // Random finite floats.
                loop {
                    let val = rng.r#gen::<f32>();
                    if val.is_finite() {
                        let value_str = val.to_string();
                        break Expr::Float(ExprFloat { value: InternedText::new(db, &value_str) });
                    }
                }
            }
        }
        NumericStrategy::Mixed => {
            if rng.gen_bool(0.3) {
                // 30% corner cases: mix of decimal and hex special values.
                let decimal_choices = vec![
                    "0.0".to_string(),
                    "1.0".to_string(),
                    "-1.0".to_string(),
                ];

                if rng.gen_bool(0.5) {
                    let (hex, _desc) = F32_HEX_SPECIAL[rng.gen_range(0..F32_HEX_SPECIAL.len())];
                    Expr::Hex(ExprHex { value: InternedText::new(db, hex) })
                } else {
                    let value_str = &decimal_choices[rng.gen_range(0..decimal_choices.len())];
                    Expr::Float(ExprFloat { value: InternedText::new(db, value_str) })
                }
            } else {
                // 70% random finite floats.
                loop {
                    let val = rng.r#gen::<f32>();
                    if val.is_finite() {
                        let value_str = val.to_string();
                        break Expr::Float(ExprFloat { value: InternedText::new(db, &value_str) });
                    }
                }
            }
        }
        NumericStrategy::SmallNonNegative => {
            // Small positive floats 0.0..255.0.
            let val = rng.gen_range(0.0f32..=255.0);
            let value_str = format!("{:.1}", val);
            Expr::Float(ExprFloat { value: InternedText::new(db, &value_str) })
        }
    }
}

/// Special f64 values represented as hex bit patterns.
///
/// Note: NaN is excluded because NaN != NaN in IEEE semantics, which breaks
/// property tests that assume clone == original.
const F64_HEX_SPECIAL: &[(&str, &str)] = &[
    ("0x7FF0000000000000", "+inf"),
    ("0xFFF0000000000000", "-inf"),
    ("0x7FEFFFFFFFFFFFFF", "f64::MAX"),
    ("0xFFEFFFFFFFFFFFFF", "f64::MIN"),
    ("0x0010000000000000", "smallest positive normal"),
    ("0x8010000000000000", "smallest negative normal"),
    ("0x0000000000000001", "smallest positive subnormal"),
    ("0x8000000000000001", "smallest negative subnormal"),
    ("0x8000000000000000", "-0.0"),
];

/// Generate a f64 expression.
///
/// Uses hex literals for special values (infinity, MIN, MAX, subnormals) that
/// cannot be represented via decimal float syntax. NaN is excluded because
/// NaN != NaN breaks property tests.
fn gen_f64_expr<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &AstGenConfig,
) -> Expr<'db> {
    match &config.numeric_strategy {
        NumericStrategy::CornerCases => {
            // Mix of decimal floats and hex for special values.
            let decimal_choices = vec![
                "0.0".to_string(),
                "-0.0".to_string(),
                "1.0".to_string(),
                "-1.0".to_string(),
                "123.456789012345".to_string(),
                "-123.456789012345".to_string(),
            ];

            // 50% chance of hex special value, 50% decimal.
            if rng.gen_bool(0.5) {
                let (hex, _desc) = F64_HEX_SPECIAL[rng.gen_range(0..F64_HEX_SPECIAL.len())];
                Expr::Hex(ExprHex { value: InternedText::new(db, hex) })
            } else {
                let value_str = &decimal_choices[rng.gen_range(0..decimal_choices.len())];
                Expr::Float(ExprFloat { value: InternedText::new(db, value_str) })
            }
        }
        NumericStrategy::Random => {
            // 20% chance of special values via hex.
            if rng.gen_bool(0.2) {
                let (hex, _desc) = F64_HEX_SPECIAL[rng.gen_range(0..F64_HEX_SPECIAL.len())];
                Expr::Hex(ExprHex { value: InternedText::new(db, hex) })
            } else {
                // Random finite floats.
                loop {
                    let val = rng.r#gen::<f64>();
                    if val.is_finite() {
                        let value_str = val.to_string();
                        break Expr::Float(ExprFloat { value: InternedText::new(db, &value_str) });
                    }
                }
            }
        }
        NumericStrategy::Mixed => {
            if rng.gen_bool(0.3) {
                // 30% corner cases: mix of decimal and hex special values.
                let decimal_choices = vec![
                    "0.0".to_string(),
                    "1.0".to_string(),
                    "-1.0".to_string(),
                ];

                if rng.gen_bool(0.5) {
                    let (hex, _desc) = F64_HEX_SPECIAL[rng.gen_range(0..F64_HEX_SPECIAL.len())];
                    Expr::Hex(ExprHex { value: InternedText::new(db, hex) })
                } else {
                    let value_str = &decimal_choices[rng.gen_range(0..decimal_choices.len())];
                    Expr::Float(ExprFloat { value: InternedText::new(db, value_str) })
                }
            } else {
                // 70% random finite floats.
                loop {
                    let val = rng.r#gen::<f64>();
                    if val.is_finite() {
                        let value_str = val.to_string();
                        break Expr::Float(ExprFloat { value: InternedText::new(db, &value_str) });
                    }
                }
            }
        }
        NumericStrategy::SmallNonNegative => {
            // Small positive floats 0.0..255.0.
            let val = rng.gen_range(0.0f64..=255.0);
            let value_str = format!("{:.1}", val);
            Expr::Float(ExprFloat { value: InternedText::new(db, &value_str) })
        }
    }
}

/// Generate a string expression.
fn gen_string_expr<'db, R: Rng>(db: &'db dyn salsa::Database, rng: &mut R) -> Expr<'db> {
    let strings = vec!["", "a", "test", "hello", "data", "value", "with\"quote", "newline\nhere"];
    let s = strings[rng.gen_range(0..strings.len())];
    // String literals in the AST include the surrounding quotes and escaped content.
    let mut escaped = String::new();
    escaped.push('"');
    for ch in s.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped.push('"');
    Expr::String(ExprString { value: InternedText::new(db, escaped.S()) })
}

/// Generate an ExprFull matching the given type hint.
fn gen_expr_full_inner<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    type_hint: TypeHint<'db>,
    config: &AstGenConfig,
    depth: usize,
) -> ExprFull<'db> {
    let expr = gen_expr_matching_type(db, rng, type_hint.clone(), config, depth);

    // Always include type hints for types that cannot be synthesized.
    // Option::None, Result values, and anonymous enums require type hints
    // for typechecking - the typechecker explicitly rejects these without hints.
    let requires_hint = matches!(
        type_hint,
        TypeHint::Option(_) | TypeHint::Result(_)
    );

    let type_hint_opt = if config.include_type_hints || requires_hint {
        Some(type_hint)
    } else {
        None
    };

    ExprFull::new(db, type_hint_opt, expr)
}

/// Generate an ExprFull for the entire expression tree.
fn gen_expr_full_random_type<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &AstGenConfig,
) -> ExprFull<'db> {
    let type_hint = gen_type_hint_inner(db, rng, config, 0);
    gen_expr_full_inner(db, rng, type_hint, config, 0)
}

/// Generate a tracked ExprFull with a seed for deterministic generation.
///
/// Named types (named tuples, structs, enums) are always disabled for seeded generation
/// because they require external type definitions for name resolution, which standalone
/// expressions don't have.
#[salsa::tracked]
pub fn gen_expr_full_seeded<'db>(
    db: &'db dyn salsa::Database,
    seed: u64,
    config: AstGenConfig,
) -> ExprFull<'db> {
    // Disable named types since they require external type definitions for name resolution.
    let mut adjusted_config = config.clone();
    adjusted_config.type_weights.named_tuple_type = 0;
    adjusted_config.type_weights.named_struct_type = 0;
    adjusted_config.type_weights.named_enum_type = 0;

    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    gen_expr_full_random_type(db, &mut rng, &adjusted_config)
}
