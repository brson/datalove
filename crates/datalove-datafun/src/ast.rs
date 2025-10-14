use rmx::prelude::*;

use bct::text::InternedText;
use crate::datalit;

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

#[salsa::tracked]
pub struct StmtRequire<'db> {
    pub kind: RequireKind,
    pub name: InternedText<'db>,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
}

#[derive(Copy, Clone, Hash, Debug, PartialEq, Eq)]
#[derive(salsa::Update)]
pub enum RequireKind {
    Module,
    Data,
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
    // Bare name/identifier (for variables, function calls, etc.)
    Name(InternedText<'db>),
    // Binary operation
    BinOp(ExprBinOp<'db>),
    // Parse error
    ParseError(ExprFunParseError<'db>),
}

#[salsa::tracked]
pub struct ExprBinOp<'db> {
    pub op: BinOp,
    pub lhs: ExprFun<'db>,
    pub rhs: ExprFun<'db>,
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

    // Saturating arithmetic (| suffix)
    AddSaturating,  // +|
    SubSaturating,  // -|
    MulSaturating,  // *|
    DivSaturating,  // /|

    // Comparison operators
    Lt,  // .<
    Gt,  // .>
    Le,  // <=
    Ge,  // >=
    Eq,  // ==
    Ne,  // !=
}

#[salsa::tracked]
pub struct ExprFunParseError<'db> {
    pub message: InternedText<'db>,
}
