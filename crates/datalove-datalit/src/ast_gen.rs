use rmx::prelude::*;
use bct::text::{InternedText, Text};
use rand::{Rng, SeedableRng};

use crate::ast::*;

/// Configuration for AST generation.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct AstGenConfig {
    /// Maximum nesting depth for recursive types.
    pub max_depth: usize,

    /// Maximum number of elements in collections.
    pub max_collection_size: usize,

    /// Whether to include type hints in generated ExprFull.
    pub include_type_hints: bool,

    /// Distribution of heap annotations.
    pub heap_distribution: HeapDistribution,

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
            max_collection_size: 5,
            include_type_hints: true,
            heap_distribution: HeapDistribution::default(),
            type_weights: TypeWeights::default(),
            numeric_strategy: NumericStrategy::Mixed,
            tensor_config: TensorConfig::default(),
        }
    }
}

/// Distribution of heap annotations.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct HeapDistribution {
    /// Weight for Local (@) heap.
    pub local: u32,

    /// Weight for Global (#) heap.
    pub global: u32,

    /// Weight for Omitted (inferred) heap.
    pub omitted: u32,
}

impl Default for HeapDistribution {
    fn default() -> Self {
        HeapDistribution {
            local: 1,
            global: 1,
            omitted: 3,
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
    pub f32_type: u32,
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
            f32_type: 5,
            int_type: 5,
            string_type: 10,
            list_type: 5,
            map_type: 3,
            set_type: 3,
            option_type: 4,
            result_type: 0,  // Disabled - needs investigation of correct Result semantics.
            tensor_type: 2,
            anon_tuple_type: 4,
            named_tuple_type: 0,  // Disabled by default - requires resolution environment.
            anon_struct_type: 3,
            named_struct_type: 0,  // Disabled by default - requires resolution environment.
            anon_enum_type: 2,
            named_enum_type: 0,  // Disabled by default - requires resolution environment.
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
            f32_type: 5,
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

/// Generate a random heap annotation based on the distribution.
pub fn gen_heap<R: Rng>(rng: &mut R, config: &AstGenConfig) -> Heap {
    let total = config.heap_distribution.local
        + config.heap_distribution.global
        + config.heap_distribution.omitted;
    let choice = rng.gen_range(0..total);

    if choice < config.heap_distribution.local {
        Heap::Local
    } else if choice < config.heap_distribution.local + config.heap_distribution.global {
        Heap::Global
    } else {
        Heap::Omitted
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

/// Generate a random field name.
pub fn gen_field_name<R: Rng>(rng: &mut R) -> String {
    let names = ["x", "y", "z", "name", "value", "data", "field", "item"];
    let name = names[rng.gen_range(0..names.len())];
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
        TypeWeights::leaf_only()
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
    add_choice(weights.int_type, 10);
    add_choice(weights.string_type, 11);
    add_choice(weights.list_type, 12);
    add_choice(weights.map_type, 13);
    add_choice(weights.set_type, 14);
    add_choice(weights.option_type, 15);
    add_choice(weights.result_type, 16);
    add_choice(weights.tensor_type, 17);
    add_choice(weights.anon_tuple_type, 18);
    add_choice(weights.named_tuple_type, 19);
    add_choice(weights.anon_struct_type, 20);
    add_choice(weights.named_struct_type, 21);
    add_choice(weights.anon_enum_type, 22);
    add_choice(weights.named_enum_type, 23);
    add_choice(weights.data_type, 24);
    add_choice(weights.error_type, 25);

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
        10 => TypeHint::Int,
        11 => TypeHint::String,
        12 => {
            let element_type = gen_type_hint_and_heap(db, rng, config, depth + 1);
            TypeHint::List(TypeHintList::new(db, element_type))
        }
        13 => {
            let key_type = gen_type_hint_and_heap(db, rng, config, depth + 1);
            let value_type = gen_type_hint_and_heap(db, rng, config, depth + 1);
            TypeHint::Map(TypeHintMap::new(db, key_type, value_type))
        }
        14 => {
            let element_type = gen_type_hint_and_heap(db, rng, config, depth + 1);
            TypeHint::Set(TypeHintSet::new(db, element_type))
        }
        15 => {
            let inner_type = gen_type_hint_and_heap(db, rng, config, depth + 1);
            TypeHint::Option(TypeHintOption::new(db, inner_type))
        }
        16 => {
            let inner_type = gen_type_hint_and_heap(db, rng, config, depth + 1);
            TypeHint::Result(TypeHintResult::new(db, inner_type))
        }
        17 => {
            let element_type = gen_type_hint_and_heap(db, rng, config, depth + 1);
            let rank = rng.gen_range(1..=config.tensor_config.max_rank);
            TypeHint::Tensor(TypeHintTensor::new(db, element_type, rank))
        }
        18 => {
            let count = rng.gen_range(0..=config.max_collection_size);
            let fields: Vec<_> = (0..count)
                .map(|_| gen_type_hint_and_heap(db, rng, config, depth + 1))
                .collect();
            TypeHint::AnonTuple(TypeHintAnonTuple::new(db, fields))
        }
        19 => {
            let name = InternedText::new(db, &gen_identifier(rng));
            let count = rng.gen_range(0..=config.max_collection_size);
            let fields: Vec<_> = (0..count)
                .map(|_| gen_type_hint_and_heap(db, rng, config, depth + 1))
                .collect();
            TypeHint::NamedTuple(TypeHintNamedTuple::new(db, name, fields))
        }
        20 => {
            let count = rng.gen_range(0..=config.max_collection_size);
            let fields: Vec<_> = (0..count)
                .map(|_| {
                    let name = InternedText::new(db, &gen_field_name(rng));
                    let type_hint = gen_type_hint_and_heap(db, rng, config, depth + 1);
                    TypeHintNamedField::new(db, name, type_hint)
                })
                .collect();
            TypeHint::AnonStruct(TypeHintAnonStruct::new(db, fields))
        }
        21 => {
            let name = InternedText::new(db, &gen_identifier(rng));
            let count = rng.gen_range(0..=config.max_collection_size);
            let fields: Vec<_> = (0..count)
                .map(|_| {
                    let field_name = InternedText::new(db, &gen_field_name(rng));
                    let type_hint = gen_type_hint_and_heap(db, rng, config, depth + 1);
                    TypeHintNamedField::new(db, field_name, type_hint)
                })
                .collect();
            TypeHint::NamedStruct(TypeHintNamedStruct::new(db, name, fields))
        }
        22 => {
            let count = rng.gen_range(1..=config.max_collection_size.max(1));
            let variants: Vec<_> = (0..count)
                .map(|_| {
                    let variant_name = InternedText::new(db, &gen_identifier(rng));
                    let has_payload = rng.gen_bool(0.5);
                    let payload = if has_payload {
                        Some(gen_type_hint_and_heap(db, rng, config, depth + 1))
                    } else {
                        None
                    };
                    TypeHintEnumVariant::new(db, variant_name, payload)
                })
                .collect();
            TypeHint::AnonEnum(TypeHintAnonEnum::new(db, variants))
        }
        23 => {
            let name = InternedText::new(db, &gen_identifier(rng));
            let count = rng.gen_range(1..=config.max_collection_size.max(1));
            let variants: Vec<_> = (0..count)
                .map(|_| {
                    let variant_name = InternedText::new(db, &gen_identifier(rng));
                    let has_payload = rng.gen_bool(0.5);
                    let payload = if has_payload {
                        Some(gen_type_hint_and_heap(db, rng, config, depth + 1))
                    } else {
                        None
                    };
                    TypeHintEnumVariant::new(db, variant_name, payload)
                })
                .collect();
            TypeHint::NamedEnum(TypeHintNamedEnum::new(db, name, variants))
        }
        24 => TypeHint::Data,
        25 => TypeHint::Error,
        _ => TypeHint::Bool,
    }
}

/// Generate a TypeHintAndHeap with random heap annotation.
pub fn gen_type_hint_and_heap<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &AstGenConfig,
    depth: usize,
) -> TypeHintAndHeap<'db> {
    let heap = gen_heap(rng, config);
    let type_hint = gen_type_hint(db, rng, config, depth);
    TypeHintAndHeap::new(db, heap, type_hint)
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
        TypeHint::F32 => gen_f32_expr(db, rng, config),
        TypeHint::Int => gen_int_expr(db, rng, config, i64::MIN, i64::MAX),
        TypeHint::String => gen_string_expr(db, rng),
        TypeHint::AnonTuple(th) => {
            let elements: Vec<_> = th
                .fields(db)
                .iter()
                .map(|field| {
                    let field_type = field.type_hint(db);
                    let field_heap = field.heap(db);
                    gen_expr_full_with_heap(db, rng, field_type, field_heap, config, depth + 1)
                })
                .collect();
            Expr::AnonTuple(ExprAnonTuple::new(db, elements))
        }
        TypeHint::NamedTuple(th) => {
            let name = th.name(db);
            let elements: Vec<_> = th
                .fields(db)
                .iter()
                .map(|field| {
                    let field_type = field.type_hint(db);
                    let field_heap = field.heap(db);
                    gen_expr_full_with_heap(db, rng, field_type, field_heap, config, depth + 1)
                })
                .collect();
            Expr::NamedTuple(ExprNamedTuple::new(db, name, elements))
        }
        TypeHint::AnonStruct(th) => {
            let fields: Vec<_> = th
                .fields(db)
                .iter()
                .map(|field| {
                    let name = field.name(db);
                    let field_type_and_heap = field.type_hint(db);
                    let field_type = field_type_and_heap.type_hint(db);
                    let field_heap = field_type_and_heap.heap(db);
                    let value = gen_expr_full_with_heap(db, rng, field_type, field_heap, config, depth + 1);
                    ExprStructField::new(db, name, value)
                })
                .collect();
            Expr::AnonStruct(ExprAnonStruct::new(db, fields))
        }
        TypeHint::NamedStruct(th) => {
            let name = th.name(db);
            let fields: Vec<_> = th
                .fields(db)
                .iter()
                .map(|field| {
                    let field_name = field.name(db);
                    let field_type_and_heap = field.type_hint(db);
                    let field_type = field_type_and_heap.type_hint(db);
                    let field_heap = field_type_and_heap.heap(db);
                    let value = gen_expr_full_with_heap(db, rng, field_type, field_heap, config, depth + 1);
                    ExprStructField::new(db, field_name, value)
                })
                .collect();
            Expr::NamedStruct(ExprNamedStruct::new(db, name, fields))
        }
        TypeHint::AnonEnum(th) => {
            let variants = th.variants(db);
            if variants.is_empty() {
                return Expr::None;
            }
            let variant = &variants[rng.gen_range(0..variants.len())];
            let variant_name = variant.name(db);
            let payload = match variant.payload(db) {
                Some(payload_type_and_heap) => {
                    let payload_type = payload_type_and_heap.type_hint(db);
                    let payload_heap = payload_type_and_heap.heap(db);
                    Some(gen_expr_full_with_heap(db, rng, payload_type, payload_heap, config, depth + 1))
                }
                None => None,
            };
            Expr::AnonEnum(ExprAnonEnum::new(db, variant_name, payload))
        }
        TypeHint::NamedEnum(th) => {
            let enum_name = th.name(db);
            let variants = th.variants(db);
            if variants.is_empty() {
                return Expr::None;
            }
            let variant = &variants[rng.gen_range(0..variants.len())];
            let variant_name = variant.name(db);
            let payload = match variant.payload(db) {
                Some(payload_type_and_heap) => {
                    let payload_type = payload_type_and_heap.type_hint(db);
                    let payload_heap = payload_type_and_heap.heap(db);
                    Some(gen_expr_full_with_heap(db, rng, payload_type, payload_heap, config, depth + 1))
                }
                None => None,
            };
            Expr::NamedEnum(ExprNamedEnum::new(db, enum_name, variant_name, payload))
        }
        TypeHint::List(th) => {
            let element_type_and_heap = th.element_type(db);
            let element_type = element_type_and_heap.type_hint(db);
            let element_heap = element_type_and_heap.heap(db);
            let count = rng.gen_range(0..=config.max_collection_size);
            let elements: Vec<_> = (0..count)
                .map(|_| gen_expr_full_with_heap(db, rng, element_type.clone(), element_heap, config, depth + 1))
                .collect();
            Expr::List(ExprList::new(db, elements))
        }
        TypeHint::Map(th) => {
            let key_type_and_heap = th.key_type(db);
            let key_type = key_type_and_heap.type_hint(db);
            let key_heap = key_type_and_heap.heap(db);
            let value_type_and_heap = th.value_type(db);
            let value_type = value_type_and_heap.type_hint(db);
            let value_heap = value_type_and_heap.heap(db);
            let count = rng.gen_range(0..=config.max_collection_size);
            let entries: Vec<_> = (0..count)
                .map(|_| {
                    let key = gen_expr_full_with_heap(db, rng, key_type.clone(), key_heap, config, depth + 1);
                    let value = gen_expr_full_with_heap(db, rng, value_type.clone(), value_heap, config, depth + 1);
                    ExprMapEntry::new(db, key, value)
                })
                .collect();
            Expr::Map(ExprMap::new(db, entries))
        }
        TypeHint::Set(th) => {
            let element_type_and_heap = th.element_type(db);
            let element_type = element_type_and_heap.type_hint(db);
            let element_heap = element_type_and_heap.heap(db);
            let count = rng.gen_range(0..=config.max_collection_size);
            let elements: Vec<_> = (0..count)
                .map(|_| gen_expr_full_with_heap(db, rng, element_type.clone(), element_heap, config, depth + 1))
                .collect();
            Expr::Set(ExprSet::new(db, elements))
        }
        TypeHint::Option(th) => {
            let inner_type_and_heap = th.inner_type(db);
            let inner_type = inner_type_and_heap.type_hint(db);
            let inner_heap = inner_type_and_heap.heap(db);
            if rng.gen_bool(0.5) {
                Expr::None
            } else {
                gen_expr_matching_type(db, rng, inner_type, config, depth + 1)
            }
        }
        TypeHint::Result(th) => {
            let inner_type_and_heap = th.inner_type(db);
            let inner_type = inner_type_and_heap.type_hint(db);
            let inner_heap = inner_type_and_heap.heap(db);
            if rng.gen_bool(0.5) {
                let value = gen_expr_full_with_heap(db, rng, inner_type, inner_heap, config, depth + 1);
                Expr::Data(ExprData::new(db, value))
            } else {
                let error_msg = gen_string_expr(db, rng);
                let error_expr_full = ExprFull::new(
                    db,
                    None,
                    ExprAndHeap::new(db, Heap::Omitted, error_msg),
                );
                Expr::Err(ExprErr::new(db, error_expr_full))
            }
        }
        TypeHint::Tensor(th) => {
            let element_type_and_heap = th.element_type(db);
            let element_type = element_type_and_heap.type_hint(db);
            let element_heap = element_type_and_heap.heap(db);
            let rank = th.rank(db);
            let shape: Vec<u32> = (0..rank)
                .map(|_| rng.gen_range(1..=config.tensor_config.max_dim_size))
                .collect();
            let total_elements: usize = shape.iter().map(|&d| d as usize).product();
            let elements: Vec<_> = (0..total_elements)
                .map(|_| gen_expr_full_with_heap(db, rng, element_type.clone(), element_heap, config, depth + 1))
                .collect();
            Expr::Tensor(ExprTensor::new(db, shape, elements))
        }
        TypeHint::Data => {
            let inner_type = gen_type_hint(db, rng, config, depth + 1);
            let value = gen_expr_full_matching_type(db, rng, inner_type, config, depth + 1);
            Expr::Data(ExprData::new(db, value))
        }
        TypeHint::Error => {
            let error_msg = gen_string_expr(db, rng);
            let error_expr_full = ExprFull::new(
                db,
                None,
                ExprAndHeap::new(db, Heap::Omitted, error_msg),
            );
            Expr::Err(ExprErr::new(db, error_expr_full))
        }
        TypeHint::ParseError(_) => Expr::None,
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
    T: std::fmt::Display + num_traits::Bounded + num_traits::One + std::ops::Sub<Output = T> + Copy + rand::distributions::uniform::SampleUniform + PartialOrd,
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
    };

    Expr::Int(ExprInt::new(db, InternedText::new(db, &value_str)))
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
    T: std::fmt::Display + num_traits::Bounded + num_traits::Zero + num_traits::One + std::ops::Sub<Output = T> + std::ops::Add<Output = T> + Copy + rand::distributions::uniform::SampleUniform + PartialOrd,
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
    };

    Expr::Int(ExprInt::new(db, InternedText::new(db, &value_str)))
}

/// Generate a float expression.
fn gen_f32_expr<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &AstGenConfig,
) -> Expr<'db> {
    let value_str = match &config.numeric_strategy {
        NumericStrategy::CornerCases => {
            let choices = vec![
                "0.0".to_string(),
                "-0.0".to_string(),
                f32::MIN.to_string(),
                f32::MAX.to_string(),
                f32::INFINITY.to_string(),
                f32::NEG_INFINITY.to_string(),
                "nan".to_string(),
                "1.0".to_string(),
                "-1.0".to_string(),
            ];
            choices[rng.gen_range(0..choices.len())].clone()
        }
        NumericStrategy::Random => {
            rng.r#gen::<f32>().to_string()
        }
        NumericStrategy::Mixed => {
            if rng.gen_bool(0.2) {
                let choices = vec![
                    "0.0".to_string(),
                    "-0.0".to_string(),
                    f32::MIN.to_string(),
                    f32::MAX.to_string(),
                    f32::INFINITY.to_string(),
                    f32::NEG_INFINITY.to_string(),
                    "nan".to_string(),
                    "1.0".to_string(),
                    "-1.0".to_string(),
                ];
                choices[rng.gen_range(0..choices.len())].clone()
            } else {
                rng.r#gen::<f32>().to_string()
            }
        }
    };

    Expr::Float(ExprFloat::new(db, InternedText::new(db, &value_str)))
}

/// Generate a string expression.
fn gen_string_expr<'db, R: Rng>(db: &'db dyn salsa::Database, rng: &mut R) -> Expr<'db> {
    let strings = vec!["", "a", "test", "hello", "data", "value"];
    let s = strings[rng.gen_range(0..strings.len())];
    Expr::String(ExprString::new(db, InternedText::new(db, s)))
}

/// Generate an ExprFull matching the given type hint with a specific heap.
fn gen_expr_full_with_heap<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    type_hint: TypeHint<'db>,
    heap: Heap,
    config: &AstGenConfig,
    depth: usize,
) -> ExprFull<'db> {
    let expr = gen_expr_matching_type(db, rng, type_hint.clone(), config, depth);
    let expr_and_heap = ExprAndHeap::new(db, heap, expr);

    let type_hint_opt = if config.include_type_hints {
        Some(TypeHintAndHeap::new(db, heap, type_hint))
    } else {
        None
    };

    ExprFull::new(db, type_hint_opt, expr_and_heap)
}

/// Generate an ExprFull matching the given type hint.
fn gen_expr_full_matching_type<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    type_hint: TypeHint<'db>,
    config: &AstGenConfig,
    depth: usize,
) -> ExprFull<'db> {
    let heap = gen_heap(rng, config);
    gen_expr_full_with_heap(db, rng, type_hint, heap, config, depth)
}

/// Generate a random ExprFull with random type.
///
/// Note: This function cannot be marked with #[salsa::tracked] because it requires mutable RNG.
/// Callers should wrap calls in their own tracked functions if needed.
pub fn gen_expr_full<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &AstGenConfig,
) -> ExprFull<'db> {
    let type_hint = gen_type_hint(db, rng, config, 0);
    gen_expr_full_matching_type(db, rng, type_hint, config, 0)
}

/// Generate a tracked ExprFull with a seed for deterministic generation.
#[salsa::tracked]
pub fn gen_expr_full_seeded<'db>(
    db: &'db dyn salsa::Database,
    seed: u64,
    config: AstGenConfig,
) -> ExprFull<'db> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    gen_expr_full(db, &mut rng, &config)
}
