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
            min_collection_size: 0,
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
        if config.type_weights.f32_type == 0 { leaf.f32_type = 0; }
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
    add_choice(weights.int_type, 10);
    add_choice(weights.string_type, 11);
    add_choice(weights.list_type, 12);
    add_choice(weights.map_type, 13);
    add_choice(weights.set_type, 14);
    add_choice(weights.option_type, 15);
    add_choice(weights.result_type, 16);
    add_choice(weights.tensor_type, 17);
    add_choice(weights.anon_tuple_type, 18);
    add_choice(weights.anon_struct_type, 19);
    add_choice(weights.anon_enum_type, 20);
    add_choice(weights.data_type, 21);
    add_choice(weights.error_type, 22);

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
            let count = rng.gen_range(config.min_collection_size..=config.max_collection_size);
            let fields: Vec<_> = (0..count)
                .map(|_| gen_type_hint_and_heap(db, rng, config, depth + 1))
                .collect();
            TypeHint::AnonTuple(TypeHintAnonTuple::new(db, fields))
        }
        19 => {
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
                    let type_hint = gen_type_hint_and_heap(db, rng, config, depth + 1);
                    TypeHintNamedField::new(db, name, type_hint)
                })
                .collect();
            TypeHint::AnonStruct(TypeHintAnonStruct::new(db, fields))
        }
        20 => {
            let count = rng.gen_range(1..=config.max_collection_size.max(1));
            let mut used_names = std::collections::HashSet::new();
            let variants: Vec<_> = (0..count)
                .map(|_| {
                    // Generate unique variant name.
                    let variant_name = loop {
                        let candidate = gen_identifier(rng);
                        if used_names.insert(candidate.clone()) {
                            break InternedText::new(db, &candidate);
                        }
                    };
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
        21 => TypeHint::Data,
        22 => TypeHint::Error,
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

/// Generate a TypeHint with a fixed heap for all nested types.
fn gen_type_hint_with_fixed_heap<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &AstGenConfig,
    heap: Heap,
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
        if config.type_weights.f32_type == 0 { leaf.f32_type = 0; }
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
    add_choice(weights.int_type, 10);
    add_choice(weights.string_type, 11);
    add_choice(weights.list_type, 12);
    add_choice(weights.map_type, 13);
    add_choice(weights.set_type, 14);
    add_choice(weights.option_type, 15);
    add_choice(weights.result_type, 16);
    add_choice(weights.tensor_type, 17);
    add_choice(weights.anon_tuple_type, 18);
    add_choice(weights.anon_struct_type, 19);
    add_choice(weights.anon_enum_type, 20);
    add_choice(weights.data_type, 21);
    add_choice(weights.error_type, 22);

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
            let element_type = TypeHintAndHeap::new(db, heap, gen_type_hint_with_fixed_heap(db, rng, config, heap, depth + 1));
            TypeHint::List(TypeHintList::new(db, element_type))
        }
        13 => {
            let key_type = TypeHintAndHeap::new(db, heap, gen_type_hint_with_fixed_heap(db, rng, config, heap, depth + 1));
            let value_type = TypeHintAndHeap::new(db, heap, gen_type_hint_with_fixed_heap(db, rng, config, heap, depth + 1));
            TypeHint::Map(TypeHintMap::new(db, key_type, value_type))
        }
        14 => {
            let element_type = TypeHintAndHeap::new(db, heap, gen_type_hint_with_fixed_heap(db, rng, config, heap, depth + 1));
            TypeHint::Set(TypeHintSet::new(db, element_type))
        }
        15 => {
            let inner_type = TypeHintAndHeap::new(db, heap, gen_type_hint_with_fixed_heap(db, rng, config, heap, depth + 1));
            TypeHint::Option(TypeHintOption::new(db, inner_type))
        }
        16 => {
            let inner_type = TypeHintAndHeap::new(db, heap, gen_type_hint_with_fixed_heap(db, rng, config, heap, depth + 1));
            TypeHint::Result(TypeHintResult::new(db, inner_type))
        }
        17 => {
            let element_type = TypeHintAndHeap::new(db, heap, gen_type_hint_with_fixed_heap(db, rng, config, heap, depth + 1));
            let rank = rng.gen_range(1..=config.tensor_config.max_rank);
            TypeHint::Tensor(TypeHintTensor::new(db, element_type, rank))
        }
        18 => {
            let count = rng.gen_range(config.min_collection_size..=config.max_collection_size);
            let fields: Vec<_> = (0..count)
                .map(|_| TypeHintAndHeap::new(db, heap, gen_type_hint_with_fixed_heap(db, rng, config, heap, depth + 1)))
                .collect();
            TypeHint::AnonTuple(TypeHintAnonTuple::new(db, fields))
        }
        19 => {
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
                    let type_hint = TypeHintAndHeap::new(db, heap, gen_type_hint_with_fixed_heap(db, rng, config, heap, depth + 1));
                    TypeHintNamedField::new(db, name, type_hint)
                })
                .collect();
            TypeHint::AnonStruct(TypeHintAnonStruct::new(db, fields))
        }
        20 => {
            let count = rng.gen_range(1..=config.max_collection_size.max(1));
            let mut used_names = std::collections::HashSet::new();
            let variants: Vec<_> = (0..count)
                .map(|_| {
                    // Generate unique variant name.
                    let variant_name = loop {
                        let candidate = gen_identifier(rng);
                        if used_names.insert(candidate.clone()) {
                            break InternedText::new(db, &candidate);
                        }
                    };
                    let has_payload = rng.gen_bool(0.5);
                    let payload = if has_payload {
                        Some(TypeHintAndHeap::new(db, heap, gen_type_hint_with_fixed_heap(db, rng, config, heap, depth + 1)))
                    } else {
                        None
                    };
                    TypeHintEnumVariant::new(db, variant_name, payload)
                })
                .collect();
            TypeHint::AnonEnum(TypeHintAnonEnum::new(db, variants))
        }
        21 => TypeHint::Data,
        22 => TypeHint::Error,
        _ => TypeHint::Bool,
    }
}

/// Generate a value matching the given type hint.
///
/// Returns the expression and the heap that should be used for wrapping it.
/// For most types this is just the input `heap`, but for Option/Result success
/// values it may be the inner type's heap.
pub fn gen_expr_matching_type<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    type_hint: TypeHint<'db>,
    heap: Heap,
    config: &AstGenConfig,
    depth: usize,
) -> (Expr<'db>, Heap) {
    match type_hint {
        TypeHint::Bool => {
            let expr = if rng.gen_bool(0.5) {
                Expr::True
            } else {
                Expr::False
            };
            (expr, heap)
        }
        TypeHint::U8 => (gen_uint_expr(db, rng, config, 0u8, u8::MAX), heap),
        TypeHint::I8 => (gen_int_expr(db, rng, config, i8::MIN, i8::MAX), heap),
        TypeHint::U16 => (gen_uint_expr(db, rng, config, 0u16, u16::MAX), heap),
        TypeHint::I16 => (gen_int_expr(db, rng, config, i16::MIN, i16::MAX), heap),
        TypeHint::U32 => (gen_uint_expr(db, rng, config, 0u32, u32::MAX), heap),
        TypeHint::I32 => (gen_int_expr(db, rng, config, i32::MIN, i32::MAX), heap),
        TypeHint::U64 => (gen_uint_expr(db, rng, config, 0u64, u64::MAX), heap),
        TypeHint::I64 => (gen_int_expr(db, rng, config, i64::MIN, i64::MAX), heap),
        TypeHint::F32 => (gen_f32_expr(db, rng, config), heap),
        TypeHint::F64 => (gen_f64_expr(db, rng, config), heap),
        TypeHint::Int => (gen_int_expr(db, rng, config, i64::MIN, i64::MAX), heap),
        TypeHint::String => (gen_string_expr(db, rng), heap),
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
            (Expr::AnonTuple(ExprAnonTuple::new(db, elements)), heap)
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
            (Expr::AnonStruct(ExprAnonStruct::new(db, fields)), heap)
        }
        TypeHint::AnonEnum(th) => {
            let variants = th.variants(db);
            if variants.is_empty() {
                return (Expr::None, heap);
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
            (Expr::AnonEnum(ExprAnonEnum::new(db, variant_name, payload)), heap)
        }
        TypeHint::List(th) => {
            let element_type_and_heap = th.element_type(db);
            let element_type = element_type_and_heap.type_hint(db);
            let element_heap = element_type_and_heap.heap(db);
            let count = rng.gen_range(config.min_collection_size..=config.max_collection_size);
            let elements: Vec<_> = (0..count)
                .map(|_| gen_expr_full_with_heap(db, rng, element_type.clone(), element_heap, config, depth + 1))
                .collect();
            (Expr::List(ExprList::new(db, elements)), heap)
        }
        TypeHint::Map(th) => {
            let key_type_and_heap = th.key_type(db);
            let key_type = key_type_and_heap.type_hint(db);
            let key_heap = key_type_and_heap.heap(db);
            let value_type_and_heap = th.value_type(db);
            let value_type = value_type_and_heap.type_hint(db);
            let value_heap = value_type_and_heap.heap(db);
            let count = rng.gen_range(config.min_collection_size..=config.max_collection_size);
            let entries: Vec<_> = (0..count)
                .map(|_| {
                    let key = gen_expr_full_with_heap(db, rng, key_type.clone(), key_heap, config, depth + 1);
                    let value = gen_expr_full_with_heap(db, rng, value_type.clone(), value_heap, config, depth + 1);
                    ExprMapEntry::new(db, key, value)
                })
                .collect();
            (Expr::Map(ExprMap::new(db, entries)), heap)
        }
        TypeHint::Set(th) => {
            let element_type_and_heap = th.element_type(db);
            let element_type = element_type_and_heap.type_hint(db);
            let element_heap = element_type_and_heap.heap(db);
            let count = rng.gen_range(config.min_collection_size..=config.max_collection_size);
            let elements: Vec<_> = (0..count)
                .map(|_| gen_expr_full_with_heap(db, rng, element_type.clone(), element_heap, config, depth + 1))
                .collect();
            (Expr::Set(ExprSet::new(db, elements)), heap)
        }
        TypeHint::Option(th) => {
            let inner_type_and_heap = th.inner_type(db);
            let inner_type = inner_type_and_heap.type_hint(db);
            let inner_heap = inner_type_and_heap.heap(db);
            if rng.gen_bool(0.5) {
                // None uses the outer heap.
                (Expr::None, heap)
            } else {
                // Some case: wrap payload in Expr::Some.
                let payload = gen_expr_full_with_heap(db, rng, inner_type, inner_heap, config, depth + 1);
                (Expr::Some(ExprSome::new(db, payload)), heap)
            }
        }
        TypeHint::Result(th) => {
            let inner_type_and_heap = th.inner_type(db);
            let inner_type = inner_type_and_heap.type_hint(db);
            let inner_heap = inner_type_and_heap.heap(db);
            if rng.gen_bool(0.5) {
                // Success case: wrap payload in Expr::Ok.
                let payload = gen_expr_full_with_heap(db, rng, inner_type, inner_heap, config, depth + 1);
                (Expr::Ok(ExprOk::new(db, payload)), heap)
            } else {
                // Error case: generate error with an arbitrary value.
                // Use fixed heap to ensure all heaps match throughout.
                let error_type = gen_type_hint_with_fixed_heap(db, rng, config, heap, depth + 1);
                let (error_value, _) = gen_expr_matching_type(db, rng, error_type.clone(), heap, config, depth + 1);
                let error_type_hint = TypeHintAndHeap::new(db, heap, error_type);
                let error_expr_full = ExprFull::new(
                    db,
                    Some(error_type_hint),
                    ExprAndHeap::new(db, heap, error_value),
                );
                (Expr::Error(ExprError::new(db, error_expr_full)), heap)
            }
        }
        TypeHint::Tensor(th) => {
            let element_type_and_heap = th.element_type(db);
            let element_type = element_type_and_heap.type_hint(db);
            let element_heap = element_type_and_heap.heap(db);
            let rank = th.rank(db);
            // Generate shape ensuring total elements don't exceed max_collection_size.
            let mut shape: Vec<u32> = Vec::with_capacity(rank as usize);
            let mut total_elements = 1usize;
            for i in 0..rank {
                let remaining_dims = rank - i;
                let max_dim = if remaining_dims == 1 {
                    // Last dimension: use all remaining budget.
                    config.max_collection_size / total_elements.max(1)
                } else {
                    // Not last: leave room for other dimensions.
                    config.tensor_config.max_dim_size as usize
                };
                let dim_size = rng.gen_range(1..=max_dim.min(config.tensor_config.max_dim_size as usize).max(1)) as u32;
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
                .map(|_| gen_expr_full_with_heap(db, rng, element_type.clone(), element_heap, config, depth + 1))
                .collect();
            (Expr::Tensor(ExprTensor::new(db, shape, elements)), heap)
        }
        TypeHint::Data => {
            // Use fixed heap for inner type to ensure all heaps match.
            let inner_type = gen_type_hint_with_fixed_heap(db, rng, config, heap, depth + 1);
            let value = gen_expr_full_with_heap(db, rng, inner_type, heap, config, depth + 1);
            (Expr::Data(ExprData::new(db, value)), heap)
        }
        TypeHint::Error => {
            let error_msg = gen_string_expr(db, rng);
            let error_expr_full = ExprFull::new(
                db,
                None,
                ExprAndHeap::new(db, Heap::Omitted, error_msg),
            );
            (Expr::Error(ExprError::new(db, error_expr_full)), heap)
        }
        TypeHint::ParseError(_) => (Expr::None, heap),
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

    Expr::Int(ExprInt::new(db, InternedText::new(db, &value_str)))
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
                Expr::Hex(ExprHex::new(db, InternedText::new(db, hex)))
            } else {
                let value_str = &decimal_choices[rng.gen_range(0..decimal_choices.len())];
                Expr::Float(ExprFloat::new(db, InternedText::new(db, value_str)))
            }
        }
        NumericStrategy::Random => {
            // 20% chance of special values via hex.
            if rng.gen_bool(0.2) {
                let (hex, _desc) = F32_HEX_SPECIAL[rng.gen_range(0..F32_HEX_SPECIAL.len())];
                Expr::Hex(ExprHex::new(db, InternedText::new(db, hex)))
            } else {
                // Random finite floats.
                loop {
                    let val = rng.r#gen::<f32>();
                    if val.is_finite() {
                        let value_str = val.to_string();
                        break Expr::Float(ExprFloat::new(db, InternedText::new(db, &value_str)));
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
                    Expr::Hex(ExprHex::new(db, InternedText::new(db, hex)))
                } else {
                    let value_str = &decimal_choices[rng.gen_range(0..decimal_choices.len())];
                    Expr::Float(ExprFloat::new(db, InternedText::new(db, value_str)))
                }
            } else {
                // 70% random finite floats.
                loop {
                    let val = rng.r#gen::<f32>();
                    if val.is_finite() {
                        let value_str = val.to_string();
                        break Expr::Float(ExprFloat::new(db, InternedText::new(db, &value_str)));
                    }
                }
            }
        }
        NumericStrategy::SmallNonNegative => {
            // Small positive floats 0.0..255.0.
            let val = rng.gen_range(0.0f32..=255.0);
            let value_str = format!("{:.1}", val);
            Expr::Float(ExprFloat::new(db, InternedText::new(db, &value_str)))
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
                Expr::Hex(ExprHex::new(db, InternedText::new(db, hex)))
            } else {
                let value_str = &decimal_choices[rng.gen_range(0..decimal_choices.len())];
                Expr::Float(ExprFloat::new(db, InternedText::new(db, value_str)))
            }
        }
        NumericStrategy::Random => {
            // 20% chance of special values via hex.
            if rng.gen_bool(0.2) {
                let (hex, _desc) = F64_HEX_SPECIAL[rng.gen_range(0..F64_HEX_SPECIAL.len())];
                Expr::Hex(ExprHex::new(db, InternedText::new(db, hex)))
            } else {
                // Random finite floats.
                loop {
                    let val = rng.r#gen::<f64>();
                    if val.is_finite() {
                        let value_str = val.to_string();
                        break Expr::Float(ExprFloat::new(db, InternedText::new(db, &value_str)));
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
                    Expr::Hex(ExprHex::new(db, InternedText::new(db, hex)))
                } else {
                    let value_str = &decimal_choices[rng.gen_range(0..decimal_choices.len())];
                    Expr::Float(ExprFloat::new(db, InternedText::new(db, value_str)))
                }
            } else {
                // 70% random finite floats.
                loop {
                    let val = rng.r#gen::<f64>();
                    if val.is_finite() {
                        let value_str = val.to_string();
                        break Expr::Float(ExprFloat::new(db, InternedText::new(db, &value_str)));
                    }
                }
            }
        }
        NumericStrategy::SmallNonNegative => {
            // Small positive floats 0.0..255.0.
            let val = rng.gen_range(0.0f64..=255.0);
            let value_str = format!("{:.1}", val);
            Expr::Float(ExprFloat::new(db, InternedText::new(db, &value_str)))
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
    Expr::String(ExprString::new(db, InternedText::new(db, escaped.S())))
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
    let (expr, value_heap) = gen_expr_matching_type(db, rng, type_hint.clone(), heap, config, depth);
    let expr_and_heap = ExprAndHeap::new(db, value_heap, expr);

    // Always include type hints for types that cannot be synthesized.
    // Option::None, Result values, and anonymous enums require type hints
    // for typechecking - the typechecker explicitly rejects these without hints.
    let requires_hint = matches!(
        type_hint,
        TypeHint::Option(_) | TypeHint::Result(_) | TypeHint::AnonEnum(_)
    );

    let type_hint_opt = if config.include_type_hints || requires_hint {
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

/// Generate an ExprFull with a fixed heap for the entire expression tree.
fn gen_expr_full_with_fixed_heap<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &AstGenConfig,
    heap: Heap,
) -> ExprFull<'db> {
    let type_hint = gen_type_hint_with_fixed_heap(db, rng, config, heap, 0);
    // Use the existing gen_expr_full_with_heap but ensure we pass the fixed heap.
    gen_expr_full_with_heap(db, rng, type_hint, heap, config, 0)
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

    // Use separate RNG for heap to avoid shifting the main random sequence.
    let mut heap_rng = rand::rngs::StdRng::seed_from_u64(seed.wrapping_mul(0x9e3779b97f4a7c15));
    let heap = gen_heap(&mut heap_rng, &adjusted_config);

    // Main RNG for type and value generation preserves original sequence.
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    gen_expr_full_with_fixed_heap(db, &mut rng, &adjusted_config, heap)
}
