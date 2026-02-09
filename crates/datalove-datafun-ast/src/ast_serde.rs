use rmx::prelude::*;

extern crate rmx;
use rmx::serde as serde;
use salsa::Database as Db;

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
    Const(StmtConst),
    Set(StmtSet),
    Fun(StmtFun),
    Ret(StmtRet),
    Require(StmtRequire),
    Import(StmtImport),
    If(StmtIf),
    Loop(StmtLoop),
    Break(StmtBreak),
    Continue(StmtContinue),
    DebugLog(StmtDebugLog),
    TypeAlias(StmtTypeAlias),
    Match(StmtMatch),
    ParseError(StmtParseError),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtLet {
    pub name: String,
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub value: ExprFun,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtVar {
    pub name: String,
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub value: Option<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtConst {
    pub name: String,
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub value: ExprFun,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtSet {
    pub target: SetTarget,
    pub value: ExprFun,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "variant")]
pub enum SetTarget {
    Name { name: String },
    Proj(SetTargetProj),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SetTargetProj {
    pub base: Box<SetTarget>,
    pub field: FieldSelector,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "variant")]
pub enum FieldSelector {
    Name { name: String },
    Index { index: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtFun {
    pub name: String,
    pub params: Vec<FunParam>,
    pub return_type: Option<datalove_datalit::ast_serde::TypeHint>,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FunParam {
    pub name: String,
    pub mode: ParamMode,
    #[serde(default)]
    pub is_comptime: bool,
    pub type_hint: datalove_datalit::ast_serde::TypeHint,
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
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
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
pub struct StmtDebugLog {
    pub value: ExprFun,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtTypeAlias {
    pub name: String,
    pub type_hint: datalove_datalit::ast_serde::TypeHint,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StmtMatch {
    pub input: ExprFun,
    pub cases: Vec<MatchCase>,
    pub default_body: Option<Vec<Statement>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MatchCase {
    pub kind: MatchCaseKind,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "variant")]
pub enum MatchCaseKind {
    Atom { name: String },
    Term { name: String, binding: String },
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
    Name { name: String },
    BinOp(ExprBinOp),
    FunctionCall(ExprFunctionCall),
    Tuple(ExprTuple),
    UnaryOp(ExprUnaryOp),
    TryOption(ExprTryOption),
    TryResult(ExprTryResult),
    CloneCoerce(ExprCloneCoerce),
    FieldProj(ExprFieldProj),

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

    // Wrapper expressions.
    Some(ExprSome),
    Ok(ExprOk),
    Er(ExprEr),
    Data(ExprData),
    Error(ExprError),

    // Table expression.
    Table(ExprTable),

    // Atom/Term/Enum expressions.
    Atom(ExprAtom),
    Term(ExprTerm),
    EnumLiteral(ExprEnumLiteral),

    ParseError(ExprFunParseError),

    // Intrinsic call expression.
    IntrinsicCall(ExprIntrinsicCall),
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
pub struct ExprIntrinsicCall {
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
    And,
    Or,
    Xor,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum UnaryOp {
    Neg,
    NegOptional,
    NegResult,
    Not,
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
pub struct ExprCloneCoerce {
    pub operand: Box<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprFieldProj {
    pub base: Box<ExprFun>,
    pub field: FieldSelector,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprAtom {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprTerm {
    pub name: String,
    pub payload: Box<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprEnumLiteral {
    pub variant: Box<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprFunParseError {
    pub message: String,
}

// Serde types for inline literal expressions.

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprLit {
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprInt {
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprFloat {
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprHex {
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprString {
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprList {
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub elements: Vec<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprSet {
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub elements: Vec<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprMap {
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub entries: Vec<ExprMapEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprMapEntry {
    pub key: ExprFun,
    pub value: ExprFun,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprTensor {
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub shape: Vec<u32>,
    pub elements: Vec<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprAnonTuple {
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub elements: Vec<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprAnonStruct {
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub fields: Vec<ExprStructField>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprStructField {
    pub name: String,
    pub value: ExprFun,
}


#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprTable {
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub header: Vec<String>,
    pub rows: Vec<ExprTableRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprTableRow {
    pub elements: Vec<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprSome {
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub payload: Box<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprOk {
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub payload: Box<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprEr {
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub payload: Box<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprData {
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub value: Box<ExprFun>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprError {
    pub type_hint: Option<datalove_datalit::ast_serde::TypeHint>,
    pub value: Box<ExprFun>,
}

// Conversion from salsa AST to serializable AST.

impl Script {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: crate::ast::ParsedStatements<'db>) -> Self {
        Script {
            statements: ast.statements.iter().map(|s| Statement::from_ast(db, s)).collect(),
        }
    }
}

impl Statement {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::Statement<'db>) -> Self {
        match ast {
            crate::ast::Statement::Let(s) => Statement::Let(StmtLet::from_ast(db, s)),
            crate::ast::Statement::Var(s) => Statement::Var(StmtVar::from_ast(db, s)),
            crate::ast::Statement::Const(s) => Statement::Const(StmtConst::from_ast(db, s)),
            crate::ast::Statement::Set(s) => Statement::Set(StmtSet::from_ast(db, s)),
            crate::ast::Statement::Fun(s) => Statement::Fun(StmtFun::from_ast(db, *s)),
            crate::ast::Statement::Ret(s) => Statement::Ret(StmtRet::from_ast(db, s)),
            crate::ast::Statement::Require(s) => Statement::Require(StmtRequire::from_ast(db, s)),
            crate::ast::Statement::Import(s) => Statement::Import(StmtImport::from_ast(db, s)),
            crate::ast::Statement::If(s) => Statement::If(StmtIf::from_ast(db, s)),
            crate::ast::Statement::Loop(s) => Statement::Loop(StmtLoop::from_ast(db, s)),
            crate::ast::Statement::Break(_) => Statement::Break(StmtBreak {}),
            crate::ast::Statement::Continue(_) => Statement::Continue(StmtContinue {}),
            crate::ast::Statement::DebugLog(s) => Statement::DebugLog(StmtDebugLog::from_ast(db, s)),
            crate::ast::Statement::TypeAlias(s) => Statement::TypeAlias(StmtTypeAlias::from_ast(db, s)),
            crate::ast::Statement::Match(s) => Statement::Match(StmtMatch::from_ast(db, s)),
            crate::ast::Statement::ParseError(s) => Statement::ParseError(StmtParseError::from_ast(db, s)),
        }
    }
}

impl StmtTypeAlias {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::StmtTypeAlias<'db>) -> Self {
        StmtTypeAlias {
            name: ast.name.as_str(db).to_string(),
            type_hint: datalove_datalit::ast_serde::TypeHint::from_ast(db, ast.type_hint.clone()),
        }
    }
}

impl StmtDebugLog {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::StmtDebugLog<'db>) -> Self {
        StmtDebugLog {
            value: ExprFun::from_ast(db, ast.value),
        }
    }
}

impl StmtLet {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::StmtLet<'db>) -> Self {
        StmtLet {
            name: ast.name.as_str(db).to_string(),
            type_hint: ast.type_hint.clone().map(|th| datalove_datalit::ast_serde::TypeHint::from_ast(db, th)),
            value: ExprFun::from_ast(db, ast.value),
        }
    }
}

impl StmtVar {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::StmtVar<'db>) -> Self {
        StmtVar {
            name: ast.name.as_str(db).to_string(),
            type_hint: ast.type_hint.clone().map(|th| datalove_datalit::ast_serde::TypeHint::from_ast(db, th)),
            value: ast.value.map(|v| ExprFun::from_ast(db, v)),
        }
    }
}

impl StmtConst {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::StmtConst<'db>) -> Self {
        StmtConst {
            name: ast.name.as_str(db).to_string(),
            type_hint: ast.type_hint.clone().map(|th| datalove_datalit::ast_serde::TypeHint::from_ast(db, th)),
            value: ExprFun::from_ast(db, ast.value),
        }
    }
}

impl StmtSet {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::StmtSet<'db>) -> Self {
        StmtSet {
            target: SetTarget::from_ast(db, &ast.target),
            value: ExprFun::from_ast(db, ast.value),
        }
    }
}

impl SetTarget {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::SetTarget<'db>) -> Self {
        match ast {
            crate::ast::SetTarget::Name(name) => SetTarget::Name {
                name: name.as_str(db).to_string(),
            },
            crate::ast::SetTarget::Proj(proj) => SetTarget::Proj(SetTargetProj::from_ast(db, proj)),
        }
    }
}

impl SetTargetProj {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::SetTargetProj<'db>) -> Self {
        SetTargetProj {
            base: Box::new(SetTarget::from_ast(db, &*ast.base)),
            field: FieldSelector::from_ast(db, &ast.field),
        }
    }
}

impl FieldSelector {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::FieldSelector<'db>) -> Self {
        match ast {
            crate::ast::FieldSelector::Name(name) => FieldSelector::Name {
                name: name.as_str(db).to_string(),
            },
            crate::ast::FieldSelector::Index(idx) => FieldSelector::Index { index: *idx },
        }
    }
}

impl StmtFun {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: crate::ast::StmtFun<'db>) -> Self {
        StmtFun {
            name: ast.name(db).as_str(db).to_string(),
            params: ast.params(db).iter().map(|p| FunParam::from_ast(db, p)).collect(),
            return_type: ast.return_type(db).map(|rt| datalove_datalit::ast_serde::TypeHint::from_ast(db, rt)),
            body: ast.body(db).iter().map(|s| Statement::from_ast(db, s)).collect(),
        }
    }
}

impl FunParam {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::FunParam<'db>) -> Self {
        FunParam {
            name: ast.name.as_str(db).to_string(),
            mode: ParamMode::from_ast(ast.mode),
            is_comptime: ast.is_comptime,
            type_hint: datalove_datalit::ast_serde::TypeHint::from_ast(db, ast.type_hint.clone()),
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
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::StmtRet<'db>) -> Self {
        StmtRet {
            value: ast.value.map(|v| ExprFun::from_ast(db, v)),
        }
    }
}

impl StmtRequire {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::StmtRequire<'db>) -> Self {
        match ast {
            crate::ast::StmtRequire::Module(m) => {
                StmtRequire::Module(StmtRequireModule {
                    import_space: m.import_space.as_str(db).to_string(),
                    package_alias: m.package_alias.as_str(db).to_string(),
                    module_alias: m.module_alias.as_str(db).to_string(),
                })
            }
            crate::ast::StmtRequire::Data(d) => {
                StmtRequire::Data(StmtRequireData {
                    name: d.name.as_str(db).to_string(),
                    type_hint: d.type_hint.clone().map(|th| datalove_datalit::ast_serde::TypeHint::from_ast(db, th)),
                })
            }
        }
    }
}

impl StmtImport {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::StmtImport<'db>) -> Self {
        StmtImport {
            module_name: ast.module_name.as_str(db).to_string(),
            item_name: ast.item_name.as_str(db).to_string(),
        }
    }
}

impl StmtIf {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::StmtIf<'db>) -> Self {
        StmtIf {
            condition: ExprFun::from_ast(db, ast.condition),
            then_body: ast.then_body.iter().map(|s| Statement::from_ast(db, s)).collect(),
            else_body: ast.else_body.as_ref().map(|stmts| {
                stmts.iter().map(|s| Statement::from_ast(db, s)).collect()
            }),
        }
    }
}

impl StmtLoop {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::StmtLoop<'db>) -> Self {
        StmtLoop {
            body: ast.body.iter().map(|s| Statement::from_ast(db, s)).collect(),
        }
    }
}

impl StmtMatch {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::StmtMatch<'db>) -> Self {
        StmtMatch {
            input: ExprFun::from_ast(db, ast.input),
            cases: ast.cases.iter().map(|c| MatchCase::from_ast(db, c)).collect(),
            default_body: ast.default_body.as_ref().map(|stmts| {
                stmts.iter().map(|s| Statement::from_ast(db, s)).collect()
            }),
        }
    }
}

impl MatchCase {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::MatchCase<'db>) -> Self {
        MatchCase {
            kind: MatchCaseKind::from_ast(db, &ast.kind),
            body: ast.body.iter().map(|s| Statement::from_ast(db, s)).collect(),
        }
    }
}

impl MatchCaseKind {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::MatchCaseKind<'db>) -> Self {
        match ast {
            crate::ast::MatchCaseKind::Atom { name } => MatchCaseKind::Atom {
                name: name.as_str(db).to_string(),
            },
            crate::ast::MatchCaseKind::Term { name, binding } => MatchCaseKind::Term {
                name: name.as_str(db).to_string(),
                binding: binding.as_str(db).to_string(),
            },
        }
    }
}

impl StmtParseError {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::StmtParseError<'db>) -> Self {
        StmtParseError {
            message: ast.message.as_str(db).to_string(),
        }
    }
}

impl ExprFun {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: crate::ast::ExprFun<'db>) -> Self {
        ExprFun {
            expr: ExprFunKind::from_ast(db, &ast.expr(db)),
        }
    }
}

impl ExprFunKind {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprFunKind<'db>) -> Self {
        match ast {
            crate::ast::ExprFunKind::Name(n) => ExprFunKind::Name {
                name: n.as_str(db).to_string(),
            },
            crate::ast::ExprFunKind::BinOp(b) => ExprFunKind::BinOp(ExprBinOp::from_ast(db, b)),
            crate::ast::ExprFunKind::FunctionCall(f) => ExprFunKind::FunctionCall(ExprFunctionCall::from_ast(db, *f)),
            crate::ast::ExprFunKind::Tuple(t) => ExprFunKind::Tuple(ExprTuple::from_ast(db, t)),
            crate::ast::ExprFunKind::UnaryOp(u) => ExprFunKind::UnaryOp(ExprUnaryOp::from_ast(db, u)),
            crate::ast::ExprFunKind::TryOption(t) => ExprFunKind::TryOption(ExprTryOption::from_ast(db, t)),
            crate::ast::ExprFunKind::TryResult(t) => ExprFunKind::TryResult(ExprTryResult::from_ast(db, t)),
            crate::ast::ExprFunKind::CloneCoerce(c) => ExprFunKind::CloneCoerce(ExprCloneCoerce::from_ast(db, c)),
            crate::ast::ExprFunKind::FieldProj(f) => ExprFunKind::FieldProj(ExprFieldProj::from_ast(db, f)),

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

            // Wrapper expressions.
            crate::ast::ExprFunKind::Some(e) => ExprFunKind::Some(ExprSome::from_ast(db, e)),
            crate::ast::ExprFunKind::Ok(e) => ExprFunKind::Ok(ExprOk::from_ast(db, e)),
            crate::ast::ExprFunKind::Er(e) => ExprFunKind::Er(ExprEr::from_ast(db, e)),
            crate::ast::ExprFunKind::Data(e) => ExprFunKind::Data(ExprData::from_ast(db, e)),
            crate::ast::ExprFunKind::Error(e) => ExprFunKind::Error(ExprError::from_ast(db, e)),

            // Table expression.
            crate::ast::ExprFunKind::Table(e) => ExprFunKind::Table(ExprTable::from_ast(db, e)),

            // Atom/Term/Enum expressions.
            crate::ast::ExprFunKind::Atom(e) => ExprFunKind::Atom(ExprAtom {
                name: e.name.as_str(db).to_string(),
            }),
            crate::ast::ExprFunKind::Term(e) => ExprFunKind::Term(ExprTerm {
                name: e.name.as_str(db).to_string(),
                payload: Box::new(ExprFun::from_ast(db, e.payload)),
            }),
            crate::ast::ExprFunKind::EnumLiteral(e) => ExprFunKind::EnumLiteral(ExprEnumLiteral {
                variant: Box::new(ExprFun::from_ast(db, e.variant)),
            }),

            crate::ast::ExprFunKind::ParseError(e) => ExprFunKind::ParseError(ExprFunParseError::from_ast(db, e)),
            crate::ast::ExprFunKind::IntrinsicCall(e) => ExprFunKind::IntrinsicCall(ExprIntrinsicCall::from_ast(db, e)),
        }
    }
}

impl ExprIntrinsicCall {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprIntrinsicCall<'db>) -> Self {
        ExprIntrinsicCall {
            name: ast.name.as_str(db).to_string(),
            args: ast.args.iter().map(|a| ExprFun::from_ast(db, *a)).collect(),
        }
    }
}

impl ExprBinOp {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprBinOp<'db>) -> Self {
        ExprBinOp {
            op: BinOp::from_ast(ast.op),
            lhs: Box::new(ExprFun::from_ast(db, ast.lhs)),
            rhs: Box::new(ExprFun::from_ast(db, ast.rhs)),
        }
    }
}

impl ExprFunctionCall {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: crate::ast::ExprFunctionCall<'db>) -> Self {
        ExprFunctionCall {
            name: ast.name(db).as_str(db).to_string(),
            args: ast.args(db).iter().map(|a| ExprFun::from_ast(db, *a)).collect(),
        }
    }
}

impl ExprTuple {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprTuple<'db>) -> Self {
        ExprTuple {
            elements: ast.elements.iter().map(|e| ExprFun::from_ast(db, *e)).collect(),
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
            crate::ast::BinOp::And => BinOp::And,
            crate::ast::BinOp::Or => BinOp::Or,
            crate::ast::BinOp::Xor => BinOp::Xor,
        }
    }
}

impl UnaryOp {
    pub fn from_ast(ast: crate::ast::UnaryOp) -> Self {
        match ast {
            crate::ast::UnaryOp::Neg => UnaryOp::Neg,
            crate::ast::UnaryOp::NegOptional => UnaryOp::NegOptional,
            crate::ast::UnaryOp::NegResult => UnaryOp::NegResult,
            crate::ast::UnaryOp::Not => UnaryOp::Not,
        }
    }
}

impl ExprUnaryOp {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprUnaryOp<'db>) -> Self {
        ExprUnaryOp {
            op: UnaryOp::from_ast(ast.op),
            operand: Box::new(ExprFun::from_ast(db, ast.operand)),
        }
    }
}

impl ExprTryOption {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprTryOption<'db>) -> Self {
        ExprTryOption {
            operand: Box::new(ExprFun::from_ast(db, ast.operand)),
        }
    }
}

impl ExprTryResult {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprTryResult<'db>) -> Self {
        ExprTryResult {
            operand: Box::new(ExprFun::from_ast(db, ast.operand)),
        }
    }
}

impl ExprCloneCoerce {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprCloneCoerce<'db>) -> Self {
        ExprCloneCoerce {
            operand: Box::new(ExprFun::from_ast(db, ast.operand)),
        }
    }
}

impl ExprFieldProj {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprFieldProj<'db>) -> Self {
        ExprFieldProj {
            base: Box::new(ExprFun::from_ast(db, ast.base)),
            field: FieldSelector::from_ast(db, &ast.field),
        }
    }
}

impl ExprFunParseError {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprFunParseError<'db>) -> Self {
        ExprFunParseError {
            message: ast.message.as_str(db).to_string(),
        }
    }
}

// Conversion helpers for inline expression types.

fn type_hint_from_ast<'db>(
    db: &'db dyn Db,
    ast: Option<datalove_datalit::ast::TypeHint<'db>>,
) -> Option<datalove_datalit::ast_serde::TypeHint> {
    ast.map(|th| datalove_datalit::ast_serde::TypeHint::from_ast(db, th))
}

impl ExprLit {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprLit<'db>) -> Self {
        ExprLit {
            type_hint: type_hint_from_ast(db, ast.type_hint.clone()),
        }
    }
}

impl ExprInt {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprInt<'db>) -> Self {
        ExprInt {
            type_hint: type_hint_from_ast(db, ast.type_hint.clone()),
            value: ast.value.as_str(db).to_string(),
        }
    }
}

impl ExprFloat {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprFloat<'db>) -> Self {
        ExprFloat {
            type_hint: type_hint_from_ast(db, ast.type_hint.clone()),
            value: ast.value.as_str(db).to_string(),
        }
    }
}

impl ExprHex {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprHex<'db>) -> Self {
        ExprHex {
            type_hint: type_hint_from_ast(db, ast.type_hint.clone()),
            value: ast.value.as_str(db).to_string(),
        }
    }
}

impl ExprString {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprString<'db>) -> Self {
        ExprString {
            type_hint: type_hint_from_ast(db, ast.type_hint.clone()),
            value: ast.value.as_str(db).to_string(),
        }
    }
}

impl ExprList {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprList<'db>) -> Self {
        ExprList {
            type_hint: type_hint_from_ast(db, ast.type_hint.clone()),
            elements: ast.elements.iter().map(|e| ExprFun::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprSet {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprSet<'db>) -> Self {
        ExprSet {
            type_hint: type_hint_from_ast(db, ast.type_hint.clone()),
            elements: ast.elements.iter().map(|e| ExprFun::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprMap {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprMap<'db>) -> Self {
        ExprMap {
            type_hint: type_hint_from_ast(db, ast.type_hint.clone()),
            entries: ast.entries.iter().map(|e| ExprMapEntry::from_ast(db, e)).collect(),
        }
    }
}

impl ExprMapEntry {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprMapEntry<'db>) -> Self {
        ExprMapEntry {
            key: ExprFun::from_ast(db, ast.key),
            value: ExprFun::from_ast(db, ast.value),
        }
    }
}

impl ExprTensor {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprTensor<'db>) -> Self {
        ExprTensor {
            type_hint: type_hint_from_ast(db, ast.type_hint.clone()),
            shape: ast.shape.clone(),
            elements: ast.elements.iter().map(|e| ExprFun::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprAnonTuple {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprAnonTuple<'db>) -> Self {
        ExprAnonTuple {
            type_hint: type_hint_from_ast(db, ast.type_hint.clone()),
            elements: ast.elements.iter().map(|e| ExprFun::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprAnonStruct {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprAnonStruct<'db>) -> Self {
        ExprAnonStruct {
            type_hint: type_hint_from_ast(db, ast.type_hint.clone()),
            fields: ast.fields.iter().map(|f| ExprStructField::from_ast(db, f)).collect(),
        }
    }
}

impl ExprStructField {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprStructField<'db>) -> Self {
        ExprStructField {
            name: ast.name.as_str(db).to_string(),
            value: ExprFun::from_ast(db, ast.value),
        }
    }
}


impl ExprTable {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprTable<'db>) -> Self {
        ExprTable {
            type_hint: type_hint_from_ast(db, ast.type_hint.clone()),
            header: ast.header.iter().map(|h| h.as_str(db).to_string()).collect(),
            rows: ast.rows.iter().map(|r| ExprTableRow::from_ast(db, r)).collect(),
        }
    }
}

impl ExprTableRow {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprTableRow<'db>) -> Self {
        ExprTableRow {
            elements: ast.elements.iter().map(|e| ExprFun::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprSome {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprSome<'db>) -> Self {
        ExprSome {
            type_hint: type_hint_from_ast(db, ast.type_hint.clone()),
            payload: Box::new(ExprFun::from_ast(db, ast.payload)),
        }
    }
}

impl ExprOk {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprOk<'db>) -> Self {
        ExprOk {
            type_hint: type_hint_from_ast(db, ast.type_hint.clone()),
            payload: Box::new(ExprFun::from_ast(db, ast.payload)),
        }
    }
}

impl ExprEr {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprEr<'db>) -> Self {
        ExprEr {
            type_hint: type_hint_from_ast(db, ast.type_hint.clone()),
            payload: Box::new(ExprFun::from_ast(db, ast.payload)),
        }
    }
}

impl ExprData {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprData<'db>) -> Self {
        ExprData {
            type_hint: type_hint_from_ast(db, ast.type_hint.clone()),
            value: Box::new(ExprFun::from_ast(db, ast.value)),
        }
    }
}

impl ExprError {
    pub fn from_ast<'db>(db: &'db dyn Db, ast: &crate::ast::ExprError<'db>) -> Self {
        ExprError {
            type_hint: type_hint_from_ast(db, ast.type_hint.clone()),
            value: Box::new(ExprFun::from_ast(db, ast.value)),
        }
    }
}
