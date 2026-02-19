use rmx::prelude::*;

use bct::module_graph::ModuleId;
use bct::text::{InternedText, Text};
use bct::text::ByteSpan;
use bct::diagnostic::SpanEntry;
use datalove_datalit as datalit;

/// Span entry for a parsed expression, using salsa IDs for storage.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ParseSpanEntry {
    pub expr_id: salsa::Id,
    pub text_id: salsa::Id,
    pub span: ByteSpan,
}

impl ParseSpanEntry {
    pub fn new(expr_id: salsa::Id, text_id: salsa::Id, span: ByteSpan) -> Self {
        ParseSpanEntry { expr_id, text_id, span }
    }
}

/// Result of parsing a source text into statements.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ParsedStatements<'db> {
    pub statements: Vec<Statement<'db>>,
}

/// Parse result containing parsed statements and span side tables.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ParseResult<'db> {
    pub parsed: ParsedStatements<'db>,
    pub expr_spans: Vec<ParseSpanEntry>,
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
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
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
    Match(StmtMatch<'db>),
    ParseError(StmtParseError<'db>),
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
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
#[derive(salsa::Update)]
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
#[derive(salsa::Update)]
pub struct StmtConst<'db> {
    pub name: InternedText<'db>,
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub value: ExprFun<'db>,
}

/// Mutation of an existing mutable variable or field.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct StmtSet<'db> {
    pub target: Place<'db>,
    pub value: ExprFun<'db>,
    /// Index for span lookup in DatafunSpans.
    pub local_index: u32,
}

/// A place: a root variable plus navigation steps to a storage location.
///
/// Used for set-statement targets and for expression-context places
/// that involve index operations (e.g., `a[i]?`, `a[i]?.field`).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct Place<'db> {
    pub root: InternedText<'db>,
    pub steps: Vec<PlaceStep<'db>>,
}

/// A navigation step within a place.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
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
#[derive(salsa::Update)]
pub struct PlaceIndex<'db> {
    pub index: ExprFun<'db>,
    pub error_mode: Option<IndexErrorMode>,
}

/// How index failure is handled.
#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
#[derive(salsa::Update)]
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
    pub module_id: Option<ModuleId>,
    /// Function name (identity key).
    pub name: InternedText<'db>,
    #[tracked]
    #[returns(ref)]
    pub params: Vec<FunParam<'db>>,
    #[tracked]
    pub return_type: Option<datalit::ast::TypeHint<'db>>,
    #[tracked]
    #[returns(ref)]
    pub body: Vec<Statement<'db>>,
    /// Index for span lookup in DatafunSpans.fun_spans.
    pub local_index: u32,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct FunParam<'db> {
    pub name: InternedText<'db>,
    pub mode: ParamMode,
    pub is_comptime: bool,
    pub type_hint: datalit::ast::TypeHint<'db>,
}

#[derive(Copy, Clone, Hash, Debug, PartialEq, Eq)]
#[derive(salsa::Update)]
pub enum ParamMode {
    In,   // by-val (default)
    Out,  // by-mut-ptr
    Ref,  // by-ref
    Mut,  // by-mut-ref
}

/// Return statement. Value is None for bare `ret` in void functions.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct StmtRet<'db> {
    pub value: Option<ExprFun<'db>>,
    /// Index for span lookup in DatafunSpans.
    pub local_index: u32,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub enum StmtRequire<'db> {
    Module(StmtRequireModule<'db>),
    Data(StmtRequireData<'db>),
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct StmtRequireModule<'db> {
    pub import_space: InternedText<'db>,
    pub package_alias: InternedText<'db>,
    pub module_alias: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct StmtRequireData<'db> {
    pub name: InternedText<'db>,
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct StmtImport<'db> {
    pub module_name: InternedText<'db>,
    pub item_name: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
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
#[derive(salsa::Update)]
pub struct StmtLoop<'db> {
    /// Optional while condition, checked at the start of each iteration.
    pub condition: Option<ExprFun<'db>>,
    pub body: Vec<Statement<'db>>,
}

/// Break statement for exiting the innermost loop.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct StmtBreak {
    /// Index for span lookup in DatafunSpans.
    pub local_index: u32,
}

/// Continue statement for skipping to the next iteration.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct StmtContinue {
    /// Index for span lookup in DatafunSpans.
    pub local_index: u32,
}

/// Debug log statement for outputting values during execution.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct StmtDebugLog<'db> {
    pub value: ExprFun<'db>,
}

/// Type alias statement: `type Name: structural_type`.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct StmtTypeAlias<'db> {
    pub name: InternedText<'db>,
    pub type_hint: datalit::ast::TypeHint<'db>,
    pub local_index: u32,
}

/// Match statement for exhaustive enum destructuring.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct StmtMatch<'db> {
    pub input: ExprFun<'db>,
    pub cases: Vec<MatchCase<'db>>,
    pub default_body: Option<Vec<Statement<'db>>>,
}

/// A single case arm in a match statement.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct MatchCase<'db> {
    pub kind: MatchCaseKind<'db>,
    pub body: Vec<Statement<'db>>,
}

/// Kind of match case: atom (no binding) or term (with binding).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub enum MatchCaseKind<'db> {
    Atom { name: InternedText<'db> },
    Term { name: InternedText<'db>, binding: InternedText<'db> },
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct StmtParseError<'db> {
    pub text: Text<'db>,
    pub span: ByteSpan,
    pub message: InternedText<'db>,
}

// Datafun expressions - wraps datalit expressions and adds datafun-specific variants
#[salsa::tracked]
pub struct ExprFun<'db> {
    /// Module this expression belongs to (identity key). None for scripts.
    pub module_id: Option<ModuleId>,
    /// Function this expression belongs to (identity key). None for script-level.
    pub fn_name: Option<InternedText<'db>>,
    /// Sequential index within the function (identity key).
    pub local_index: u32,
    #[tracked]
    pub expr: ExprFunKind<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub enum ExprFunKind<'db> {
    // Bare name/identifier (for variables)
    Name(InternedText<'db>),
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
#[derive(salsa::Update)]
pub struct ExprBinOp<'db> {
    pub op: BinOp,
    pub lhs: ExprFun<'db>,
    pub rhs: ExprFun<'db>,
}

#[salsa::tracked]
pub struct ExprFunctionCall<'db> {
    /// Module this call belongs to (identity key). None for scripts.
    pub module_id: Option<ModuleId>,
    /// Function this call belongs to (identity key). None for script-level.
    pub fn_name: Option<InternedText<'db>>,
    /// Sequential index within the function (identity key).
    pub local_index: u32,
    #[tracked]
    pub name: InternedText<'db>,
    #[tracked]
    #[returns(ref)]
    pub args: Vec<ExprFun<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprTuple<'db> {
    pub elements: Vec<ExprFun<'db>>,
}

#[derive(Copy, Clone, Hash, Debug, PartialEq, Eq)]
#[derive(salsa::Update)]
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
#[derive(salsa::Update)]
pub enum UnaryOp {
    Neg,          // - (bare, for bigints)
    NegOptional,  // -?
    NegResult,    // -!
    Not,          // not (boolean)
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprUnaryOp<'db> {
    pub op: UnaryOp,
    pub operand: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprTryOption<'db> {
    pub operand: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
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
#[derive(salsa::Update)]
pub struct ExprCloneCoerce<'db> {
    pub operand: ExprFun<'db>,
}

/// Field projection expression (a.x or a.0).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprFieldProj<'db> {
    pub base: ExprFun<'db>,
    pub field: FieldSelector<'db>,
}

/// Index expression: `base[index]`.
///
/// Produces a fallible place — must be resolved by postfix `?` or `!`.
/// Bare `a[i]` is a type error.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprIndex<'db> {
    pub base: ExprFun<'db>,
    pub index: ExprFun<'db>,
}

/// Selector for field projection.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub enum FieldSelector<'db> {
    /// Named field: a.x
    Name(InternedText<'db>),
    /// Indexed field: a.0
    Index(u32),
}

/// Base struct for simple literals (true, false, none).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprLit<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprInt<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub value: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprFloat<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub value: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprHex<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub value: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprString<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub value: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprList<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub elements: Vec<ExprFun<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprSet<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub elements: Vec<ExprFun<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprMap<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub entries: Vec<ExprMapEntry<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprMapEntry<'db> {
    pub key: ExprFun<'db>,
    pub value: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprTensor<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub shape: Vec<u32>,
    pub elements: Vec<ExprFun<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprAnonTuple<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub elements: Vec<ExprFun<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprAnonStruct<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub fields: Vec<ExprStructField<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprStructField<'db> {
    pub name: InternedText<'db>,
    pub value: ExprFun<'db>,
}


#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprSome<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub payload: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprOk<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub payload: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprEr<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub payload: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprData<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub value: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprError<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub value: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprTable<'db> {
    pub type_hint: Option<datalit::ast::TypeHint<'db>>,
    pub header: Vec<InternedText<'db>>,
    pub rows: Vec<ExprTableRow<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprTableRow<'db> {
    pub elements: Vec<ExprFun<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprFunParseError<'db> {
    pub text: Text<'db>,
    pub span: ByteSpan,
    pub message: InternedText<'db>,
}

/// Atom expression: `atom Foo`.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprAtom<'db> {
    pub name: InternedText<'db>,
}

/// Term expression: `term Foo payload`.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprTerm<'db> {
    pub name: InternedText<'db>,
    pub payload: ExprFun<'db>,
}

/// Enum literal expression: `enum { atom Foo }` (requires type context).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprEnumLiteral<'db> {
    pub variant: ExprFun<'db>,
}

/// Intrinsic call expression (icall name(args)).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprIntrinsicCall<'db> {
    /// The intrinsic name (e.g., "bitnot_u32").
    pub name: InternedText<'db>,
    /// Arguments to the intrinsic.
    pub args: Vec<ExprFun<'db>>,
}
