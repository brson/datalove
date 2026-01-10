use rmx::prelude::*;

use bct::text::{InternedText, Text};
use datalove_diagnostic::ByteSpan;
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
#[salsa::tracked]
pub struct ParsedStatements<'db> {
    #[returns(ref)]
    pub statements: Vec<Statement<'db>>,
}

/// Parse result containing parsed statements and span side table.
#[salsa::tracked]
pub struct ParseResult<'db> {
    pub parsed: ParsedStatements<'db>,
    #[returns(ref)]
    pub expr_spans: Vec<ParseSpanEntry>,
}

#[derive(Clone, Hash)]
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
    Break(StmtBreak<'db>),
    Continue(StmtContinue<'db>),
    DebugLog(StmtDebugLog<'db>),
    ParseError(StmtParseError<'db>),
}

#[salsa::tracked]
pub struct StmtLet<'db> {
    pub name: InternedText<'db>,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub value: ExprFun<'db>,
}

/// Mutable variable declaration.
#[salsa::tracked]
pub struct StmtVar<'db> {
    pub name: InternedText<'db>,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub value: ExprFun<'db>,
}

/// Mutation of an existing mutable variable.
#[salsa::tracked]
pub struct StmtSet<'db> {
    pub name: InternedText<'db>,
    pub value: ExprFun<'db>,
}

#[salsa::tracked]
pub struct StmtFun<'db> {
    pub name: InternedText<'db>,
    #[returns(ref)]
    pub params: Vec<FunParam<'db>>,
    pub return_type: Option<datalit::ast::TypeHintAndHeap<'db>>,
    #[returns(ref)]
    pub body: Vec<Statement<'db>>,
}

#[salsa::tracked]
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
#[salsa::tracked]
pub struct StmtRet<'db> {
    pub value: Option<ExprFun<'db>>,
}

#[derive(Clone, Hash)]
#[derive(salsa::Update)]
pub enum StmtRequire<'db> {
    Module(StmtRequireModule<'db>),
    Data(StmtRequireData<'db>),
}

#[salsa::tracked]
pub struct StmtRequireModule<'db> {
    pub import_space: InternedText<'db>,
    pub package_alias: InternedText<'db>,
    pub module_alias: InternedText<'db>,
}

#[salsa::tracked]
pub struct StmtRequireData<'db> {
    pub name: InternedText<'db>,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
}

#[salsa::tracked]
pub struct StmtImport<'db> {
    pub module_name: InternedText<'db>,
    pub item_name: InternedText<'db>,
}

#[salsa::tracked]
pub struct StmtIf<'db> {
    pub condition: ExprFun<'db>,
    pub then_binding: Option<InternedText<'db>>,
    #[returns(ref)]
    pub then_body: Vec<Statement<'db>>,
    pub else_binding: Option<InternedText<'db>>,
    #[returns(ref)]
    pub else_body: Option<Vec<Statement<'db>>>,
}

/// Loop statement with optional while condition.
///
/// Basic: `loop ... end loop`
/// With while: `loop while cond ... end loop`
#[salsa::tracked]
pub struct StmtLoop<'db> {
    /// Optional while condition, checked at the start of each iteration.
    pub condition: Option<ExprFun<'db>>,
    #[returns(ref)]
    pub body: Vec<Statement<'db>>,
}

/// Break statement for exiting the innermost loop.
#[salsa::tracked]
pub struct StmtBreak<'db> {
    /// Placeholder field since salsa::tracked needs at least one field.
    #[allow(dead_code)]
    _marker: (),
}

/// Continue statement for skipping to the next iteration.
#[salsa::tracked]
pub struct StmtContinue<'db> {
    /// Placeholder field since salsa::tracked needs at least one field.
    #[allow(dead_code)]
    _marker: (),
}

/// Debug log statement for outputting values during execution.
#[salsa::tracked]
pub struct StmtDebugLog<'db> {
    pub value: ExprFun<'db>,
}

#[salsa::tracked]
pub struct StmtParseError<'db> {
    pub text: Text<'db>,
    pub span: ByteSpan,
    pub message: InternedText<'db>,
}

// Datafun expressions - wraps datalit expressions and adds datafun-specific variants
#[salsa::tracked]
pub struct ExprFun<'db> {
    pub expr: ExprFunKind<'db>,
}

#[derive(Clone, Hash)]
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

#[salsa::tracked]
pub struct ExprBinOp<'db> {
    pub op: BinOp,
    pub lhs: ExprFun<'db>,
    pub rhs: ExprFun<'db>,
}

#[salsa::tracked]
pub struct ExprFunctionCall<'db> {
    pub name: InternedText<'db>,
    #[returns(ref)]
    pub args: Vec<ExprFun<'db>>,
}

#[salsa::tracked]
pub struct ExprTuple<'db> {
    #[returns(ref)]
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

#[salsa::tracked]
pub struct ExprUnaryOp<'db> {
    pub op: UnaryOp,
    pub operand: ExprFun<'db>,
}

#[salsa::tracked]
pub struct ExprTryOption<'db> {
    pub operand: ExprFun<'db>,
}

#[salsa::tracked]
pub struct ExprTryResult<'db> {
    pub operand: ExprFun<'db>,
}

// Base struct for simple literals (true, false, none).
#[salsa::tracked]
pub struct ExprLit<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
}

#[salsa::tracked]
pub struct ExprInt<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub value: InternedText<'db>,
}

#[salsa::tracked]
pub struct ExprFloat<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub value: InternedText<'db>,
}

#[salsa::tracked]
pub struct ExprHex<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub value: InternedText<'db>,
}

#[salsa::tracked]
pub struct ExprString<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub value: InternedText<'db>,
}

#[salsa::tracked]
pub struct ExprList<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    #[returns(ref)]
    pub elements: Vec<ExprFun<'db>>,
}

#[salsa::tracked]
pub struct ExprSet<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    #[returns(ref)]
    pub elements: Vec<ExprFun<'db>>,
}

#[salsa::tracked]
pub struct ExprMap<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    #[returns(ref)]
    pub entries: Vec<ExprMapEntry<'db>>,
}

#[salsa::tracked]
pub struct ExprMapEntry<'db> {
    pub key: ExprFun<'db>,
    pub value: ExprFun<'db>,
}

#[salsa::tracked]
pub struct ExprTensor<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    #[returns(ref)]
    pub shape: Vec<u32>,
    #[returns(ref)]
    pub elements: Vec<ExprFun<'db>>,
}

#[salsa::tracked]
pub struct ExprAnonTuple<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    #[returns(ref)]
    pub elements: Vec<ExprFun<'db>>,
}

#[salsa::tracked]
pub struct ExprAnonStruct<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    #[returns(ref)]
    pub fields: Vec<ExprStructField<'db>>,
}

#[salsa::tracked]
pub struct ExprStructField<'db> {
    pub name: InternedText<'db>,
    pub value: ExprFun<'db>,
}

#[salsa::tracked]
pub struct ExprAnonEnum<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub variant_name: InternedText<'db>,
    pub payload: Option<ExprFun<'db>>,
}

#[salsa::tracked]
pub struct ExprSome<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub payload: ExprFun<'db>,
}

#[salsa::tracked]
pub struct ExprOk<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub payload: ExprFun<'db>,
}

#[salsa::tracked]
pub struct ExprEr<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub payload: ExprFun<'db>,
}

#[salsa::tracked]
pub struct ExprData<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub value: ExprFun<'db>,
}

#[salsa::tracked]
pub struct ExprError<'db> {
    pub heap: datalit::ast::Heap,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub value: ExprFun<'db>,
}

#[salsa::tracked]
pub struct ExprFunParseError<'db> {
    pub text: Text<'db>,
    pub span: ByteSpan,
    pub message: InternedText<'db>,
}
