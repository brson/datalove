use rmx::prelude::*;

use bct::text::InternedText;
use crate::datalit;

/// Result of parsing a source text into statements.
#[salsa::tracked]
pub struct Script<'db> {
    #[returns(ref)]
    pub statements: Vec<Statement<'db>>,
}

#[derive(Clone, Hash)]
#[derive(salsa::Update)]
pub enum Statement<'db> {
    Let(StmtLet<'db>),
    Fun(StmtFun<'db>),
    Ret(StmtRet<'db>),
    Require(StmtRequire<'db>),
    Import(StmtImport<'db>),
    If(StmtIf<'db>),
    ParseError(StmtParseError<'db>),
}

#[salsa::tracked]
pub struct StmtLet<'db> {
    pub name: InternedText<'db>,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
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

#[salsa::tracked]
pub struct StmtRet<'db> {
    pub value: ExprFun<'db>,
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

#[salsa::tracked]
pub struct StmtParseError<'db> {
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
    // Wrap datalit expression (literals, tuples, structs, etc.)
    Datalit(datalit::ast::ExprFull<'db>),
    // Bare name/identifier (for variables)
    Name(InternedText<'db>),
    // Binary operation
    BinOp(ExprBinOp<'db>),
    // Function call
    FunctionCall(ExprFunctionCall<'db>),
    // Try operators (postfix ? and !)
    TryOption(ExprTryOption<'db>),
    TryResult(ExprTryResult<'db>),
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
}

#[salsa::tracked]
pub struct ExprTryOption<'db> {
    pub operand: ExprFun<'db>,
}

#[salsa::tracked]
pub struct ExprTryResult<'db> {
    pub operand: ExprFun<'db>,
}

#[salsa::tracked]
pub struct ExprFunParseError<'db> {
    pub message: InternedText<'db>,
}
