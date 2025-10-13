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
    pub value: datalit::ast::ExprFull<'db>,
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
    pub value: datalit::ast::ExprFull<'db>,
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
