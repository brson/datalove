use rmx::prelude::*;
use bct::text::InternedText;

#[salsa::tracked]
pub struct ExprFull<'db> {
    #[returns(ref)]
    pub type_hint: TypeHintAndHeap<'db>,
    #[returns(ref)]
    pub expr: ExprAndHeap<'db>,
}

#[derive(Copy, Clone, Hash)]
#[derive(salsa::Update)]
pub enum Heap {
    Local,
    Global,
    None,
    Omitted,
}

#[salsa::tracked]
pub struct TypeHintAndHeap<'db> {
    pub heap: Heap,
    pub type_hint: TypeHint<'db>,
}

#[derive(Clone, Hash)]
#[derive(salsa::Update)]
pub enum TypeHint<'db> {
    Bool,
    U32,
    F32,
    Int,
    Nil,
    Token(TypeHintToken<'db>),
    AnonTuple(TypeHintAnonTuple<'db>),
    NamedTuple(TypeHintNamedTuple<'db>),
    AnonStruct(TypeHintAnonStruct<'db>),
    NamedStruct(TypeHintNamedStruct<'db>),
}

#[salsa::tracked]
pub struct TypeHintToken<'db> {
    pub name: InternedText<'db>,
}

#[salsa::tracked]
pub struct TypeHintAnonTuple<'db> {
    pub fields: Vec<TypeHintAndHeap<'db>>,
}

#[salsa::tracked]
pub struct TypeHintNamedTuple<'db> {
    pub name: InternedText<'db>,
    pub fields: Vec<TypeHintAndHeap<'db>>,
}

#[salsa::tracked]
pub struct TypeHintAnonStruct<'db> {
    pub fields: Vec<TypeHintNamedField<'db>>,
}

#[salsa::tracked]
pub struct TypeHintNamedStruct<'db> {
    pub name: InternedText<'db>,
    pub fields: Vec<TypeHintNamedField<'db>>,
}

#[salsa::tracked]
pub struct TypeHintNamedField<'db> {
    pub name: InternedText<'db>,
    pub type_hint: TypeHintAndHeap<'db>,
}

#[salsa::tracked]
pub struct ExprAndHeap<'db> {
    pub heap: Heap,
    pub expr: Expr,
}

#[derive(Copy, Clone, Hash)]
#[derive(salsa::Update)]
pub enum Expr {
    True,
    False,
}

