use rmx::prelude::*;

extern crate rmx;
use rmx::serde as serde;

/// Serializable AST for test snapshots.
/// Mirrors the salsa-based AST in ast.rs but uses regular Rust types.

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Script {
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum Statement {
    Let(StmtLet),
    Var(StmtVar),
    Set(StmtSet),
    Fun(StmtFun),
    Ret(StmtRet),
    Require(StmtRequire),
    Import(StmtImport),
    If(StmtIf),
    Loop(StmtLoop),
    Break(StmtBreak),
    Continue(StmtContinue),
    ParseError(StmtParseError),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtLet {
    pub name: String,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub value: ExprFun,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtVar {
    pub name: String,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub value: ExprFun,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtSet {
    pub name: String,
    pub value: ExprFun,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtFun {
    pub name: String,
    pub params: Vec<FunParam>,
    pub return_type: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FunParam {
    pub name: String,
    pub mode: ParamMode,
    pub type_hint: crate::datalit::ast_serde::TypeHintAndHeap,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ParamMode {
    In,
    Out,
    Ref,
    Mut,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtRet {
    pub value: Option<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "variant")]
pub enum StmtRequire {
    Module(StmtRequireModule),
    Data(StmtRequireData),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtRequireModule {
    pub import_space: String,
    pub package_alias: String,
    pub module_alias: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtRequireData {
    pub name: String,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtImport {
    pub module_name: String,
    pub item_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtIf {
    pub condition: ExprFun,
    pub then_body: Vec<Statement>,
    pub else_body: Option<Vec<Statement>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtLoop {
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtBreak {}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtContinue {}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtParseError {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprFun {
    pub expr: ExprFunKind,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum ExprFunKind {
    Name { name: String },
    BinOp(ExprBinOp),
    FunctionCall(ExprFunctionCall),
    Tuple(ExprTuple),
    UnaryOp(ExprUnaryOp),
    TryOption(ExprTryOption),
    TryResult(ExprTryResult),

    // Inline literal expressions.
    True(ExprLit),
    False(ExprLit),
    None(ExprLit),
    Int(ExprInt),
    Float(ExprFloat),
    Hex(ExprHex),
    String(ExprString),

    // Collection expressions.
    List(ExprList),
    Set(ExprSet),
    Map(ExprMap),
    Tensor(ExprTensor),

    // Aggregate expressions.
    AnonTuple(ExprAnonTuple),
    AnonStruct(ExprAnonStruct),
    AnonEnum(ExprAnonEnum),

    // Wrapper expressions.
    Some(ExprSome),
    Ok(ExprOk),
    Er(ExprEr),
    Data(ExprData),
    Error(ExprError),

    ParseError(ExprFunParseError),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprBinOp {
    pub op: BinOp,
    pub lhs: Box<ExprFun>,
    pub rhs: Box<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprFunctionCall {
    pub name: String,
    pub args: Vec<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprTuple {
    pub elements: Vec<ExprFun>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    AddChecked,
    SubChecked,
    MulChecked,
    DivChecked,
    AddOptional,
    SubOptional,
    MulOptional,
    DivOptional,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum UnaryOp {
    Neg,
    NegOptional,
    NegResult,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprUnaryOp {
    pub op: UnaryOp,
    pub operand: Box<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprTryOption {
    pub operand: Box<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprTryResult {
    pub operand: Box<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprFunParseError {
    pub message: String,
}

// Serde types for inline literal expressions.

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprLit {
    pub heap: crate::datalit::ast_serde::Heap,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprInt {
    pub heap: crate::datalit::ast_serde::Heap,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprFloat {
    pub heap: crate::datalit::ast_serde::Heap,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprHex {
    pub heap: crate::datalit::ast_serde::Heap,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprString {
    pub heap: crate::datalit::ast_serde::Heap,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprList {
    pub heap: crate::datalit::ast_serde::Heap,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub elements: Vec<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprSet {
    pub heap: crate::datalit::ast_serde::Heap,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub elements: Vec<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprMap {
    pub heap: crate::datalit::ast_serde::Heap,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub entries: Vec<ExprMapEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprMapEntry {
    pub key: ExprFun,
    pub value: ExprFun,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprTensor {
    pub heap: crate::datalit::ast_serde::Heap,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub shape: Vec<u32>,
    pub elements: Vec<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprAnonTuple {
    pub heap: crate::datalit::ast_serde::Heap,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub elements: Vec<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprAnonStruct {
    pub heap: crate::datalit::ast_serde::Heap,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub fields: Vec<ExprStructField>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprStructField {
    pub name: String,
    pub value: ExprFun,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprAnonEnum {
    pub heap: crate::datalit::ast_serde::Heap,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub variant_name: String,
    pub payload: Option<Box<ExprFun>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprSome {
    pub heap: crate::datalit::ast_serde::Heap,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub payload: Box<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprOk {
    pub heap: crate::datalit::ast_serde::Heap,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub payload: Box<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprEr {
    pub heap: crate::datalit::ast_serde::Heap,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub payload: Box<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprData {
    pub heap: crate::datalit::ast_serde::Heap,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub value: Box<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprError {
    pub heap: crate::datalit::ast_serde::Heap,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
    pub value: Box<ExprFun>,
}

// Conversion from salsa AST to serializable AST.

impl Script {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::Script<'db>) -> Self {
        Script {
            statements: ast.statements(db).iter().map(|s| Statement::from_ast(db, s)).collect(),
        }
    }
}

impl Statement {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: &crate::ast::Statement<'db>) -> Self {
        match ast {
            crate::ast::Statement::Let(s) => Statement::Let(StmtLet::from_ast(db, *s)),
            crate::ast::Statement::Var(s) => Statement::Var(StmtVar::from_ast(db, *s)),
            crate::ast::Statement::Set(s) => Statement::Set(StmtSet::from_ast(db, *s)),
            crate::ast::Statement::Fun(s) => Statement::Fun(StmtFun::from_ast(db, *s)),
            crate::ast::Statement::Ret(s) => Statement::Ret(StmtRet::from_ast(db, *s)),
            crate::ast::Statement::Require(s) => Statement::Require(StmtRequire::from_ast(db, s)),
            crate::ast::Statement::Import(s) => Statement::Import(StmtImport::from_ast(db, *s)),
            crate::ast::Statement::If(s) => Statement::If(StmtIf::from_ast(db, *s)),
            crate::ast::Statement::Loop(s) => Statement::Loop(StmtLoop::from_ast(db, *s)),
            crate::ast::Statement::Break(_) => Statement::Break(StmtBreak {}),
            crate::ast::Statement::Continue(_) => Statement::Continue(StmtContinue {}),
            crate::ast::Statement::ParseError(s) => Statement::ParseError(StmtParseError::from_ast(db, *s)),
        }
    }
}

impl StmtLet {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::StmtLet<'db>) -> Self {
        StmtLet {
            name: ast.name(db).as_str(db).to_string(),
            type_hint: ast.type_hint(db).map(|th| crate::datalit::ast_serde::TypeHintAndHeap::from_ast(db, th)),
            value: ExprFun::from_ast(db, ast.value(db)),
        }
    }
}

impl StmtVar {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::StmtVar<'db>) -> Self {
        StmtVar {
            name: ast.name(db).as_str(db).to_string(),
            type_hint: ast.type_hint(db).map(|th| crate::datalit::ast_serde::TypeHintAndHeap::from_ast(db, th)),
            value: ExprFun::from_ast(db, ast.value(db)),
        }
    }
}

impl StmtSet {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::StmtSet<'db>) -> Self {
        StmtSet {
            name: ast.name(db).as_str(db).to_string(),
            value: ExprFun::from_ast(db, ast.value(db)),
        }
    }
}

impl StmtFun {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::StmtFun<'db>) -> Self {
        StmtFun {
            name: ast.name(db).as_str(db).to_string(),
            params: ast.params(db).iter().map(|p| FunParam::from_ast(db, *p)).collect(),
            return_type: ast.return_type(db).map(|rt| crate::datalit::ast_serde::TypeHintAndHeap::from_ast(db, rt)),
            body: ast.body(db).iter().map(|s| Statement::from_ast(db, s)).collect(),
        }
    }
}

impl FunParam {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::FunParam<'db>) -> Self {
        FunParam {
            name: ast.name(db).as_str(db).to_string(),
            mode: ParamMode::from_ast(ast.mode(db)),
            type_hint: crate::datalit::ast_serde::TypeHintAndHeap::from_ast(db, ast.type_hint(db)),
        }
    }
}

impl ParamMode {
    pub fn from_ast(ast: crate::ast::ParamMode) -> Self {
        match ast {
            crate::ast::ParamMode::In => ParamMode::In,
            crate::ast::ParamMode::Out => ParamMode::Out,
            crate::ast::ParamMode::Ref => ParamMode::Ref,
            crate::ast::ParamMode::Mut => ParamMode::Mut,
        }
    }
}

impl StmtRet {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::StmtRet<'db>) -> Self {
        StmtRet {
            value: ast.value(db).map(|v| ExprFun::from_ast(db, v)),
        }
    }
}

impl StmtRequire {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: &crate::ast::StmtRequire<'db>) -> Self {
        match ast {
            crate::ast::StmtRequire::Module(m) => {
                StmtRequire::Module(StmtRequireModule {
                    import_space: m.import_space(db).as_str(db).to_string(),
                    package_alias: m.package_alias(db).as_str(db).to_string(),
                    module_alias: m.module_alias(db).as_str(db).to_string(),
                })
            }
            crate::ast::StmtRequire::Data(d) => {
                StmtRequire::Data(StmtRequireData {
                    name: d.name(db).as_str(db).to_string(),
                    type_hint: d.type_hint(db).map(|th| crate::datalit::ast_serde::TypeHintAndHeap::from_ast(db, th)),
                })
            }
        }
    }
}

impl StmtImport {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::StmtImport<'db>) -> Self {
        StmtImport {
            module_name: ast.module_name(db).as_str(db).to_string(),
            item_name: ast.item_name(db).as_str(db).to_string(),
        }
    }
}

impl StmtIf {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::StmtIf<'db>) -> Self {
        StmtIf {
            condition: ExprFun::from_ast(db, ast.condition(db)),
            then_body: ast.then_body(db).iter().map(|s| Statement::from_ast(db, s)).collect(),
            else_body: ast.else_body(db).as_ref().map(|stmts| {
                stmts.iter().map(|s| Statement::from_ast(db, s)).collect()
            }),
        }
    }
}

impl StmtLoop {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::StmtLoop<'db>) -> Self {
        StmtLoop {
            body: ast.body(db).iter().map(|s| Statement::from_ast(db, s)).collect(),
        }
    }
}

impl StmtParseError {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::StmtParseError<'db>) -> Self {
        StmtParseError {
            message: ast.message(db).as_str(db).to_string(),
        }
    }
}

impl ExprFun {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprFun<'db>) -> Self {
        ExprFun {
            expr: ExprFunKind::from_ast(db, ast.expr(db)),
        }
    }
}

impl ExprFunKind {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprFunKind<'db>) -> Self {
        match ast {
            crate::ast::ExprFunKind::Name(n) => ExprFunKind::Name {
                name: n.as_str(db).to_string(),
            },
            crate::ast::ExprFunKind::BinOp(b) => ExprFunKind::BinOp(ExprBinOp::from_ast(db, b)),
            crate::ast::ExprFunKind::FunctionCall(f) => ExprFunKind::FunctionCall(ExprFunctionCall::from_ast(db, f)),
            crate::ast::ExprFunKind::Tuple(t) => ExprFunKind::Tuple(ExprTuple::from_ast(db, t)),
            crate::ast::ExprFunKind::UnaryOp(u) => ExprFunKind::UnaryOp(ExprUnaryOp::from_ast(db, u)),
            crate::ast::ExprFunKind::TryOption(t) => ExprFunKind::TryOption(ExprTryOption::from_ast(db, t)),
            crate::ast::ExprFunKind::TryResult(t) => ExprFunKind::TryResult(ExprTryResult::from_ast(db, t)),

            // Inline literal expressions.
            crate::ast::ExprFunKind::True(e) => ExprFunKind::True(ExprLit::from_ast(db, e)),
            crate::ast::ExprFunKind::False(e) => ExprFunKind::False(ExprLit::from_ast(db, e)),
            crate::ast::ExprFunKind::None(e) => ExprFunKind::None(ExprLit::from_ast(db, e)),
            crate::ast::ExprFunKind::Int(e) => ExprFunKind::Int(ExprInt::from_ast(db, e)),
            crate::ast::ExprFunKind::Float(e) => ExprFunKind::Float(ExprFloat::from_ast(db, e)),
            crate::ast::ExprFunKind::Hex(e) => ExprFunKind::Hex(ExprHex::from_ast(db, e)),
            crate::ast::ExprFunKind::String(e) => ExprFunKind::String(ExprString::from_ast(db, e)),

            // Collection expressions.
            crate::ast::ExprFunKind::List(e) => ExprFunKind::List(ExprList::from_ast(db, e)),
            crate::ast::ExprFunKind::Set(e) => ExprFunKind::Set(ExprSet::from_ast(db, e)),
            crate::ast::ExprFunKind::Map(e) => ExprFunKind::Map(ExprMap::from_ast(db, e)),
            crate::ast::ExprFunKind::Tensor(e) => ExprFunKind::Tensor(ExprTensor::from_ast(db, e)),

            // Aggregate expressions.
            crate::ast::ExprFunKind::AnonTuple(e) => ExprFunKind::AnonTuple(ExprAnonTuple::from_ast(db, e)),
            crate::ast::ExprFunKind::AnonStruct(e) => ExprFunKind::AnonStruct(ExprAnonStruct::from_ast(db, e)),
            crate::ast::ExprFunKind::AnonEnum(e) => ExprFunKind::AnonEnum(ExprAnonEnum::from_ast(db, e)),

            // Wrapper expressions.
            crate::ast::ExprFunKind::Some(e) => ExprFunKind::Some(ExprSome::from_ast(db, e)),
            crate::ast::ExprFunKind::Ok(e) => ExprFunKind::Ok(ExprOk::from_ast(db, e)),
            crate::ast::ExprFunKind::Er(e) => ExprFunKind::Er(ExprEr::from_ast(db, e)),
            crate::ast::ExprFunKind::Data(e) => ExprFunKind::Data(ExprData::from_ast(db, e)),
            crate::ast::ExprFunKind::Error(e) => ExprFunKind::Error(ExprError::from_ast(db, e)),

            crate::ast::ExprFunKind::ParseError(e) => ExprFunKind::ParseError(ExprFunParseError::from_ast(db, e)),
        }
    }
}

impl ExprBinOp {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprBinOp<'db>) -> Self {
        ExprBinOp {
            op: BinOp::from_ast(ast.op(db)),
            lhs: Box::new(ExprFun::from_ast(db, ast.lhs(db))),
            rhs: Box::new(ExprFun::from_ast(db, ast.rhs(db))),
        }
    }
}

impl ExprFunctionCall {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprFunctionCall<'db>) -> Self {
        ExprFunctionCall {
            name: ast.name(db).as_str(db).to_string(),
            args: ast.args(db).iter().map(|a| ExprFun::from_ast(db, *a)).collect(),
        }
    }
}

impl ExprTuple {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprTuple<'db>) -> Self {
        ExprTuple {
            elements: ast.elements(db).iter().map(|e| ExprFun::from_ast(db, *e)).collect(),
        }
    }
}

impl BinOp {
    pub fn from_ast(ast: crate::ast::BinOp) -> Self {
        match ast {
            crate::ast::BinOp::Add => BinOp::Add,
            crate::ast::BinOp::Sub => BinOp::Sub,
            crate::ast::BinOp::Mul => BinOp::Mul,
            crate::ast::BinOp::Div => BinOp::Div,
            crate::ast::BinOp::AddChecked => BinOp::AddChecked,
            crate::ast::BinOp::SubChecked => BinOp::SubChecked,
            crate::ast::BinOp::MulChecked => BinOp::MulChecked,
            crate::ast::BinOp::DivChecked => BinOp::DivChecked,
            crate::ast::BinOp::AddOptional => BinOp::AddOptional,
            crate::ast::BinOp::SubOptional => BinOp::SubOptional,
            crate::ast::BinOp::MulOptional => BinOp::MulOptional,
            crate::ast::BinOp::DivOptional => BinOp::DivOptional,
            crate::ast::BinOp::Lt => BinOp::Lt,
            crate::ast::BinOp::Gt => BinOp::Gt,
            crate::ast::BinOp::Le => BinOp::Le,
            crate::ast::BinOp::Ge => BinOp::Ge,
            crate::ast::BinOp::Eq => BinOp::Eq,
            crate::ast::BinOp::Ne => BinOp::Ne,
        }
    }
}

impl UnaryOp {
    pub fn from_ast(ast: crate::ast::UnaryOp) -> Self {
        match ast {
            crate::ast::UnaryOp::Neg => UnaryOp::Neg,
            crate::ast::UnaryOp::NegOptional => UnaryOp::NegOptional,
            crate::ast::UnaryOp::NegResult => UnaryOp::NegResult,
        }
    }
}

impl ExprUnaryOp {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprUnaryOp<'db>) -> Self {
        ExprUnaryOp {
            op: UnaryOp::from_ast(ast.op(db)),
            operand: Box::new(ExprFun::from_ast(db, ast.operand(db))),
        }
    }
}

impl ExprTryOption {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprTryOption<'db>) -> Self {
        ExprTryOption {
            operand: Box::new(ExprFun::from_ast(db, ast.operand(db))),
        }
    }
}

impl ExprTryResult {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprTryResult<'db>) -> Self {
        ExprTryResult {
            operand: Box::new(ExprFun::from_ast(db, ast.operand(db))),
        }
    }
}

impl ExprFunParseError {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprFunParseError<'db>) -> Self {
        ExprFunParseError {
            message: ast.message(db).as_str(db).to_string(),
        }
    }
}

// Conversion helpers for inline expression types.

fn heap_from_ast(ast: crate::datalit::ast::Heap) -> crate::datalit::ast_serde::Heap {
    crate::datalit::ast_serde::Heap::from_ast(ast)
}

fn type_hint_from_ast<'db>(
    db: &'db dyn crate::Db,
    ast: Option<crate::datalit::ast::TypeHintAndHeap<'db>>,
) -> Option<crate::datalit::ast_serde::TypeHintAndHeap> {
    ast.map(|th| crate::datalit::ast_serde::TypeHintAndHeap::from_ast(db, th))
}

impl ExprLit {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprLit<'db>) -> Self {
        ExprLit {
            heap: heap_from_ast(ast.heap(db)),
            type_hint: type_hint_from_ast(db, ast.type_hint(db)),
        }
    }
}

impl ExprInt {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprInt<'db>) -> Self {
        ExprInt {
            heap: heap_from_ast(ast.heap(db)),
            type_hint: type_hint_from_ast(db, ast.type_hint(db)),
            value: ast.value(db).as_str(db).to_string(),
        }
    }
}

impl ExprFloat {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprFloat<'db>) -> Self {
        ExprFloat {
            heap: heap_from_ast(ast.heap(db)),
            type_hint: type_hint_from_ast(db, ast.type_hint(db)),
            value: ast.value(db).as_str(db).to_string(),
        }
    }
}

impl ExprHex {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprHex<'db>) -> Self {
        ExprHex {
            heap: heap_from_ast(ast.heap(db)),
            type_hint: type_hint_from_ast(db, ast.type_hint(db)),
            value: ast.value(db).as_str(db).to_string(),
        }
    }
}

impl ExprString {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprString<'db>) -> Self {
        ExprString {
            heap: heap_from_ast(ast.heap(db)),
            type_hint: type_hint_from_ast(db, ast.type_hint(db)),
            value: ast.value(db).as_str(db).to_string(),
        }
    }
}

impl ExprList {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprList<'db>) -> Self {
        ExprList {
            heap: heap_from_ast(ast.heap(db)),
            type_hint: type_hint_from_ast(db, ast.type_hint(db)),
            elements: ast.elements(db).iter().map(|e| ExprFun::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprSet {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprSet<'db>) -> Self {
        ExprSet {
            heap: heap_from_ast(ast.heap(db)),
            type_hint: type_hint_from_ast(db, ast.type_hint(db)),
            elements: ast.elements(db).iter().map(|e| ExprFun::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprMap {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprMap<'db>) -> Self {
        ExprMap {
            heap: heap_from_ast(ast.heap(db)),
            type_hint: type_hint_from_ast(db, ast.type_hint(db)),
            entries: ast.entries(db).iter().map(|e| ExprMapEntry::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprMapEntry {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprMapEntry<'db>) -> Self {
        ExprMapEntry {
            key: ExprFun::from_ast(db, ast.key(db)),
            value: ExprFun::from_ast(db, ast.value(db)),
        }
    }
}

impl ExprTensor {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprTensor<'db>) -> Self {
        ExprTensor {
            heap: heap_from_ast(ast.heap(db)),
            type_hint: type_hint_from_ast(db, ast.type_hint(db)),
            shape: ast.shape(db).clone(),
            elements: ast.elements(db).iter().map(|e| ExprFun::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprAnonTuple {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprAnonTuple<'db>) -> Self {
        ExprAnonTuple {
            heap: heap_from_ast(ast.heap(db)),
            type_hint: type_hint_from_ast(db, ast.type_hint(db)),
            elements: ast.elements(db).iter().map(|e| ExprFun::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprAnonStruct {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprAnonStruct<'db>) -> Self {
        ExprAnonStruct {
            heap: heap_from_ast(ast.heap(db)),
            type_hint: type_hint_from_ast(db, ast.type_hint(db)),
            fields: ast.fields(db).iter().map(|f| ExprStructField::from_ast(db, *f)).collect(),
        }
    }
}

impl ExprStructField {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprStructField<'db>) -> Self {
        ExprStructField {
            name: ast.name(db).as_str(db).to_string(),
            value: ExprFun::from_ast(db, ast.value(db)),
        }
    }
}

impl ExprAnonEnum {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprAnonEnum<'db>) -> Self {
        ExprAnonEnum {
            heap: heap_from_ast(ast.heap(db)),
            type_hint: type_hint_from_ast(db, ast.type_hint(db)),
            variant_name: ast.variant_name(db).as_str(db).to_string(),
            payload: ast.payload(db).map(|p| Box::new(ExprFun::from_ast(db, p))),
        }
    }
}

impl ExprSome {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprSome<'db>) -> Self {
        ExprSome {
            heap: heap_from_ast(ast.heap(db)),
            type_hint: type_hint_from_ast(db, ast.type_hint(db)),
            payload: Box::new(ExprFun::from_ast(db, ast.payload(db))),
        }
    }
}

impl ExprOk {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprOk<'db>) -> Self {
        ExprOk {
            heap: heap_from_ast(ast.heap(db)),
            type_hint: type_hint_from_ast(db, ast.type_hint(db)),
            payload: Box::new(ExprFun::from_ast(db, ast.payload(db))),
        }
    }
}

impl ExprEr {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprEr<'db>) -> Self {
        ExprEr {
            heap: heap_from_ast(ast.heap(db)),
            type_hint: type_hint_from_ast(db, ast.type_hint(db)),
            payload: Box::new(ExprFun::from_ast(db, ast.payload(db))),
        }
    }
}

impl ExprData {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprData<'db>) -> Self {
        ExprData {
            heap: heap_from_ast(ast.heap(db)),
            type_hint: type_hint_from_ast(db, ast.type_hint(db)),
            value: Box::new(ExprFun::from_ast(db, ast.value(db))),
        }
    }
}

impl ExprError {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprError<'db>) -> Self {
        ExprError {
            heap: heap_from_ast(ast.heap(db)),
            type_hint: type_hint_from_ast(db, ast.type_hint(db)),
            value: Box::new(ExprFun::from_ast(db, ast.value(db))),
        }
    }
}
