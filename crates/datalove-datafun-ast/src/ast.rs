use rmx::prelude::*;

use bct::module_graph::ModuleId;
use bct::text::{InternedText, Text};
use datalove_diagnostic::{ByteSpan, SpanEntry};
use crate::datalit;

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
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub enum Statement<'db> {
    Let(StmtLet<'db>),
    Var(StmtVar<'db>),
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
    ParseError(StmtParseError<'db>),
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct StmtLet<'db> {
    pub name: InternedText<'db>,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub value: ExprFun<'db>,
}

/// Mutable variable declaration.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct StmtVar<'db> {
    pub name: InternedText<'db>,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub value: ExprFun<'db>,
}

/// Mutation of an existing mutable variable or field.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct StmtSet<'db> {
    pub target: SetTarget<'db>,
    pub value: ExprFun<'db>,
    /// Index for span lookup in DatafunSpans.
    pub local_index: u32,
}

/// Target of a set statement.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub enum SetTarget<'db> {
    /// Simple variable: `set x = ...`
    Name(InternedText<'db>),
    /// Field projection: `set a.x = ...` or `set a.0 = ...`
    Proj(SetTargetProj<'db>),
}

/// Projection chain for set target.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct SetTargetProj<'db> {
    pub base: Box<SetTarget<'db>>,
    pub field: FieldSelector<'db>,
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
    pub return_type: Option<datalit::ast::TypeHintAndHeap<'db>>,
    #[tracked]
    #[returns(ref)]
    pub body: Vec<Statement<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct FunParam<'db> {
    pub name: InternedText<'db>,
    pub mode: ParamMode,
    pub type_hint: datalit::ast::TypeHintAndHeap<'db>,
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
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
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
    AnonEnum(ExprAnonEnum<'db>),

    // Wrapper expressions.
    Some(ExprSome<'db>),
    Ok(ExprOk<'db>),
    Er(ExprEr<'db>),
    Data(ExprData<'db>),
    Error(ExprError<'db>),

    // Parse error
    ParseError(ExprFunParseError<'db>),
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

/// Field projection expression (a.x or a.0).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprFieldProj<'db> {
    pub base: ExprFun<'db>,
    pub field: FieldSelector<'db>,
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
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprInt<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub value: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprFloat<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub value: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprHex<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub value: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprString<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub value: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprList<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub elements: Vec<ExprFun<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprSet<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub elements: Vec<ExprFun<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprMap<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
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
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub shape: Vec<u32>,
    pub elements: Vec<ExprFun<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprAnonTuple<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub elements: Vec<ExprFun<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprAnonStruct<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
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
pub struct ExprAnonEnum<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub variant_name: InternedText<'db>,
    pub payload: Option<ExprFun<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprSome<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub payload: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprOk<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub payload: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprEr<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub payload: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprData<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub value: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprError<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub value: ExprFun<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ExprFunParseError<'db> {
    pub text: Text<'db>,
    pub span: ByteSpan,
    pub message: InternedText<'db>,
}
