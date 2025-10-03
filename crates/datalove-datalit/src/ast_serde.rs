use rmx::prelude::*;

extern crate rmx;
use rmx::serde as serde;

/// Serializable AST for test snapshots.
/// Mirrors the salsa-based AST in ast.rs but uses regular Rust types.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprFull {
    pub type_hint: TypeHintAndHeap,
    pub expr: ExprAndHeap,
}

#[derive(Debug, Copy, Clone, serde::Serialize, serde::Deserialize)]
pub enum Heap {
    Local,
    Global,
    Omitted,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TypeHintAndHeap {
    pub heap: Heap,
    pub type_hint: TypeHint,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TypeHint {
    Bool,
    U32,
    F32,
    Int,
    Nil,
    String,
    Token(TypeHintToken),
    AnonTuple(TypeHintAnonTuple),
    NamedTuple(TypeHintNamedTuple),
    AnonStruct(TypeHintAnonStruct),
    NamedStruct(TypeHintNamedStruct),
    AnonEnum(TypeHintAnonEnum),
    NamedEnum(TypeHintNamedEnum),
    List(TypeHintList),
    Map(TypeHintMap),
    Set(TypeHintSet),
    Option(TypeHintOption),
    Result(TypeHintResult),
    Data,
    Error,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TypeHintToken {
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TypeHintAnonTuple {
    pub fields: Vec<TypeHintAndHeap>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TypeHintNamedTuple {
    pub name: String,
    pub fields: Vec<TypeHintAndHeap>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TypeHintAnonStruct {
    pub fields: Vec<TypeHintNamedField>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TypeHintNamedStruct {
    pub name: String,
    pub fields: Vec<TypeHintNamedField>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TypeHintNamedField {
    pub name: String,
    pub type_hint: TypeHintAndHeap,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TypeHintAnonEnum {
    pub variants: Vec<TypeHintEnumVariant>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TypeHintNamedEnum {
    pub name: String,
    pub variants: Vec<TypeHintEnumVariant>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TypeHintEnumVariant {
    pub name: String,
    pub payload: Option<Box<TypeHintAndHeap>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TypeHintList {
    pub element_type: Box<TypeHintAndHeap>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TypeHintMap {
    pub key_type: Box<TypeHintAndHeap>,
    pub value_type: Box<TypeHintAndHeap>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TypeHintSet {
    pub element_type: Box<TypeHintAndHeap>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TypeHintOption {
    pub inner_type: Box<TypeHintAndHeap>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TypeHintResult {
    pub inner_type: Box<TypeHintAndHeap>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprAndHeap {
    pub heap: Heap,
    pub expr: Expr,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Expr {
    True,
    False,
    Nil,
    Int(ExprInt),
    U32(ExprU32),
    F32(ExprF32),
    String(ExprString),
    Token(ExprToken),
    AnonTuple(ExprAnonTuple),
    NamedTuple(ExprNamedTuple),
    AnonStruct(ExprAnonStruct),
    NamedStruct(ExprNamedStruct),
    AnonEnum(ExprAnonEnum),
    NamedEnum(ExprNamedEnum),
    List(ExprList),
    Map(ExprMap),
    Set(ExprSet),
    None,
    Err(ExprErr),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprInt {
    pub value: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprU32 {
    pub value: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprF32 {
    pub value: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprString {
    pub value: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprToken {
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprAnonTuple {
    pub elements: Vec<ExprFull>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprNamedTuple {
    pub name: String,
    pub elements: Vec<ExprFull>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprAnonStruct {
    pub fields: Vec<ExprStructField>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprNamedStruct {
    pub name: String,
    pub fields: Vec<ExprStructField>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprStructField {
    pub name: String,
    pub value: ExprFull,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprAnonEnum {
    pub variant_name: String,
    pub payload: Option<Box<ExprFull>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprNamedEnum {
    pub enum_name: String,
    pub variant_name: String,
    pub payload: Option<Box<ExprFull>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprList {
    pub elements: Vec<ExprFull>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprMap {
    pub entries: Vec<ExprMapEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprMapEntry {
    pub key: ExprFull,
    pub value: ExprFull,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprSet {
    pub elements: Vec<ExprFull>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprErr {
    pub value: Box<ExprFull>,
}

/// Conversion from salsa AST to serializable AST.
impl ExprFull {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprFull<'db>) -> Self {
        ExprFull {
            type_hint: TypeHintAndHeap::from_ast(db, *ast.type_hint(db)),
            expr: ExprAndHeap::from_ast(db, *ast.expr(db)),
        }
    }
}

impl Heap {
    pub fn from_ast(ast: crate::ast::Heap) -> Self {
        match ast {
            crate::ast::Heap::Local => Heap::Local,
            crate::ast::Heap::Global => Heap::Global,
            crate::ast::Heap::Omitted => Heap::Omitted,
        }
    }
}

impl TypeHintAndHeap {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintAndHeap<'db>) -> Self {
        TypeHintAndHeap {
            heap: Heap::from_ast(ast.heap(db)),
            type_hint: TypeHint::from_ast(db, ast.type_hint(db)),
        }
    }
}

impl TypeHint {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHint<'db>) -> Self {
        match ast {
            crate::ast::TypeHint::Bool => TypeHint::Bool,
            crate::ast::TypeHint::U32 => TypeHint::U32,
            crate::ast::TypeHint::F32 => TypeHint::F32,
            crate::ast::TypeHint::Int => TypeHint::Int,
            crate::ast::TypeHint::Nil => TypeHint::Nil,
            crate::ast::TypeHint::String => TypeHint::String,
            crate::ast::TypeHint::Token(t) => TypeHint::Token(TypeHintToken::from_ast(db, t)),
            crate::ast::TypeHint::AnonTuple(t) => TypeHint::AnonTuple(TypeHintAnonTuple::from_ast(db, t)),
            crate::ast::TypeHint::NamedTuple(t) => TypeHint::NamedTuple(TypeHintNamedTuple::from_ast(db, t)),
            crate::ast::TypeHint::AnonStruct(t) => TypeHint::AnonStruct(TypeHintAnonStruct::from_ast(db, t)),
            crate::ast::TypeHint::NamedStruct(t) => TypeHint::NamedStruct(TypeHintNamedStruct::from_ast(db, t)),
            crate::ast::TypeHint::AnonEnum(t) => TypeHint::AnonEnum(TypeHintAnonEnum::from_ast(db, t)),
            crate::ast::TypeHint::NamedEnum(t) => TypeHint::NamedEnum(TypeHintNamedEnum::from_ast(db, t)),
            crate::ast::TypeHint::List(t) => TypeHint::List(TypeHintList::from_ast(db, t)),
            crate::ast::TypeHint::Map(t) => TypeHint::Map(TypeHintMap::from_ast(db, t)),
            crate::ast::TypeHint::Set(t) => TypeHint::Set(TypeHintSet::from_ast(db, t)),
            crate::ast::TypeHint::Option(t) => TypeHint::Option(TypeHintOption::from_ast(db, t)),
            crate::ast::TypeHint::Result(t) => TypeHint::Result(TypeHintResult::from_ast(db, t)),
            crate::ast::TypeHint::Data => TypeHint::Data,
            crate::ast::TypeHint::Error => TypeHint::Error,
        }
    }
}

impl TypeHintToken {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintToken<'db>) -> Self {
        TypeHintToken {
            name: ast.name(db).text(db).S(),
        }
    }
}

impl TypeHintAnonTuple {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintAnonTuple<'db>) -> Self {
        TypeHintAnonTuple {
            fields: ast.fields(db).iter().map(|f| TypeHintAndHeap::from_ast(db, *f)).collect(),
        }
    }
}

impl TypeHintNamedTuple {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintNamedTuple<'db>) -> Self {
        TypeHintNamedTuple {
            name: ast.name(db).text(db).S(),
            fields: ast.fields(db).iter().map(|f| TypeHintAndHeap::from_ast(db, *f)).collect(),
        }
    }
}

impl TypeHintAnonStruct {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintAnonStruct<'db>) -> Self {
        TypeHintAnonStruct {
            fields: ast.fields(db).iter().map(|f| TypeHintNamedField::from_ast(db, *f)).collect(),
        }
    }
}

impl TypeHintNamedStruct {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintNamedStruct<'db>) -> Self {
        TypeHintNamedStruct {
            name: ast.name(db).text(db).S(),
            fields: ast.fields(db).iter().map(|f| TypeHintNamedField::from_ast(db, *f)).collect(),
        }
    }
}

impl TypeHintNamedField {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintNamedField<'db>) -> Self {
        TypeHintNamedField {
            name: ast.name(db).text(db).S(),
            type_hint: TypeHintAndHeap::from_ast(db, ast.type_hint(db)),
        }
    }
}

impl TypeHintAnonEnum {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintAnonEnum<'db>) -> Self {
        TypeHintAnonEnum {
            variants: ast.variants(db).iter().map(|v| TypeHintEnumVariant::from_ast(db, *v)).collect(),
        }
    }
}

impl TypeHintNamedEnum {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintNamedEnum<'db>) -> Self {
        TypeHintNamedEnum {
            name: ast.name(db).text(db).S(),
            variants: ast.variants(db).iter().map(|v| TypeHintEnumVariant::from_ast(db, *v)).collect(),
        }
    }
}

impl TypeHintEnumVariant {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintEnumVariant<'db>) -> Self {
        TypeHintEnumVariant {
            name: ast.name(db).text(db).S(),
            payload: ast.payload(db).map(|p| Box::new(TypeHintAndHeap::from_ast(db, p))),
        }
    }
}

impl TypeHintList {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintList<'db>) -> Self {
        TypeHintList {
            element_type: Box::new(TypeHintAndHeap::from_ast(db, ast.element_type(db))),
        }
    }
}

impl TypeHintMap {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintMap<'db>) -> Self {
        TypeHintMap {
            key_type: Box::new(TypeHintAndHeap::from_ast(db, ast.key_type(db))),
            value_type: Box::new(TypeHintAndHeap::from_ast(db, ast.value_type(db))),
        }
    }
}

impl TypeHintSet {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintSet<'db>) -> Self {
        TypeHintSet {
            element_type: Box::new(TypeHintAndHeap::from_ast(db, ast.element_type(db))),
        }
    }
}

impl TypeHintOption {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintOption<'db>) -> Self {
        TypeHintOption {
            inner_type: Box::new(TypeHintAndHeap::from_ast(db, ast.inner_type(db))),
        }
    }
}

impl TypeHintResult {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::TypeHintResult<'db>) -> Self {
        TypeHintResult {
            inner_type: Box::new(TypeHintAndHeap::from_ast(db, ast.inner_type(db))),
        }
    }
}

impl ExprAndHeap {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprAndHeap<'db>) -> Self {
        ExprAndHeap {
            heap: Heap::from_ast(ast.heap(db)),
            expr: Expr::from_ast(db, ast.expr(db)),
        }
    }
}

impl Expr {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::Expr<'db>) -> Self {
        match ast {
            crate::ast::Expr::True => Expr::True,
            crate::ast::Expr::False => Expr::False,
            crate::ast::Expr::Nil => Expr::Nil,
            crate::ast::Expr::Int(e) => Expr::Int(ExprInt::from_ast(db, e)),
            crate::ast::Expr::U32(e) => Expr::U32(ExprU32::from_ast(db, e)),
            crate::ast::Expr::F32(e) => Expr::F32(ExprF32::from_ast(db, e)),
            crate::ast::Expr::String(e) => Expr::String(ExprString::from_ast(db, e)),
            crate::ast::Expr::Token(e) => Expr::Token(ExprToken::from_ast(db, e)),
            crate::ast::Expr::AnonTuple(e) => Expr::AnonTuple(ExprAnonTuple::from_ast(db, e)),
            crate::ast::Expr::NamedTuple(e) => Expr::NamedTuple(ExprNamedTuple::from_ast(db, e)),
            crate::ast::Expr::AnonStruct(e) => Expr::AnonStruct(ExprAnonStruct::from_ast(db, e)),
            crate::ast::Expr::NamedStruct(e) => Expr::NamedStruct(ExprNamedStruct::from_ast(db, e)),
            crate::ast::Expr::AnonEnum(e) => Expr::AnonEnum(ExprAnonEnum::from_ast(db, e)),
            crate::ast::Expr::NamedEnum(e) => Expr::NamedEnum(ExprNamedEnum::from_ast(db, e)),
            crate::ast::Expr::List(e) => Expr::List(ExprList::from_ast(db, e)),
            crate::ast::Expr::Map(e) => Expr::Map(ExprMap::from_ast(db, e)),
            crate::ast::Expr::Set(e) => Expr::Set(ExprSet::from_ast(db, e)),
            crate::ast::Expr::None => Expr::None,
            crate::ast::Expr::Err(e) => Expr::Err(ExprErr::from_ast(db, e)),
        }
    }
}

impl ExprInt {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprInt<'db>) -> Self {
        ExprInt {
            value: ast.value(db).text(db).S(),
        }
    }
}

impl ExprU32 {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprU32<'db>) -> Self {
        ExprU32 {
            value: ast.value(db),
        }
    }
}

impl ExprF32 {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprF32<'db>) -> Self {
        ExprF32 {
            value: ast.value(db).text(db).S(),
        }
    }
}

impl ExprString {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprString<'db>) -> Self {
        ExprString {
            value: ast.value(db).text(db).S(),
        }
    }
}

impl ExprToken {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprToken<'db>) -> Self {
        ExprToken {
            name: ast.name(db).text(db).S(),
        }
    }
}

impl ExprAnonTuple {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprAnonTuple<'db>) -> Self {
        ExprAnonTuple {
            elements: ast.elements(db).iter().map(|e| ExprFull::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprNamedTuple {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprNamedTuple<'db>) -> Self {
        ExprNamedTuple {
            name: ast.name(db).text(db).S(),
            elements: ast.elements(db).iter().map(|e| ExprFull::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprAnonStruct {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprAnonStruct<'db>) -> Self {
        ExprAnonStruct {
            fields: ast.fields(db).iter().map(|f| ExprStructField::from_ast(db, *f)).collect(),
        }
    }
}

impl ExprNamedStruct {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprNamedStruct<'db>) -> Self {
        ExprNamedStruct {
            name: ast.name(db).text(db).S(),
            fields: ast.fields(db).iter().map(|f| ExprStructField::from_ast(db, *f)).collect(),
        }
    }
}

impl ExprStructField {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprStructField<'db>) -> Self {
        ExprStructField {
            name: ast.name(db).text(db).S(),
            value: ExprFull::from_ast(db, ast.value(db)),
        }
    }
}

impl ExprAnonEnum {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprAnonEnum<'db>) -> Self {
        ExprAnonEnum {
            variant_name: ast.variant_name(db).text(db).S(),
            payload: ast.payload(db).map(|p| Box::new(ExprFull::from_ast(db, p))),
        }
    }
}

impl ExprNamedEnum {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprNamedEnum<'db>) -> Self {
        ExprNamedEnum {
            enum_name: ast.enum_name(db).text(db).S(),
            variant_name: ast.variant_name(db).text(db).S(),
            payload: ast.payload(db).map(|p| Box::new(ExprFull::from_ast(db, p))),
        }
    }
}

impl ExprList {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprList<'db>) -> Self {
        ExprList {
            elements: ast.elements(db).iter().map(|e| ExprFull::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprMap {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprMap<'db>) -> Self {
        ExprMap {
            entries: ast.entries(db).iter().map(|e| ExprMapEntry::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprMapEntry {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprMapEntry<'db>) -> Self {
        ExprMapEntry {
            key: ExprFull::from_ast(db, ast.key(db)),
            value: ExprFull::from_ast(db, ast.value(db)),
        }
    }
}

impl ExprSet {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprSet<'db>) -> Self {
        ExprSet {
            elements: ast.elements(db).iter().map(|e| ExprFull::from_ast(db, *e)).collect(),
        }
    }
}

impl ExprErr {
    pub fn from_ast<'db>(db: &'db dyn crate::Db, ast: crate::ast::ExprErr<'db>) -> Self {
        ExprErr {
            value: Box::new(ExprFull::from_ast(db, ast.value(db))),
        }
    }
}
