//! Shared type definitions for datafun.
//!
//! Contains types used by both name resolution and typechecking phases:
//! Type, TypeFunction, TypeError, ParsedModuleGraph, and related utilities.

use std::collections::{HashMap, BTreeMap};
use bct::text::InternedText;
use bct::module_graph::ModuleId;
use datalove_datalit as datalit;

pub mod generics;
use datalit::ast::TypeHint;

use salsa::Database as Db;

use datalove_datafun_ast::ast::{ParsedStatements, StmtFun, ParamMode};
use datalove_datafun_ast::spans::DatafunSpans;

// ============================================================================
// Parallel Execution Infrastructure
// ============================================================================

/// Extension trait for database cloning in parallel execution.
///
/// This trait provides a `dyn_clone` method that enables cloning the database
/// through a trait object. When a database is cloned, salsa creates a new
/// ZalsaLocal (thread-local state) while sharing the Arc<Zalsa> (global state).
/// This allows safe parallel query execution across threads.
///
/// Implement this trait for your concrete Database types to enable parallel
/// query execution with rayon.
pub trait DbClone: salsa::Database {
    /// Clone the database for use on another thread.
    ///
    /// Returns a boxed clone that can be sent to another thread. Each clone
    /// has its own ZalsaLocal but shares the underlying Zalsa state.
    fn dyn_clone(&self) -> Box<dyn DbClone + Send>;

    /// Get a reference to self as a salsa::Database trait object.
    ///
    /// This allows passing the cloned database to salsa tracked functions
    /// that expect `&dyn salsa::Database`.
    fn as_salsa_db(&self) -> &dyn salsa::Database;
}

/// Controls whether compilation runs sequentially or in parallel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ParallelMode {
    /// Sequential compilation (default, salsa-tracked).
    #[default]
    Sequential,
    /// Parallel compilation with rayon (warms cache, then delegates to sequential).
    Parallel,
}

/// Read parallel mode from `DATALOVE_PARALLEL` environment variable.
///
/// Returns `ParallelMode::Parallel` if the env var is set (to any value),
/// otherwise returns `ParallelMode::Sequential`.
pub fn parallel_mode_from_env() -> ParallelMode {
    match std::env::var("DATALOVE_PARALLEL") {
        Ok(_) => ParallelMode::Parallel,
        Err(_) => ParallelMode::Sequential,
    }
}

/// Controls automatic adaptation of type mismatches via the `@` operator.
///
/// When enabled, the compiler automatically inserts `@` (adapt) operations
/// to fix recoverable type errors, such as:
/// - Integer widening (u8 → int, i16 → i32, etc.)
/// - Cross-sign widening (u8 → i16, u16 → i32, etc.)
/// - Cloning linear types for reuse
///
/// See `botdocs/report-adapt-cases.md` for the full list of recoverable errors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[derive(salsa::SalsaValue)]
pub enum AutoAdaptMode {
    /// Disabled - emit errors for type mismatches (default).
    #[default]
    Disabled,
    /// Enabled - automatically insert @ for recoverable errors (silent).
    Enabled,
    /// Enabled with reporting - insert @ and emit info diagnostics showing what was adapted.
    EnabledWithReport,
}

impl AutoAdaptMode {
    /// Returns true if auto-adapt is enabled (either silent or with reporting).
    pub fn is_enabled(&self) -> bool {
        matches!(self, AutoAdaptMode::Enabled | AutoAdaptMode::EnabledWithReport)
    }

    /// Returns true if adaptations should be reported as diagnostics.
    pub fn should_report(&self) -> bool {
        matches!(self, AutoAdaptMode::EnabledWithReport)
    }
}

// ============================================================================
// Rider Interface
// ============================================================================

/// A parsed rider interface from a `.dli` file.
///
/// Contains native function signatures and type aliases exported by a rider.
/// Plain data, not salsa::tracked. Flows through the pipeline as part of
/// `ParsedModuleGraph`.
#[derive(Clone, PartialEq, Eq, Hash)]
#[derive(salsa::SalsaValue)]
pub struct RiderInterface<'db> {
    pub name: InternedText<'db>,
    /// Synthetic ModuleId for this rider (e.g. `@rider/testlib`).
    pub module_id: ModuleId<'db>,
    pub functions: Vec<(InternedText<'db>, TypeFunction<'db>)>,
    pub type_aliases: Vec<(InternedText<'db>, Type<'db>)>,
}

// ============================================================================
// Parsed Module Graph
// ============================================================================

/// A module graph paired with pre-parsed statements and spans for each module.
#[salsa::tracked]
pub struct ParsedModuleGraph<'db> {
    /// The underlying module graph (identity key).
    #[returns(copy)]
    pub graph: bct::module_graph::ModuleGraph<'db>,

    /// Pre-parsed statements only, as (ModuleId<'db>, ParsedStatements) tuples.
    /// Separate from spans so typecheck can depend only on statements.
    /// Order matches graph.iter_modules() order.
    #[tracked]
    #[returns(ref)]
    pub statements_only: Vec<(ModuleId<'db>, ParsedStatements<'db>)>,

    /// Expression spans for each module, separate from statements.
    /// Changes to spans don't invalidate typecheck.
    #[tracked]
    #[returns(ref)]
    pub spans: Vec<(ModuleId<'db>, DatafunSpans<'db>)>,

    /// Resolved module requires from package resolution.
    ///
    /// Maps each module to its resolved require aliases: (alias, target_module_id).
    /// This is populated by the package resolver and used by the typechecker for
    /// import resolution instead of re-parsing require statements.
    #[tracked]
    #[returns(ref)]
    pub resolved_requires: BTreeMap<ModuleId<'db>, Vec<(InternedText<'db>, ModuleId<'db>)>>,

    /// Recursive content hashes for each module.
    ///
    /// Each module's hash incorporates its source text and the content hashes of
    /// its resolved dependencies (sorted by alias for determinism). This enables
    /// verification that Salsa memoization is working correctly: if a module's
    /// content hash is unchanged, its typecheck result should be cached.
    #[tracked]
    #[returns(ref)]
    pub module_content_hashes: BTreeMap<ModuleId<'db>, u64>,

    /// Resolved rider interfaces per module.
    ///
    /// Maps each module to its resolved rider aliases: (alias, rider_interface).
    /// Populated by the compiler driver from `require rider` statements.
    #[tracked]
    #[returns(ref)]
    pub resolved_riders: BTreeMap<ModuleId<'db>, Vec<(InternedText<'db>, RiderInterface<'db>)>>,
}

impl<'db> ParsedModuleGraph<'db> {
    /// Get the parsed statements for a module by its ID.
    pub fn get_parsed(&self, db: &'db dyn Db, module_id: ModuleId<'db>) -> Option<ParsedStatements<'db>> {
        self.statements_only(db).iter()
            .find(|(id, _)| *id == module_id)
            .map(|(_, parsed)| parsed.clone())
    }

    /// Get the spans for a module by its ID.
    pub fn get_spans(&self, db: &'db dyn Db, module_id: ModuleId<'db>) -> Option<DatafunSpans> {
        self.spans(db).iter()
            .find(|(id, _)| *id == module_id)
            .map(|(_, spans)| spans.clone())
    }

    /// Get the resolved require aliases for a module.
    ///
    /// Returns a slice of (alias, target_module_id) pairs representing what
    /// `require module` statements in this module resolved to.
    pub fn get_requires(&self, db: &'db dyn Db, module_id: ModuleId<'db>) -> &[(InternedText<'db>, ModuleId<'db>)] {
        self.resolved_requires(db)
            .get(&module_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get the resolved rider interfaces for a module.
    ///
    /// Returns a slice of (alias, rider_interface) pairs representing what
    /// `require rider` statements in this module resolved to.
    pub fn get_riders(&self, db: &'db dyn Db, module_id: ModuleId<'db>) -> &[(InternedText<'db>, RiderInterface<'db>)] {
        self.resolved_riders(db)
            .get(&module_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

// ============================================================================
// Name Resolution Result Types
// ============================================================================

/// Result of name resolution for a single module.
///
/// Contains type aliases, function signatures, and function ASTs collected
/// from a module's parsed statements. This is computed before typechecking
/// and is memoized per-module via Salsa.
#[salsa::tracked]
pub struct ModuleNameResolution<'db> {
    /// Module this is for.
    #[returns(copy)]
    pub module_id: ModuleId<'db>,

    /// Type aliases defined in this module: (name, resolved_type).
    #[returns(ref)]
    pub type_aliases: Vec<(InternedText<'db>, Type<'db>)>,

    /// Function signatures: (name, function_type).
    #[returns(ref)]
    pub functions: Vec<(InternedText<'db>, TypeFunction<'db>)>,

    /// Function ASTs for inlining: (name, ast).
    #[returns(ref)]
    pub function_asts: Vec<(InternedText<'db>, StmtFun<'db>)>,

    /// Errors encountered during name resolution.
    #[returns(ref)]
    pub errors: Vec<TypeError>,
}

/// Aggregated name resolutions for all modules in a graph.
#[salsa::tracked]
pub struct AllModuleNameResolutions<'db> {
    /// Per-module name resolutions.
    #[returns(ref)]
    pub resolutions: BTreeMap<ModuleId<'db>, ModuleNameResolution<'db>>,
}

/// Collected names from statements (type aliases and optionally functions).
///
/// This is the core name resolution result used by both modules and scripts.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct CollectedNames<'db> {
    /// Type aliases: (name, resolved_type).
    pub type_aliases: Vec<(InternedText<'db>, Type<'db>)>,
    /// Function signatures: (name, function_type). Empty if `collect_functions` was false.
    pub functions: Vec<(InternedText<'db>, TypeFunction<'db>)>,
    /// Function ASTs for inlining: (name, ast). Empty if `collect_functions` was false.
    pub function_asts: Vec<(InternedText<'db>, StmtFun<'db>)>,
    /// Errors encountered during name resolution.
    pub errors: Vec<TypeError>,
}

/// All module exports collected from the graph.
#[salsa::tracked]
pub struct AllModuleExports<'db> {
    #[returns(ref)]
    pub exports: BTreeMap<ModuleId<'db>, Vec<(InternedText<'db>, TypeFunction<'db>)>>,
}

/// All function ASTs collected from the graph.
#[salsa::tracked]
pub struct AllModuleFunctionAsts<'db> {
    #[returns(ref)]
    pub asts: BTreeMap<ModuleId<'db>, Vec<(InternedText<'db>, StmtFun<'db>)>>,
}

/// Type representation for datafun (extends datalit types with function types).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum Type<'db> {
    /// Datalit type (primitives, collections, etc.).
    Datalit(datalove_datalit::tycheck::Type<'db>),
    /// Function type: (param_types) -> return_type.
    Function(TypeFunction<'db>),
}

/// Function type with parameter types and return type.
#[salsa::tracked]
pub struct TypeFunction<'db> {
    #[tracked]
    #[returns(ref)]
    pub param_types: Vec<Type<'db>>,
    #[returns(clone)]
    pub param_modes: Vec<ParamMode>,
    #[returns(ref)]
    pub param_comptime: Vec<bool>,
    #[tracked]
    #[returns(clone)]
    pub return_type: Type<'db>,
}

// ============================================================================
// Const Parameter Specialization Registry
// ============================================================================

/// A call site with const parameter arguments, recorded during typecheck.
///
/// Note: We record const binding *names*, not values. The values are looked up
/// later from ResolvedConsts during specialization (after const evaluation).
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ComptimeCallSite<'db> {
    /// Name of the called function.
    pub func_name: InternedText<'db>,
    /// Indices of const parameters in the callee.
    pub comptime_param_indices: Vec<usize>,
    /// Names of const bindings used as const parameter args (NOT values yet).
    pub comptime_arg_names: Vec<InternedText<'db>>,
}

/// Registry of const parameter call sites and functions, collected during typecheck.
///
/// Uses BTreeMap instead of HashMap to satisfy Hash/Eq requirements for Salsa tracking.
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ComptimeCallSiteRegistry<'db> {
    /// All call sites with const parameter args.
    pub call_sites: Vec<ComptimeCallSite<'db>>,
    /// Functions that have const parameters (name -> param indices).
    pub comptime_funcs: BTreeMap<InternedText<'db>, Vec<usize>>,
}

impl<'db> ComptimeCallSiteRegistry<'db> {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if there are any const parameter functions or call sites.
    pub fn is_empty(&self) -> bool {
        self.call_sites.is_empty() && self.comptime_funcs.is_empty()
    }

    /// Register a function with const parameters.
    pub fn register_comptime_func(&mut self, name: InternedText<'db>, comptime_indices: Vec<usize>) {
        self.comptime_funcs.entry(name).or_insert(comptime_indices);
    }

    /// Record a call site with const parameter arguments.
    pub fn record_call_site(&mut self, call_site: ComptimeCallSite<'db>) {
        self.call_sites.push(call_site);
    }

    /// Get const parameter indices for a function.
    pub fn get_comptime_indices(&self, func_name: InternedText<'db>) -> Option<&Vec<usize>> {
        self.comptime_funcs.get(&func_name)
    }

    /// Merge another registry into this one.
    pub fn merge(&mut self, other: &ComptimeCallSiteRegistry<'db>) {
        self.call_sites.extend(other.call_sites.iter().cloned());
        for (name, indices) in &other.comptime_funcs {
            self.comptime_funcs.entry(*name).or_insert_with(|| indices.clone());
        }
    }
}

/// Type error representation.
///
/// A field type that is `'static` needs no `SalsaValue`; these live in maps
/// keyed by `ModuleId`, which borrows the database, so they do.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum TypeError {
    TypeMismatch { expected: String, actual: String },
    UnresolvedName(String),
    CannotSynthesize,
    InvalidOperandType { op: String, ty: String },
    ArityMismatch { expected: usize, actual: usize },
    NotAFunction(String),
    DatalitError(String),
    ResultRequiresErrorBinding,
    TryTypeMismatch { operator: String, actual_type: String },
    TryReturnTypeMismatch { operator: String, return_type: String },
    IntOutOfRange,
    MissingField(String),
    ExtraField(String),
    FieldOrderMismatch,
    VariantNotFound(String),
    BreakOutsideLoop,
    ContinueOutsideLoop,
    /// Field index out of bounds for tuple.
    FieldIndexOutOfBounds { index: u32, tuple_size: usize },
    /// Named field not found in struct.
    FieldNotFound { field_name: String, ty: String },
    /// Projection on non-aggregate type.
    ProjectionOnNonAggregate { ty: String },
    /// Field projection on move-type field (not allowed outside ref context).
    NonCopyFieldProjection { field_ty: String },
    /// Index projection on move-type element (not allowed outside ref context).
    NonCopyIndexProjection { elem_ty: String },
    /// View type cannot be bound to mut/out parameter.
    /// Views alias parent data; whole-value replacement would leak or corrupt.
    ViewTypeMutBinding { view_ty: String },
    /// Void function returning a value.
    VoidFunctionReturnsValue,
    /// Non-void function with bare return (missing return value).
    FunctionRequiresReturnValue,
    /// Undefined variable reference.
    UndefinedVariable,
    /// Cannot assign to immutable variable.
    VariableNotMutable,
    /// Unresolved type alias (forward reference).
    UnresolvedTypeAlias(String),
    /// A type parameter sat somewhere erasure could not reach.
    TypeParamNotErasable(String),
    /// Duplicate type alias definition.
    DuplicateTypeAlias(String),
    /// Two imports bound the same name.
    ///
    /// The second used to overwrite the first without a word, which made a
    /// call resolve to whichever import came last.
    DuplicateImport { name: String, first: String, second: String },
    /// Cannot shadow primitive type name.
    CannotShadowPrimitive(String),
    /// A const expression referenced a binding that is not itself const.
    NonConstInConstExpr(String),
    /// A type parameter appeared inside a composite type.
    ///
    /// Erasure replaces the parameter itself with `data`, which works wherever
    /// the parameter stands alone. Inside a list or an option the element type
    /// would have to change too, and converting a caller's collection to match
    /// Call-site mode marker disagrees with the parameter's declared mode.
    ArgumentModeMismatch {
        param_idx: usize,
        expected: String,
        found: String,
    },
    /// Comptime argument must be a const binding name.
    ComptimeArgNotConstBinding {
        param_idx: usize,
        reason: String,
    },
}

impl From<datalove_datalit::tycheck::TypeError> for TypeError {
    fn from(err: datalove_datalit::tycheck::TypeError) -> Self {
        match err {
            datalove_datalit::tycheck::TypeError::TypeMismatch { expected, actual } => {
                TypeError::TypeMismatch { expected, actual }
            }
            datalove_datalit::tycheck::TypeError::CannotSynthesize => TypeError::CannotSynthesize,
            datalove_datalit::tycheck::TypeError::MissingField(name) => TypeError::MissingField(name),
            datalove_datalit::tycheck::TypeError::ExtraField(name) => TypeError::ExtraField(name),
            datalove_datalit::tycheck::TypeError::FieldOrderMismatch => TypeError::FieldOrderMismatch,
            datalove_datalit::tycheck::TypeError::IntOutOfRange => TypeError::IntOutOfRange,
            datalove_datalit::tycheck::TypeError::VariantNotFound(name) => TypeError::VariantNotFound(name),
            datalove_datalit::tycheck::TypeError::ArityMismatch { expected, actual } => {
                TypeError::ArityMismatch { expected, actual }
            }
        }
    }
}

// ============================================================================
// Re-exports from datalit
// ============================================================================

pub use datalit::tycheck::{
    TypeAnonTuple,
    TypeAnonStruct,
    TypeNamedField,

    TypeList,
    TypeMap,
    TypeSet,
    TypeOption,
    TypeResult,
    TypeTensor,
    TypeTable,

    TypeAtom,
    TypeTerm,
    TypeEnum,
    TypeEnumVariant,
};

// ============================================================================
// Type Predicates
// ============================================================================

/// Check if a type is numeric.
pub fn is_numeric_type(ty: &Type<'_>) -> bool {
    match ty {
        Type::Datalit(dt) => datalit::tycheck::is_numeric_type(dt),
        _ => false,
    }
}

/// Check if a type is a floating-point type.
pub fn is_float_type(ty: &Type<'_>) -> bool {
    match ty {
        Type::Datalit(dt) => datalit::tycheck::is_float_type(dt),
        _ => false,
    }
}

/// Check if a type is the arbitrary-precision integer type.
pub fn is_bigint_type(ty: &Type<'_>) -> bool {
    match ty {
        Type::Datalit(dt) => datalit::tycheck::is_bigint_type(dt),
        _ => false,
    }
}

/// Check if a type is a fixed-size integer type.
pub fn is_fixed_int_type(ty: &Type<'_>) -> bool {
    match ty {
        Type::Datalit(dt) => datalit::tycheck::is_fixed_int_type(dt),
        _ => false,
    }
}

/// Check if a type is an unsigned integer type.
pub fn is_unsigned_int_type(ty: &Type<'_>) -> bool {
    match ty {
        Type::Datalit(dt) => datalit::tycheck::is_unsigned_int_type(dt),
        _ => false,
    }
}

/// Check if a type is a signed fixed-size integer type (i8, i16, i32, i64).
pub fn is_signed_fixed_int_type(ty: &Type<'_>) -> bool {
    is_fixed_int_type(ty) && !is_unsigned_int_type(ty)
}

/// Check if the `@` operator can convert from one type to another.
///
/// The `@` operator performs lossless clone/coerce operations:
/// - Clone: for linear types where source and target are the same
/// - Widen: for fixed integers along signedness chains
/// - Cross-sign widen: for unsigned to larger signed (u8 -> i16, u16 -> i32, etc.)
/// - Both: when widening produces a linear type (e.g., to `int`)
///
/// This function returns true if the conversion is valid.
pub fn can_clone_coerce_to<'db>(from: &Type<'db>, to: &Type<'db>, db: &'db dyn Db) -> bool {
    // Same type: always valid (clone for linear, no-op for copy)
    if types_equivalent(db, from, to) {
        return true;
    }

    // Extract datalit types
    let (from_dt, to_dt) = match (from, to) {
        (Type::Datalit(from_dt), Type::Datalit(to_dt)) => (from_dt, to_dt),
        _ => return false,
    };

    // Check standard widening
    if datalit::tycheck::can_widen_to(from_dt, to_dt) {
        return true;
    }

    // Check cross-sign widening (unsigned to larger signed)
    // u8 -> i16, i32, i64, int
    // u16 -> i32, i64, int
    // u32 -> i64, int
    use datalit::tycheck::Type as DT;
    match (from_dt, to_dt) {
        (DT::U8, DT::I16 | DT::I32 | DT::I64 | DT::Int) => true,
        (DT::U16, DT::I32 | DT::I64 | DT::Int) => true,
        (DT::U32, DT::I64 | DT::Int) => true,
        // Atom -> Enum: atom is a variant of the enum (no payload).
        (DT::Atom(a), DT::Enum(e)) => {
            e.variants.iter().any(|v| v.name == a.name && v.payload.is_none())
        }
        // Term -> Enum: term is a variant of the enum (with matching payload type).
        (DT::Term(t), DT::Enum(e)) => {
            e.variants.iter().any(|v| {
                v.name == t.name
                    && v.payload.as_ref().map_or(false, |p| {
                        datalit::tycheck::types_equivalent(db, &t.payload, p)
                    })
            })
        }
        _ => false,
    }
}

/// Check if a type is boolean.
pub fn is_bool_type(ty: &Type<'_>) -> bool {
    match ty {
        Type::Datalit(dt) => datalit::tycheck::is_bool_type(dt),
        _ => false,
    }
}

// ============================================================================
// Type Equivalence
// ============================================================================

/// Check if two types are equivalent.
pub fn types_equivalent<'db>(db: &'db dyn Db, t1: &Type<'db>, t2: &Type<'db>) -> bool {
    match (t1, t2) {
        (Type::Datalit(dt1), Type::Datalit(dt2)) => datalit::tycheck::types_equivalent(db, dt1, dt2),
        (Type::Function(f1), Type::Function(f2)) => {
            f1.param_types(db).len() == f2.param_types(db).len()
                && f1.param_types(db).iter().zip(f2.param_types(db).iter())
                    .all(|(a, b)| types_equivalent(db, a, b))
                && types_equivalent(db, &f1.return_type(db), &f2.return_type(db))
        }
        _ => false,
    }
}

// ============================================================================
// Primitive Type Names
// ============================================================================

/// Check if a name is a primitive type name that cannot be shadowed.
pub fn is_primitive_name(name: &str) -> bool {
    matches!(name,
        "bool" | "u8" | "i8" | "u16" | "i16" | "u32" | "i32" | "u64" | "i64" |
        "index" | "offset" | "f32" | "f64" | "int" | "string" | "data" | "error" |
        "tuple" | "enum" | "map" | "set"
    )
}

// ============================================================================
// Type Hint Conversion
// ============================================================================

/// Convert a type hint to a type.
pub fn convert_type_hint<'db>(
    db: &'db dyn Db,
    type_hint: TypeHint<'db>,
) -> Result<Type<'db>, TypeError> {
    convert_type_hint_inner(db, type_hint)
}

fn convert_type_hint_inner<'db>(
    db: &'db dyn Db,
    type_hint: TypeHint<'db>,
) -> Result<Type<'db>, TypeError> {
    let ty = match type_hint {
        TypeHint::Bool => Type::Datalit(datalit::tycheck::Type::Bool),
        TypeHint::U8 => Type::Datalit(datalit::tycheck::Type::U8),
        TypeHint::I8 => Type::Datalit(datalit::tycheck::Type::I8),
        TypeHint::U16 => Type::Datalit(datalit::tycheck::Type::U16),
        TypeHint::I16 => Type::Datalit(datalit::tycheck::Type::I16),
        TypeHint::U32 => Type::Datalit(datalit::tycheck::Type::U32),
        TypeHint::I32 => Type::Datalit(datalit::tycheck::Type::I32),
        TypeHint::U64 => Type::Datalit(datalit::tycheck::Type::U64),
        TypeHint::I64 => Type::Datalit(datalit::tycheck::Type::I64),
        TypeHint::Index => Type::Datalit(datalit::tycheck::Type::Index),
        TypeHint::Offset => Type::Datalit(datalit::tycheck::Type::Offset),
        TypeHint::F32 => Type::Datalit(datalit::tycheck::Type::F32),
        TypeHint::F64 => Type::Datalit(datalit::tycheck::Type::F64),
        TypeHint::Int => Type::Datalit(datalit::tycheck::Type::Int),
        TypeHint::String => Type::Datalit(datalit::tycheck::Type::String),
        TypeHint::Data => Type::Datalit(datalit::tycheck::Type::Data),
        TypeHint::Error => Type::Datalit(datalit::tycheck::Type::Error),

        TypeHint::AnonTuple(t) => {
            let fields: Result<Vec<_>, TypeError> = t.fields.iter()
                .map(|f| {
                    let f_ty = convert_type_hint_inner(db, f.clone())?;
                    match f_ty {
                        Type::Datalit(dt) => Ok(dt.clone()),
                        Type::Function(_) => unreachable!("type hint cannot produce function type"),
                    }
                })
                .collect();
            Type::Datalit(datalit::tycheck::Type::AnonTuple(
                datalit::tycheck::TypeAnonTuple { fields: fields? }
            ))
        }

        TypeHint::AnonStruct(s) => {
            let fields: Result<Vec<_>, TypeError> = s.fields.iter()
                .map(|f| {
                    let name = f.name;
                    let f_ty = convert_type_hint_inner(db, (*f.type_hint).clone())?;
                    let dt = match f_ty {
                        Type::Datalit(dt) => dt.clone(),
                        Type::Function(_) => unreachable!("type hint cannot produce function type"),
                    };
                    Ok(datalit::tycheck::TypeNamedField {
                        name,
                        ty: Box::new(dt),
                    })
                })
                .collect();
            Type::Datalit(datalit::tycheck::Type::AnonStruct(
                datalit::tycheck::TypeAnonStruct { fields: fields? }
            ))
        }


        TypeHint::List(l) => {
            let elem_ty = convert_type_hint_inner(db, (*l.element_type).clone())?;
            let dt = match elem_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("type hint cannot produce function type"),
            };
            Type::Datalit(datalit::tycheck::Type::List(
                datalit::tycheck::TypeList {
                    element_type: Box::new(dt)
                }
            ))
        }

        TypeHint::Map(m) => {
            let key_ty = convert_type_hint_inner(db, (*m.key_type).clone())?;
            let key_dt = match key_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("type hint cannot produce function type"),
            };
            let value_ty = convert_type_hint_inner(db, (*m.value_type).clone())?;
            let value_dt = match value_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("type hint cannot produce function type"),
            };
            Type::Datalit(datalit::tycheck::Type::Map(
                datalit::tycheck::TypeMap {
                    key_type: Box::new(key_dt),
                    value_type: Box::new(value_dt),
                }
            ))
        }

        TypeHint::Set(s) => {
            let elem_ty = convert_type_hint_inner(db, (*s.element_type).clone())?;
            let dt = match elem_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("type hint cannot produce function type"),
            };
            Type::Datalit(datalit::tycheck::Type::Set(
                datalit::tycheck::TypeSet {
                    element_type: Box::new(dt)
                }
            ))
        }

        TypeHint::Option(o) => {
            let inner_ty = convert_type_hint_inner(db, (*o.inner_type).clone())?;
            let dt = match inner_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("type hint cannot produce function type"),
            };
            Type::Datalit(datalit::tycheck::Type::Option(
                datalit::tycheck::TypeOption {
                    inner_type: Box::new(dt)
                }
            ))
        }

        TypeHint::Result(r) => {
            let inner_ty = convert_type_hint_inner(db, (*r.inner_type).clone())?;
            let dt = match inner_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("type hint cannot produce function type"),
            };
            Type::Datalit(datalit::tycheck::Type::Result(
                datalit::tycheck::TypeResult {
                    inner_type: Box::new(dt)
                }
            ))
        }

        TypeHint::Tensor(t) => {
            let elem_ty = convert_type_hint_inner(db, (*t.element_type).clone())?;
            let dt = match elem_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("type hint cannot produce function type"),
            };
            Type::Datalit(datalit::tycheck::Type::Tensor(
                datalit::tycheck::TypeTensor {
                    element_type: Box::new(dt),
                    rank: t.rank,
                }
            ))
        }

        TypeHint::Atom(a) => {
            Type::Datalit(datalit::tycheck::Type::Atom(
                datalit::tycheck::TypeAtom { name: a.name }
            ))
        }

        TypeHint::Term(t) => {
            let payload_ty = convert_type_hint_inner(db, (*t.payload).clone())?;
            let dt = match payload_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("type hint cannot produce function type"),
            };
            Type::Datalit(datalit::tycheck::Type::Term(
                datalit::tycheck::TypeTerm { name: t.name, payload: Box::new(dt) }
            ))
        }

        TypeHint::Enum(e) => {
            let variants: Result<Vec<_>, TypeError> = e.variants.iter()
                .map(|v| {
                    let payload = match &v.payload {
                        Some(p) => {
                            let p_ty = convert_type_hint_inner(db, (**p).clone())?;
                            let dt = match p_ty {
                                Type::Datalit(dt) => dt.clone(),
                                Type::Function(_) => unreachable!("type hint cannot produce function type"),
                            };
                            Some(Box::new(dt))
                        }
                        None => None,
                    };
                    Ok(datalit::tycheck::TypeEnumVariant { name: v.name, payload })
                })
                .collect();
            let mut variants = variants?;
            variants.sort_by(|a, b| a.name.as_str(db).cmp(b.name.as_str(db)));
            Type::Datalit(datalit::tycheck::Type::Enum(
                datalit::tycheck::TypeEnum { variants }
            ))
        }

        TypeHint::ParseError(_) => return Err(TypeError::CannotSynthesize),
        TypeHint::Alias(name) => {
            // Type alias cannot be resolved without alias map.
            return Err(TypeError::UnresolvedTypeAlias(name.as_str(db).to_string()));
        }
        TypeHint::Table(t) => {
            let columns: Result<Vec<_>, TypeError> = t.columns.iter()
                .map(|c| {
                    let name = c.name;
                    let c_ty = convert_type_hint_inner(db, (*c.type_hint).clone())?;
                    let dt = match c_ty {
                        Type::Datalit(dt) => dt.clone(),
                        Type::Function(_) => unreachable!("type hint cannot produce function type"),
                    };
                    Ok(datalit::tycheck::TypeNamedField {
                        name,
                        ty: Box::new(dt),
                    })
                })
                .collect();
            Type::Datalit(datalit::tycheck::Type::Table(
                datalit::tycheck::TypeTable { columns: columns? }
            ))
        }
    };

    Ok(ty)
}

/// Convert a type hint to a type, resolving type aliases.
pub fn convert_type_hint_with_aliases<'db>(
    db: &'db dyn Db,
    type_hint: TypeHint<'db>,
    aliases: &HashMap<InternedText<'db>, Type<'db>>,
) -> Result<Type<'db>, TypeError> {
    convert_type_hint_with_aliases_inner(db, type_hint, aliases)
}

fn convert_type_hint_with_aliases_inner<'db>(
    db: &'db dyn Db,
    type_hint: TypeHint<'db>,
    aliases: &HashMap<InternedText<'db>, Type<'db>>,
) -> Result<Type<'db>, TypeError> {
    let ty = match type_hint {
        TypeHint::Bool => Type::Datalit(datalit::tycheck::Type::Bool),
        TypeHint::U8 => Type::Datalit(datalit::tycheck::Type::U8),
        TypeHint::I8 => Type::Datalit(datalit::tycheck::Type::I8),
        TypeHint::U16 => Type::Datalit(datalit::tycheck::Type::U16),
        TypeHint::I16 => Type::Datalit(datalit::tycheck::Type::I16),
        TypeHint::U32 => Type::Datalit(datalit::tycheck::Type::U32),
        TypeHint::I32 => Type::Datalit(datalit::tycheck::Type::I32),
        TypeHint::U64 => Type::Datalit(datalit::tycheck::Type::U64),
        TypeHint::I64 => Type::Datalit(datalit::tycheck::Type::I64),
        TypeHint::Index => Type::Datalit(datalit::tycheck::Type::Index),
        TypeHint::Offset => Type::Datalit(datalit::tycheck::Type::Offset),
        TypeHint::F32 => Type::Datalit(datalit::tycheck::Type::F32),
        TypeHint::F64 => Type::Datalit(datalit::tycheck::Type::F64),
        TypeHint::Int => Type::Datalit(datalit::tycheck::Type::Int),
        TypeHint::String => Type::Datalit(datalit::tycheck::Type::String),
        TypeHint::Data => Type::Datalit(datalit::tycheck::Type::Data),
        TypeHint::Error => Type::Datalit(datalit::tycheck::Type::Error),

        TypeHint::AnonTuple(t) => {
            let fields: Result<Vec<_>, TypeError> = t.fields.iter()
                .map(|f| {
                    let f_ty = convert_type_hint_with_aliases_inner(db, f.clone(), aliases)?;
                    match f_ty {
                        Type::Datalit(dt) => Ok(dt.clone()),
                        Type::Function(_) => unreachable!("type hint cannot produce function type"),
                    }
                })
                .collect();
            Type::Datalit(datalit::tycheck::Type::AnonTuple(
                datalit::tycheck::TypeAnonTuple { fields: fields? }
            ))
        }

        TypeHint::AnonStruct(s) => {
            let fields: Result<Vec<_>, TypeError> = s.fields.iter()
                .map(|f| {
                    let name = f.name;
                    let f_ty = convert_type_hint_with_aliases_inner(db, (*f.type_hint).clone(), aliases)?;
                    let dt = match f_ty {
                        Type::Datalit(dt) => dt.clone(),
                        Type::Function(_) => unreachable!("type hint cannot produce function type"),
                    };
                    Ok(datalit::tycheck::TypeNamedField {
                        name,
                        ty: Box::new(dt),
                    })
                })
                .collect();
            Type::Datalit(datalit::tycheck::Type::AnonStruct(
                datalit::tycheck::TypeAnonStruct { fields: fields? }
            ))
        }


        TypeHint::List(l) => {
            let elem_ty = convert_type_hint_with_aliases_inner(db, (*l.element_type).clone(), aliases)?;
            let dt = match elem_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("type hint cannot produce function type"),
            };
            Type::Datalit(datalit::tycheck::Type::List(
                datalit::tycheck::TypeList {
                    element_type: Box::new(dt)
                }
            ))
        }

        TypeHint::Map(m) => {
            let key_ty = convert_type_hint_with_aliases_inner(db, (*m.key_type).clone(), aliases)?;
            let key_dt = match key_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("type hint cannot produce function type"),
            };
            let value_ty = convert_type_hint_with_aliases_inner(db, (*m.value_type).clone(), aliases)?;
            let value_dt = match value_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("type hint cannot produce function type"),
            };
            Type::Datalit(datalit::tycheck::Type::Map(
                datalit::tycheck::TypeMap {
                    key_type: Box::new(key_dt),
                    value_type: Box::new(value_dt),
                }
            ))
        }

        TypeHint::Set(s) => {
            let elem_ty = convert_type_hint_with_aliases_inner(db, (*s.element_type).clone(), aliases)?;
            let dt = match elem_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("type hint cannot produce function type"),
            };
            Type::Datalit(datalit::tycheck::Type::Set(
                datalit::tycheck::TypeSet {
                    element_type: Box::new(dt)
                }
            ))
        }

        TypeHint::Option(o) => {
            let inner_ty = convert_type_hint_with_aliases_inner(db, (*o.inner_type).clone(), aliases)?;
            let dt = match inner_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("type hint cannot produce function type"),
            };
            Type::Datalit(datalit::tycheck::Type::Option(
                datalit::tycheck::TypeOption {
                    inner_type: Box::new(dt)
                }
            ))
        }

        TypeHint::Result(r) => {
            let inner_ty = convert_type_hint_with_aliases_inner(db, (*r.inner_type).clone(), aliases)?;
            let dt = match inner_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("type hint cannot produce function type"),
            };
            Type::Datalit(datalit::tycheck::Type::Result(
                datalit::tycheck::TypeResult {
                    inner_type: Box::new(dt)
                }
            ))
        }

        TypeHint::Tensor(t) => {
            let elem_ty = convert_type_hint_with_aliases_inner(db, (*t.element_type).clone(), aliases)?;
            let dt = match elem_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("type hint cannot produce function type"),
            };
            Type::Datalit(datalit::tycheck::Type::Tensor(
                datalit::tycheck::TypeTensor {
                    element_type: Box::new(dt),
                    rank: t.rank,
                }
            ))
        }

        TypeHint::Atom(a) => {
            Type::Datalit(datalit::tycheck::Type::Atom(
                datalit::tycheck::TypeAtom { name: a.name }
            ))
        }

        TypeHint::Term(t) => {
            let payload_ty = convert_type_hint_with_aliases_inner(db, (*t.payload).clone(), aliases)?;
            let dt = match payload_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("type hint cannot produce function type"),
            };
            Type::Datalit(datalit::tycheck::Type::Term(
                datalit::tycheck::TypeTerm { name: t.name, payload: Box::new(dt) }
            ))
        }

        TypeHint::Enum(e) => {
            let variants: Result<Vec<_>, TypeError> = e.variants.iter()
                .map(|v| {
                    let payload = match &v.payload {
                        Some(p) => {
                            let p_ty = convert_type_hint_with_aliases_inner(db, (**p).clone(), aliases)?;
                            let dt = match p_ty {
                                Type::Datalit(dt) => dt.clone(),
                                Type::Function(_) => unreachable!("type hint cannot produce function type"),
                            };
                            Some(Box::new(dt))
                        }
                        None => None,
                    };
                    Ok(datalit::tycheck::TypeEnumVariant { name: v.name, payload })
                })
                .collect();
            let mut variants = variants?;
            variants.sort_by(|a, b| a.name.as_str(db).cmp(b.name.as_str(db)));
            Type::Datalit(datalit::tycheck::Type::Enum(
                datalit::tycheck::TypeEnum { variants }
            ))
        }

        TypeHint::ParseError(_) => return Err(TypeError::CannotSynthesize),

        TypeHint::Alias(name) => {
            // Look up the alias in the map.
            if let Some(resolved_ty) = aliases.get(&name) {
                return Ok(resolved_ty.clone());
            }
            return Err(TypeError::UnresolvedTypeAlias(name.as_str(db).to_string()));
        }

        TypeHint::Table(t) => {
            let columns: Result<Vec<_>, TypeError> = t.columns.iter()
                .map(|c| {
                    let name = c.name;
                    let c_ty = convert_type_hint_with_aliases_inner(db, (*c.type_hint).clone(), aliases)?;
                    let dt = match c_ty {
                        Type::Datalit(dt) => dt.clone(),
                        Type::Function(_) => unreachable!("type hint cannot produce function type"),
                    };
                    Ok(datalit::tycheck::TypeNamedField {
                        name,
                        ty: Box::new(dt),
                    })
                })
                .collect();
            Type::Datalit(datalit::tycheck::Type::Table(
                datalit::tycheck::TypeTable { columns: columns? }
            ))
        }
    };

    Ok(ty)
}

// ============================================================================
// Type to String
// ============================================================================

/// Convert a type to a string for error messages.
pub fn type_to_string<'db>(db: &'db dyn Db, ty: &Type<'db>) -> String {
    match ty {
        Type::Datalit(dt) => datalit::tycheck::type_to_string(db, dt),
        Type::Function(f) => {
            let params: Vec<_> = f.param_types(db).iter()
                .map(|p| type_to_string(db, p))
                .collect();
            let ret = type_to_string(db, &f.return_type(db));
            format!("fn({}) -> {}", params.join(", "), ret)
        }
    }
}

// ============================================================================
// Unit Type Helper
// ============================================================================

/// Create the unit type `()`.
pub fn unit_type<'db>(_db: &'db dyn Db) -> Type<'db> {
    let datalit_unit = datalit::tycheck::unit_type();
    Type::Datalit(datalit_unit)
}

// ============================================================================
// Element Compatibility
// ============================================================================

/// Check that an element type is compatible with the expected element type.
pub fn check_element_compatible<'db>(
    db: &'db dyn Db,
    expected: &Type<'db>,
    actual: &Type<'db>,
) -> Result<(), TypeError> {
    if !types_equivalent(db, expected, actual) {
        return Err(TypeError::TypeMismatch {
            expected: type_to_string(db, expected),
            actual: type_to_string(db, actual),
        });
    }
    Ok(())
}

// ============================================================================
// Integer Range Checking
// ============================================================================

/// Check if an integer value fits within a type (delegated to datalit).
pub fn check_int_fits_wrapped_type<'db>(
    value_str: &str,
    ty: &datalit::tycheck::Type<'db>,
    _db: &'db dyn Db,
) -> Result<(), TypeError> {
    datalit::tycheck::check_int_fits_wrapped_type(value_str, ty)
        .map_err(TypeError::from)
}

/// Check if a hex value fits within a type (delegated to datalit).
pub fn check_hex_fits_wrapped_type<'db>(
    value_str: &str,
    ty: &datalit::tycheck::Type<'db>,
    _db: &'db dyn Db,
) -> Result<(), TypeError> {
    datalit::tycheck::check_hex_fits_wrapped_type(value_str, ty)
        .map_err(TypeError::from)
}

// ============================================================================
// Wrapper Unwrapping
// ============================================================================

/// Unwrap Option/Result wrappers to get the innermost type.
pub fn unwrap_wrapper_types<'db>(
    _db: &'db dyn Db,
    ty: &Type<'db>,
) -> Type<'db> {
    match ty {
        Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
            Type::Datalit(*opt.inner_type.clone())
        }
        Type::Datalit(datalit::tycheck::Type::Result(res)) => {
            Type::Datalit(*res.inner_type.clone())
        }
        _ => ty.clone(),
    }
}

// ============================================================================
// Type Conversion Helpers
// ============================================================================

/// Convert datafun Type to datalit Type.
pub fn to_datalit_type<'db>(
    _db: &'db dyn Db,
    ty: Type<'db>,
) -> Result<Type<'db>, TypeError> {
    match ty {
        Type::Datalit(dt) => Ok(Type::Datalit(dt.clone())),
        Type::Function(_) => Err(TypeError::CannotSynthesize),
    }
}
