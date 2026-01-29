use rmx::prelude::*;

extern crate rmx;
use rmx::serde as serde;

/// Serializable AST for test snapshots.
/// Mirrors the salsa-based AST in ast.rs but uses regular Rust types.

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprFull {
    pub type_hint: Option<TypeHint>,
    pub expr: Expr,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TypeHint {
    Bool,
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    Index,
    Offset,
    F32,
    F64,
    Int,
    String,
    AnonTuple(TypeHintAnonTuple),
    AnonStruct(TypeHintAnonStruct),
    AnonEnum(TypeHintAnonEnum),
    List(TypeHintList),
    Map(TypeHintMap),
    Set(TypeHintSet),
    Option(TypeHintOption),
    Result(TypeHintResult),
    Tensor(TypeHintTensor),
    Table(TypeHintTable),
    Data,
    Error,
    Alias(String),
    ParseError(TypeHintParseError),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypeHintAnonTuple {
    pub fields: Vec<TypeHint>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypeHintAnonStruct {
    pub fields: Vec<TypeHintNamedField>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypeHintNamedField {
    pub name: String,
    pub type_hint: TypeHint,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypeHintAnonEnum {
    pub variants: Vec<TypeHintEnumVariant>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypeHintEnumVariant {
    pub name: String,
    pub payload: Option<Box<TypeHint>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypeHintList {
    pub element_type: Box<TypeHint>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypeHintMap {
    pub key_type: Box<TypeHint>,
    pub value_type: Box<TypeHint>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypeHintSet {
    pub element_type: Box<TypeHint>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypeHintOption {
    pub inner_type: Box<TypeHint>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypeHintResult {
    pub inner_type: Box<TypeHint>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypeHintTensor {
    pub element_type: Box<TypeHint>,
    pub rank: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypeHintTable {
    pub columns: Vec<TypeHintNamedField>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Expr {
    True,
    False,
    Int(ExprInt),
    Float(ExprFloat),
    Hex(ExprHex),
    String(ExprString),
    AnonTuple(ExprAnonTuple),
    AnonStruct(ExprAnonStruct),
    AnonEnum(ExprAnonEnum),
    List(ExprList),
    Map(ExprMap),
    Set(ExprSet),
    Tensor(ExprTensor),
    Table(ExprTable),
    None,
    Some(ExprSome),
    Ok(ExprOk),
    Er(ExprEr),
    Data(ExprData),
    Error(ExprError),
    ParseError(ExprParseError),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprInt {
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprFloat {
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprHex {
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprString {
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprAnonTuple {
    pub elements: Vec<ExprFull>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprAnonStruct {
    pub fields: Vec<ExprStructField>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprStructField {
    pub name: String,
    pub value: ExprFull,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprAnonEnum {
    pub variant_name: String,
    pub payload: Option<Box<ExprFull>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprList {
    pub elements: Vec<ExprFull>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprMap {
    pub entries: Vec<ExprMapEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprMapEntry {
    pub key: ExprFull,
    pub value: ExprFull,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprSet {
    pub elements: Vec<ExprFull>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprTensor {
    pub shape: Vec<u32>,
    pub elements: Vec<ExprFull>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprTable {
    pub header: Vec<String>,
    pub rows: Vec<ExprTableRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprTableRow {
    pub elements: Vec<ExprFull>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprSome {
    pub payload: Box<ExprFull>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprOk {
    pub payload: Box<ExprFull>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprEr {
    pub payload: Box<ExprFull>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprData {
    pub value: Box<ExprFull>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprError {
    pub value: Box<ExprFull>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypeHintParseError {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExprParseError {
    pub message: String,
}

/// Conversion from salsa AST to serializable AST.
impl ExprFull {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprFull<'db>) -> Self {
        ExprFull {
            type_hint: ast.type_hint(db).map(|th| TypeHint::from_ast(db, th)),
            expr: Expr::from_ast(db, ast.expr(db).clone()),
        }
    }
}

impl TypeHint {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHint<'db>) -> Self {
        match ast {
            crate::ast::TypeHint::Bool => TypeHint::Bool,
            crate::ast::TypeHint::U8 => TypeHint::U8,
            crate::ast::TypeHint::I8 => TypeHint::I8,
            crate::ast::TypeHint::U16 => TypeHint::U16,
            crate::ast::TypeHint::I16 => TypeHint::I16,
            crate::ast::TypeHint::U32 => TypeHint::U32,
            crate::ast::TypeHint::I32 => TypeHint::I32,
            crate::ast::TypeHint::U64 => TypeHint::U64,
            crate::ast::TypeHint::I64 => TypeHint::I64,
            crate::ast::TypeHint::Index => TypeHint::Index,
            crate::ast::TypeHint::Offset => TypeHint::Offset,
            crate::ast::TypeHint::F32 => TypeHint::F32,
            crate::ast::TypeHint::F64 => TypeHint::F64,
            crate::ast::TypeHint::Int => TypeHint::Int,
            crate::ast::TypeHint::String => TypeHint::String,
            crate::ast::TypeHint::AnonTuple(t) => TypeHint::AnonTuple(TypeHintAnonTuple::from_ast(db, t)),
            crate::ast::TypeHint::AnonStruct(t) => TypeHint::AnonStruct(TypeHintAnonStruct::from_ast(db, t)),
            crate::ast::TypeHint::AnonEnum(t) => TypeHint::AnonEnum(TypeHintAnonEnum::from_ast(db, t)),
            crate::ast::TypeHint::List(t) => TypeHint::List(TypeHintList::from_ast(db, t)),
            crate::ast::TypeHint::Map(t) => TypeHint::Map(TypeHintMap::from_ast(db, t)),
            crate::ast::TypeHint::Set(t) => TypeHint::Set(TypeHintSet::from_ast(db, t)),
            crate::ast::TypeHint::Option(t) => TypeHint::Option(TypeHintOption::from_ast(db, t)),
            crate::ast::TypeHint::Result(t) => TypeHint::Result(TypeHintResult::from_ast(db, t)),
            crate::ast::TypeHint::Tensor(t) => TypeHint::Tensor(TypeHintTensor::from_ast(db, t)),
            crate::ast::TypeHint::Table(t) => TypeHint::Table(TypeHintTable::from_ast(db, t)),
            crate::ast::TypeHint::Data => TypeHint::Data,
            crate::ast::TypeHint::Error => TypeHint::Error,
            crate::ast::TypeHint::Alias(name) => TypeHint::Alias(name.as_str(db).to_string()),
            crate::ast::TypeHint::ParseError(e) => TypeHint::ParseError(TypeHintParseError::from_ast(db, e)),
        }
    }
}

impl TypeHintAnonTuple {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintAnonTuple<'db>) -> Self {
        TypeHintAnonTuple {
            fields: ast.fields.iter().map(|f| TypeHint::from_ast(db, f.clone())).collect(),
        }
    }
}

impl TypeHintAnonStruct {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintAnonStruct<'db>) -> Self {
        TypeHintAnonStruct {
            fields: ast.fields.iter().map(|f| TypeHintNamedField::from_ast(db, f.clone())).collect(),
        }
    }
}

impl TypeHintNamedField {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintNamedField<'db>) -> Self {
        TypeHintNamedField {
            name: ast.name.text(db).S(),
            type_hint: TypeHint::from_ast(db, *ast.type_hint.clone()),
        }
    }
}

impl TypeHintAnonEnum {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintAnonEnum<'db>) -> Self {
        TypeHintAnonEnum {
            variants: ast.variants.iter().map(|v| TypeHintEnumVariant::from_ast(db, v.clone())).collect(),
        }
    }
}

impl TypeHintEnumVariant {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintEnumVariant<'db>) -> Self {
        TypeHintEnumVariant {
            name: ast.name.text(db).S(),
            payload: ast.payload.clone().map(|p| Box::new(TypeHint::from_ast(db, *p))),
        }
    }
}

impl TypeHintList {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintList<'db>) -> Self {
        TypeHintList {
            element_type: Box::new(TypeHint::from_ast(db, *ast.element_type.clone())),
        }
    }
}

impl TypeHintMap {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintMap<'db>) -> Self {
        TypeHintMap {
            key_type: Box::new(TypeHint::from_ast(db, *ast.key_type.clone())),
            value_type: Box::new(TypeHint::from_ast(db, *ast.value_type.clone())),
        }
    }
}

impl TypeHintSet {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintSet<'db>) -> Self {
        TypeHintSet {
            element_type: Box::new(TypeHint::from_ast(db, *ast.element_type.clone())),
        }
    }
}

impl TypeHintOption {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintOption<'db>) -> Self {
        TypeHintOption {
            inner_type: Box::new(TypeHint::from_ast(db, *ast.inner_type.clone())),
        }
    }
}

impl TypeHintResult {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintResult<'db>) -> Self {
        TypeHintResult {
            inner_type: Box::new(TypeHint::from_ast(db, *ast.inner_type.clone())),
        }
    }
}

impl TypeHintTensor {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintTensor<'db>) -> Self {
        TypeHintTensor {
            element_type: Box::new(TypeHint::from_ast(db, *ast.element_type.clone())),
            rank: ast.rank,
        }
    }
}

impl TypeHintTable {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintTable<'db>) -> Self {
        TypeHintTable {
            columns: ast.columns.iter().map(|f| TypeHintNamedField::from_ast(db, f.clone())).collect(),
        }
    }
}


impl Expr {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::Expr<'db>) -> Self {
        match ast {
            crate::ast::Expr::True => Expr::True,
            crate::ast::Expr::False => Expr::False,
            crate::ast::Expr::Int(e) => Expr::Int(ExprInt::from_ast(db, e)),
            crate::ast::Expr::Float(e) => Expr::Float(ExprFloat::from_ast(db, e)),
            crate::ast::Expr::Hex(e) => Expr::Hex(ExprHex::from_ast(db, e)),
            crate::ast::Expr::String(e) => Expr::String(ExprString::from_ast(db, e)),
            crate::ast::Expr::AnonTuple(e) => Expr::AnonTuple(ExprAnonTuple::from_ast(db, e)),
            crate::ast::Expr::AnonStruct(e) => Expr::AnonStruct(ExprAnonStruct::from_ast(db, e)),
            crate::ast::Expr::AnonEnum(e) => Expr::AnonEnum(ExprAnonEnum::from_ast(db, e)),
            crate::ast::Expr::List(e) => Expr::List(ExprList::from_ast(db, e)),
            crate::ast::Expr::Map(e) => Expr::Map(ExprMap::from_ast(db, e)),
            crate::ast::Expr::Set(e) => Expr::Set(ExprSet::from_ast(db, e)),
            crate::ast::Expr::Tensor(e) => Expr::Tensor(ExprTensor::from_ast(db, e)),
            crate::ast::Expr::Table(e) => Expr::Table(ExprTable::from_ast(db, e)),
            crate::ast::Expr::None => Expr::None,
            crate::ast::Expr::Some(e) => Expr::Some(ExprSome::from_ast(db, e)),
            crate::ast::Expr::Ok(e) => Expr::Ok(ExprOk::from_ast(db, e)),
            crate::ast::Expr::Er(e) => Expr::Er(ExprEr::from_ast(db, e)),
            crate::ast::Expr::Data(e) => Expr::Data(ExprData::from_ast(db, e)),
            crate::ast::Expr::Error(e) => Expr::Error(ExprError::from_ast(db, e)),
            crate::ast::Expr::ParseError(e) => Expr::ParseError(ExprParseError::from_ast(db, e)),
        }
    }
}

impl ExprInt {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprInt<'db>) -> Self {
        ExprInt {
            value: ast.value.text(db).S(),
        }
    }
}

impl ExprFloat {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprFloat<'db>) -> Self {
        ExprFloat {
            value: ast.value.text(db).S(),
        }
    }
}

impl ExprHex {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprHex<'db>) -> Self {
        ExprHex {
            value: ast.value.text(db).S(),
        }
    }
}

impl ExprString {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprString<'db>) -> Self {
        ExprString {
            value: ast.value.text(db).S(),
        }
    }
}

impl ExprAnonTuple {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprAnonTuple<'db>) -> Self {
        ExprAnonTuple {
            elements: ast.elements.iter().map(|e| ExprFull::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprAnonStruct {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprAnonStruct<'db>) -> Self {
        ExprAnonStruct {
            fields: ast.fields.iter().map(|f| ExprStructField::from_ast(db, f.clone())).collect(),
        }
    }
}

impl ExprStructField {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprStructField<'db>) -> Self {
        ExprStructField {
            name: ast.name.text(db).S(),
            value: ExprFull::from_ast(db, ast.value),
        }
    }
}

impl ExprAnonEnum {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprAnonEnum<'db>) -> Self {
        ExprAnonEnum {
            variant_name: ast.variant_name.text(db).S(),
            payload: ast.payload.map(|p| Box::new(ExprFull::from_ast(db, p))),
        }
    }
}

impl ExprList {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprList<'db>) -> Self {
        ExprList {
            elements: ast.elements.iter().map(|e| ExprFull::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprMap {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprMap<'db>) -> Self {
        ExprMap {
            entries: ast.entries.iter().map(|e| ExprMapEntry::from_ast(db, e.clone())).collect(),
        }
    }
}

impl ExprMapEntry {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprMapEntry<'db>) -> Self {
        ExprMapEntry {
            key: ExprFull::from_ast(db, ast.key),
            value: ExprFull::from_ast(db, ast.value),
        }
    }
}

impl ExprSet {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprSet<'db>) -> Self {
        ExprSet {
            elements: ast.elements.iter().map(|e| ExprFull::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprTensor {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprTensor<'db>) -> Self {
        ExprTensor {
            shape: ast.shape.clone(),
            elements: ast.elements.iter().map(|e| ExprFull::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprTable {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprTable<'db>) -> Self {
        ExprTable {
            header: ast.header.iter().map(|n| n.as_str(db).S()).collect(),
            rows: ast.rows.iter().map(|r| ExprTableRow::from_ast(db, r.clone())).collect(),
        }
    }
}

impl ExprTableRow {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprTableRow<'db>) -> Self {
        ExprTableRow {
            elements: ast.elements.iter().map(|e| ExprFull::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprSome {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprSome<'db>) -> Self {
        ExprSome {
            payload: Box::new(ExprFull::from_ast(db, ast.payload)),
        }
    }
}

impl ExprOk {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprOk<'db>) -> Self {
        ExprOk {
            payload: Box::new(ExprFull::from_ast(db, ast.payload)),
        }
    }
}

impl ExprEr {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprEr<'db>) -> Self {
        ExprEr {
            payload: Box::new(ExprFull::from_ast(db, ast.payload)),
        }
    }
}

impl ExprData {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprData<'db>) -> Self {
        ExprData {
            value: Box::new(ExprFull::from_ast(db, ast.value)),
        }
    }
}

impl ExprError {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprError<'db>) -> Self {
        ExprError {
            value: Box::new(ExprFull::from_ast(db, ast.value)),
        }
    }
}

impl TypeHintParseError {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintParseError<'db>) -> Self {
        TypeHintParseError {
            message: ast.message.as_str(db).to_string(),
        }
    }
}

impl ExprParseError {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprParseError<'db>) -> Self {
        ExprParseError {
            message: ast.message.as_str(db).to_string(),
        }
    }
}
