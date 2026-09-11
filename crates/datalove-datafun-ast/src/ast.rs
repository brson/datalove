use rmx::prelude::*;

use std::fmt;

use bct::module_graph::ModuleId;
use bct::text::{InternedText, Text};
use bct::text::ByteSpan;
use bct::diagnostic::SpanEntry;
use datalove_datalit as datalit;

/// Stable identity of an expression.
///
/// These are the identity keys salsa gives `ExprFun`, held as a plain value.
/// A salsa `Id` is not usable for this: its generation counter changes when
/// salsa recycles a slot, and an id renumbers between salsa releases, so
/// neither side of a lookup can rely on one staying put.
///
/// The module has to be part of it. Within one module a function name and an
/// index identify an expression, but the type table is merged across a whole
/// graph, and every module numbers its expressions from zero.
#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[derive(salsa::SalsaValue)]
pub struct ExprKey<'db> {
    /// Module the expression belongs to, or None for a script.
    pub module_id: Option<ModuleId<'db>>,
    /// Function the expression belongs to, or None at script level.
    pub fn_name: Option<InternedText<'db>>,
    /// Sequential index within that function.
    pub local_index: u32,
}

/// Print the key without the function name's salsa id.
///
/// `InternedText` debug-prints the id behind it, and reading the text needs a
/// database this does not have. Printing the id would put salsa's numbering
/// back into error messages and expected test output, which is what keying on
/// `ExprKey` exists to avoid.
impl fmt::Debug for ExprKey<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.fn_name {
            Some(_) => write!(f, "ExprKey(fn #{})", self.local_index),
            None => write!(f, "ExprKey(script #{})", self.local_index),
        }
    }
}

impl<'db> ExprKey<'db> {
    pub fn new(
        module_id: Option<ModuleId<'db>>,
        fn_name: Option<InternedText<'db>>,
        local_index: u32,
    ) -> Self {
        ExprKey { module_id, fn_name, local_index }
    }

    /// The key identifying `expr`.
    pub fn of(db: &'db dyn salsa::Database, expr: ExprFun<'db>) -> Self {
        ExprKey::new(expr.module_id(db), expr.fn_name(db), expr.local_index(db))
    }

    /// The key identifying `call`.
    ///
    /// Calls are numbered separately from expressions, so these keys only make
    /// sense against a table of calls.
    pub fn of_call(db: &'db dyn salsa::Database, call: ExprFunctionCall<'db>) -> Self {
        ExprKey::new(call.module_id(db), call.fn_name(db), call.local_index(db))
    }
}

/// Span entry for a parsed expression.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ParseSpanEntry<'db> {
    pub expr_key: ExprKey<'db>,
    pub source: bct::input::Source,
    pub span: ByteSpan,
}

impl<'db> ParseSpanEntry<'db> {
    pub fn new(expr_key: ExprKey<'db>, source: bct::input::Source, span: ByteSpan) -> Self {
        ParseSpanEntry { expr_key, source, span }
    }
}

/// Result of parsing a source text into statements.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ParsedStatements<'db> {
    pub statements: Vec<Statement<'db>>,
}

/// Parse result containing parsed statements and span side tables.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ParseResult<'db> {
    pub parsed: ParsedStatements<'db>,
    pub expr_spans: Vec<ParseSpanEntry<'db>>,
    /// Break statement spans, indexed by local_index.
    pub break_spans: Vec<SpanEntry>,
    /// Continue statement spans, indexed by local_index.
    pub continue_spans: Vec<SpanEntry>,
    /// Return statement spans, indexed by local_index.
    pub ret_spans: Vec<SpanEntry>,
    /// Set statement spans, indexed by local_index.
    pub set_spans: Vec<SpanEntry>,
    /// Function definition spans, indexed by local_index.
    pub fun_spans: Vec<SpanEntry>,
    /// Type alias spans, indexed by local_index.
    pub type_alias_spans: Vec<SpanEntry>,
    /// Import statement spans, indexed by position among the imports.
    pub import_spans: Vec<SpanEntry>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum Statement<'db> {
    Let(StmtLet<'db>),
    Var(StmtVar<'db>),
    Const(StmtConst<'db>),
    Set(StmtSet<'db>),
    Fun(StmtFun<'db>),
    Ret(StmtRet<'db>),
    Require(StmtRequire<'db>),
    Import(StmtImport<'db>),
    If(StmtIf<'db>),
    Loop(StmtLoop<'db>),
    Break(StmtBreak),
    Continue(StmtContinue),
    DebugLog(StmtDebugLog<'db>),
    TypeAlias(StmtTypeAlias<'db>),
    NativeFun(StmtNativeFun<'db>),
    Match(StmtMatch<'db>),
    ExprStatement(StmtExprStatement<'db>),
    ParseError(StmtParseError<'db>),
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtLet<'db> {
    pub name: InternedText<'db>,
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub value: ExprFun<'db>,
}

/// Mutable variable declaration.
///
/// If `value` is None, the variable is declared but not initialized.
/// A type hint is required when there is no initializer.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtVar<'db> {
    pub name: InternedText<'db>,
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    /// The initial value. None for uninitialized declarations like `var x: i32`.
    pub value: Option<ExprFun<'db>>,
}

/// Compile-time constant binding.
///
/// The value expression is evaluated at compile time via the const evaluator.
/// The resulting value is inlined at use sites.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtConst<'db> {
    pub name: InternedText<'db>,
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub value: ExprFun<'db>,
}

/// Mutation of an existing mutable variable or field.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtSet<'db> {
    pub target: Place<'db>,
    pub value: ExprFun<'db>,
    /// Index for span lookup in DatafunSpans.
    pub local_index: u32,
}

/// A place: a root variable plus navigation steps to a storage location.
///
/// All variable references are Places. A bare variable `x` is
/// `Place { root: "x", steps: [] }`. Field chains like `x.a.b` have
/// Field steps. Index operations like `a[i]?` have Index steps.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct Place<'db> {
    pub root: InternedText<'db>,
    pub steps: Vec<PlaceStep<'db>>,
}

/// A navigation step within a place.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum PlaceStep<'db> {
    /// Field or tuple-element projection: `.field` or `.0`.
    Field(FieldSelector<'db>),
    /// Index operation: `[expr]` with optional error mode.
    ///
    /// `error_mode` is `None` for bare index (map upsert in set context),
    /// `Some(Option)` for `?`, `Some(Result)` for `!`.
    Index(PlaceIndex<'db>),
}

/// Index step in a place.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct PlaceIndex<'db> {
    pub index: ExprFun<'db>,
    pub error_mode: Option<IndexErrorMode>,
}

/// How index failure is handled.
#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
#[derive(salsa::SalsaValue)]
pub enum IndexErrorMode {
    /// `?` — early-return none on failure.
    Option,
    /// `!` — early-return error on failure.
    Result,
}

#[salsa::tracked]
pub struct StmtFun<'db> {
    /// Module this function belongs to (identity key).
    /// None for script-local functions.
    #[returns(copy)]
    pub module_id: Option<ModuleId<'db>>,
    /// Function name (identity key).
    #[returns(copy)]
    pub name: InternedText<'db>,
    /// Type parameters, in declaration order.
    ///
    /// A parameter or return type may name one, and it stands for whatever type
    /// the call site supplies. Empty for a function written without `<...>`.
    #[tracked]
    #[returns(ref)]
    pub type_params: Vec<InternedText<'db>>,
    /// The bound each type parameter was written with, by the same index.
    ///
    /// Built beside `type_params` from one parse, so the two cannot disagree
    /// about how many there are.
    #[tracked]
    #[returns(ref)]
    pub type_bounds: Vec<Option<TypeBound>>,
    #[tracked]
    #[returns(ref)]
    pub params: Vec<FunParam<'db>>,
    #[tracked]
    #[returns(clone)]
    pub return_type: Option<datalit::ast::TypeHint<'db>>,
    #[tracked]
    #[returns(ref)]
    pub body: Vec<Statement<'db>>,
    /// Index for span lookup in DatafunSpans.fun_spans.
    #[returns(copy)]
    pub local_index: u32,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct FunParam<'db> {
    pub name: InternedText<'db>,
    pub mode: ParamMode,
    pub is_comptime: bool,
    pub type_hint: datalit::ast::TypeHint<'db>,
}

#[derive(Copy, Clone, Hash, Debug, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum ParamMode {
    In,   // by-val (default)
    Out,  // by-mut-ptr
    Ref,  // by-ref
    Mut,  // by-mut-ref
}

/// Return statement. Value is None for bare `ret` in void functions.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtRet<'db> {
    pub value: Option<ExprFun<'db>>,
    /// Index for span lookup in DatafunSpans.
    pub local_index: u32,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum StmtRequire<'db> {
    Module(StmtRequireModule<'db>),
    Data(StmtRequireData<'db>),
    Rider(StmtRequireRider<'db>),
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtRequireModule<'db> {
    pub import_space: InternedText<'db>,
    pub package_alias: InternedText<'db>,
    pub module_alias: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtRequireData<'db> {
    pub name: InternedText<'db>,
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
}

/// Rider requirement: `require rider <name>`.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtRequireRider<'db> {
    pub name: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtImport<'db> {
    pub module_name: InternedText<'db>,
    pub item_name: InternedText<'db>,
    pub local_index: u32,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtIf<'db> {
    pub condition: ExprFun<'db>,
    pub then_binding: Option<InternedText<'db>>,
    pub then_body: Vec<Statement<'db>>,
    pub else_binding: Option<InternedText<'db>>,
    pub else_body: Option<Vec<Statement<'db>>>,
}

/// Loop statement with optional while condition.
///
/// Basic: `loop ... end loop`
/// With while: `loop while cond ... end loop`
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtLoop<'db> {
    /// Optional while condition, checked at the start of each iteration.
    pub condition: Option<ExprFun<'db>>,
    pub body: Vec<Statement<'db>>,
}

/// Break statement for exiting the innermost loop.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtBreak {
    /// Index for span lookup in DatafunSpans.
    pub local_index: u32,
}

/// Continue statement for skipping to the next iteration.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtContinue {
    /// Index for span lookup in DatafunSpans.
    pub local_index: u32,
}

/// Debug log statement for outputting values during execution.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtDebugLog<'db> {
    pub value: ExprFun<'db>,
}

/// Type alias statement: `type Name: structural_type`.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtTypeAlias<'db> {
    pub name: InternedText<'db>,
    pub type_hint: datalit::ast::TypeHint<'db>,
    pub local_index: u32,
}

/// What a type parameter was constrained to.
///
/// A parameter written bare stands for any type at all, and nothing can be
/// done to a value of it but move, drop, clone, print and hand it on. A bound
/// says which types it may be, and in exchange the body may do what all of
/// those have in common.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum TypeBound {
    /// `f32` or `f64`.
    Float,
    /// Any type at all, which is to say every type has a total order.
    ///
    /// It is the weakest bound there is, and says only what the runtime can
    /// already do for any value: order it against another of its type. What it
    /// rules out is a type with no ordering, of which the language has none
    /// today -- a function value would be the first.
    Ord,
    /// Any of the ten fixed-width integers, signed or unsigned.
    ///
    /// What they share is the comparisons and the checked and optional
    /// arithmetic. Bare `+` is not among them, because a fixed-width integer
    /// does not have one; neither is negation, because half of these have no
    /// negative values to give.
    FixedInt,
}

impl TypeBound {
    /// The bound a name in `T is name` stands for, if it names one.
    pub fn from_name(name: &str) -> Option<TypeBound> {
        match name {
            "float" => Some(TypeBound::Float),
            "fixedint" => Some(TypeBound::FixedInt),
            "ord" => Some(TypeBound::Ord),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TypeBound::Float => "float",
            TypeBound::FixedInt => "fixedint",
            TypeBound::Ord => "ord",
        }
    }
}

/// Native function declaration: `native fun name(params): ret_type`.
///
/// Declares a function signature without a body. Used in `.dli` rider
/// interface files to describe functions implemented in Rust.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtNativeFun<'db> {
    pub name: InternedText<'db>,
    pub type_params: Vec<InternedText<'db>>,
    /// See `StmtFun::type_bounds`.
    pub type_bounds: Vec<Option<TypeBound>>,
    pub params: Vec<FunParam<'db>>,
    pub return_type: Option<datalit::ast::TypeHint<'db>>,
}

/// Expression statement for void function calls.
///
/// Allows calling unit-returning functions at statement position.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtExprStatement<'db> {
    pub expr: ExprFun<'db>,
}

/// Match statement for exhaustive enum destructuring.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtMatch<'db> {
    pub input: ExprFun<'db>,
    pub cases: Vec<MatchCase<'db>>,
    pub default_body: Option<Vec<Statement<'db>>>,
}

/// A single case arm in a match statement.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct MatchCase<'db> {
    pub kind: MatchCaseKind<'db>,
    pub body: Vec<Statement<'db>>,
}

/// Kind of match case: atom (no binding) or term (with binding).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum MatchCaseKind<'db> {
    Atom { name: InternedText<'db> },
    Term { name: InternedText<'db>, binding: InternedText<'db> },
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct StmtParseError<'db> {
    pub text: Text<'db>,
    pub span: ByteSpan,
    pub message: InternedText<'db>,
}

// Datafun expressions - wraps datalit expressions and adds datafun-specific variants
#[salsa::tracked]
pub struct ExprFun<'db> {
    /// Module this expression belongs to (identity key). None for scripts.
    #[returns(copy)]
    pub module_id: Option<ModuleId<'db>>,
    /// Function this expression belongs to (identity key). None for script-level.
    #[returns(copy)]
    pub fn_name: Option<InternedText<'db>>,
    /// Sequential index within the function (identity key).
    #[returns(copy)]
    pub local_index: u32,
    #[tracked]
    #[returns(clone)]
    pub expr: ExprFunKind<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum ExprFunKind<'db> {
    // Binary operation
    BinOp(ExprBinOp<'db>),
    // Function call
    FunctionCall(ExprFunctionCall<'db>),
    // Datafun tuple (elements are datafun expressions)
    Tuple(ExprTuple<'db>),
    // Unary operators
    UnaryOp(ExprUnaryOp<'db>),
    // Try operators (postfix ? and !)
    TryOption(ExprTryOption<'db>),
    TryResult(ExprTryResult<'db>),
    // Clone/coerce operator (postfix @)
    CloneCoerce(ExprCloneCoerce<'db>),
    // Field projection (postfix .field or .0)
    FieldProj(ExprFieldProj<'db>),

    // Literal expressions (formerly delegated to datalit).
    True(ExprLit<'db>),
    False(ExprLit<'db>),
    None(ExprLit<'db>),
    Int(ExprInt<'db>),
    Float(ExprFloat<'db>),
    Hex(ExprHex<'db>),
    String(ExprString<'db>),

    // Collection expressions.
    List(ExprList<'db>),
    Set(ExprSet<'db>),
    Map(ExprMap<'db>),
    Tensor(ExprTensor<'db>),

    // Aggregate expressions.
    AnonTuple(ExprAnonTuple<'db>),
    AnonStruct(ExprAnonStruct<'db>),

    // Wrapper expressions.
    Some(ExprSome<'db>),
    Ok(ExprOk<'db>),
    Er(ExprEr<'db>),
    Data(ExprData<'db>),
    Error(ExprError<'db>),

    // Table expression.
    Table(ExprTable<'db>),

    // Atom expression: `atom Foo`.
    Atom(ExprAtom<'db>),
    // Term expression: `term Foo payload`.
    Term(ExprTerm<'db>),
    // Enum literal expression: `enum { atom Foo }` (checking-only).
    EnumLiteral(ExprEnumLiteral<'db>),

    // Index expression: base[index].
    Index(ExprIndex<'db>),

    /// Place expression with index steps (e.g., `a[i]?`, `a[i]?.field`).
    Place(Place<'db>),

    // Parse error
    ParseError(ExprFunParseError<'db>),

    // Intrinsic call (icall name(args)).
    IntrinsicCall(ExprIntrinsicCall<'db>),
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprBinOp<'db> {
    pub op: BinOp,
    pub lhs: ExprFun<'db>,
    pub rhs: ExprFun<'db>,
}

#[salsa::tracked]
pub struct ExprFunctionCall<'db> {
    /// Module this call belongs to (identity key). None for scripts.
    #[returns(copy)]
    pub module_id: Option<ModuleId<'db>>,
    /// Function this call belongs to (identity key). None for script-level.
    #[returns(copy)]
    pub fn_name: Option<InternedText<'db>>,
    /// Sequential index within the function (identity key).
    #[returns(copy)]
    pub local_index: u32,
    #[tracked]
    #[returns(copy)]
    pub name: InternedText<'db>,
    #[tracked]
    #[returns(ref)]
    pub args: Vec<ExprFun<'db>>,
    /// Mode marker written before each argument, parallel to `args`.
    ///
    /// `None` where the argument carried no marker, which denotes `in`. The
    /// marker must agree with the callee's declared mode; see F050.
    #[tracked]
    #[returns(ref)]
    pub arg_modes: Vec<Option<ParamMode>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprTuple<'db> {
    pub elements: Vec<ExprFun<'db>>,
}

#[derive(Copy, Clone, Hash, Debug, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum BinOp {
    // Basic arithmetic (no suffix)
    Add,    // +
    Sub,    // -
    Mul,    // *
    Div,    // /

    // Checked arithmetic (! suffix, error propagation)
    AddChecked,  // +!
    SubChecked,  // -!
    MulChecked,  // *!
    DivChecked,  // /!

    // Optional arithmetic (? suffix, returns Option)
    AddOptional,  // +?
    SubOptional,  // -?
    MulOptional,  // *?
    DivOptional,  // /?

    // Comparison operators
    Lt,  // .<
    Gt,  // .>
    Le,  // <=
    Ge,  // >=
    Eq,  // ==
    Ne,  // !=

    // Logical operators (boolean)
    And,  // and
    Or,   // or
    Xor,  // xor
}

#[derive(Copy, Clone, Hash, Debug, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum UnaryOp {
    Neg,          // - (bare, for bigints)
    NegOptional,  // -?
    NegResult,    // -!
    Not,          // not (boolean)
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprUnaryOp<'db> {
    pub op: UnaryOp,
    pub operand: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprTryOption<'db> {
    pub operand: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprTryResult<'db> {
    pub operand: ExprFun<'db>,
}

/// Clone/coerce expression (postfix @).
///
/// Performs lossless conversions:
/// - Clone: for linear types, creates a copy so the original remains valid
/// - Widen: for fixed integers, widens along signedness chains
/// - Both: when widening produces a linear type (e.g., to `int`)
///
/// Target type is inferred from context (assignment, parameter, binary op).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprCloneCoerce<'db> {
    pub operand: ExprFun<'db>,
}

/// Field projection expression (a.x or a.0).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprFieldProj<'db> {
    pub base: ExprFun<'db>,
    pub field: FieldSelector<'db>,
}

/// Index expression: `base[index]`.
///
/// Produces a fallible place — must be resolved by postfix `?` or `!`.
/// Bare `a[i]` is a type error.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprIndex<'db> {
    pub base: ExprFun<'db>,
    pub index: ExprFun<'db>,
}

/// Selector for field projection.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum FieldSelector<'db> {
    /// Named field: a.x
    Name(InternedText<'db>),
    /// Indexed field: a.0
    Index(u32),
}

/// Base struct for simple literals (true, false, none).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprLit<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprInt<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub value: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprFloat<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub value: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprHex<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub value: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprString<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub value: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprList<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub elements: Vec<ExprFun<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprSet<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub elements: Vec<ExprFun<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprMap<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub entries: Vec<ExprMapEntry<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprMapEntry<'db> {
    pub key: ExprFun<'db>,
    pub value: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprTensor<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub shape: Vec<u32>,
    pub elements: Vec<ExprFun<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprAnonTuple<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub elements: Vec<ExprFun<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprAnonStruct<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub fields: Vec<ExprStructField<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprStructField<'db> {
    pub name: InternedText<'db>,
    pub value: ExprFun<'db>,
}


#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprSome<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub payload: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprOk<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub payload: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprEr<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub payload: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprData<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub value: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprError<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub value: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprTable<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub header: Vec<InternedText<'db>>,
    pub rows: Vec<ExprTableRow<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprTableRow<'db> {
    pub elements: Vec<ExprFun<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprFunParseError<'db> {
    pub text: Text<'db>,
    pub span: ByteSpan,
    pub message: InternedText<'db>,
}

/// Atom expression: `atom Foo`.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprAtom<'db> {
    pub name: InternedText<'db>,
}

/// Term expression: `term Foo payload`.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprTerm<'db> {
    pub name: InternedText<'db>,
    pub payload: ExprFun<'db>,
}

/// Enum literal expression: `enum { atom Foo }` (requires type context).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprEnumLiteral<'db> {
    pub variant: ExprFun<'db>,
}

/// Intrinsic call expression (icall name(args)).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprIntrinsicCall<'db> {
    /// The intrinsic name (e.g., "bitnot_u32").
    pub name: InternedText<'db>,
    /// Arguments to the intrinsic.
    pub args: Vec<ExprFun<'db>>,
    /// Mode marker written before each argument, parallel to `args`.
    ///
    /// Intrinsics take every argument by value, so any marker here is an
    /// error; it is carried so typechecking can report one.
    pub arg_modes: Vec<Option<ParamMode>>,
}
