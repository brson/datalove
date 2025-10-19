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
    Fun(StmtFun),
    Ret(StmtRet),
    Require(StmtRequire),
    Import(StmtImport),
    If(StmtIf),
    ParseError(StmtParseError),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtLet {
    pub name: String,
    pub type_hint: Option<crate::datalit::ast_serde::TypeHintAndHeap>,
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
    pub value: ExprFun,
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
    Datalit { value: Box<crate::datalit::ast_serde::ExprFull> },
    Name { name: String },
    BinOp(ExprBinOp),
    FunctionCall(ExprFunctionCall),
    Tuple(ExprTuple),
    TryOption(ExprTryOption),
    TryResult(ExprTryResult),
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
            crate::ast::Statement::Fun(s) => Statement::Fun(StmtFun::from_ast(db, *s)),
            crate::ast::Statement::Ret(s) => Statement::Ret(StmtRet::from_ast(db, *s)),
            crate::ast::Statement::Require(s) => Statement::Require(StmtRequire::from_ast(db, s)),
            crate::ast::Statement::Import(s) => Statement::Import(StmtImport::from_ast(db, *s)),
            crate::ast::Statement::If(s) => Statement::If(StmtIf::from_ast(db, *s)),
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
            value: ExprFun::from_ast(db, ast.value(db)),
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
            crate::ast::ExprFunKind::Datalit(e) => ExprFunKind::Datalit {
                value: Box::new(crate::datalit::ast_serde::ExprFull::from_ast(db, e)),
            },
            crate::ast::ExprFunKind::Name(n) => ExprFunKind::Name {
                name: n.as_str(db).to_string(),
            },
            crate::ast::ExprFunKind::BinOp(b) => ExprFunKind::BinOp(ExprBinOp::from_ast(db, b)),
            crate::ast::ExprFunKind::FunctionCall(f) => ExprFunKind::FunctionCall(ExprFunctionCall::from_ast(db, f)),
            crate::ast::ExprFunKind::Tuple(t) => ExprFunKind::Tuple(ExprTuple::from_ast(db, t)),
            crate::ast::ExprFunKind::TryOption(t) => ExprFunKind::TryOption(ExprTryOption::from_ast(db, t)),
            crate::ast::ExprFunKind::TryResult(t) => ExprFunKind::TryResult(ExprTryResult::from_ast(db, t)),
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
