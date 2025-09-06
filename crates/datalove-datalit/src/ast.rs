use rmx::prelude::*;
use bct::text::InternedText;

#[salsa::tracked]
pub struct ExprFull<'db> {
    #[returns(ref)]
    pub type_hint: TypeHintAndHeap<'db>,
    #[returns(ref)]
    pub expr: ExprAndHeap<'db>,
}

#[derive(Copy, Clone, Debug, Hash)]
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

#[derive(Copy, Clone, Debug, Hash)]
#[derive(salsa::Update)]
pub enum TypeHint<'db> {
    Bool,
    U32,
    F32,
    Int,
    Nil,
    Token(TypeHintToken<'db>),
    AnonTuple(TypeHintAnonTuple),
    NamedTuple(TypeHintNamedTuple<'db>),
}

#[derive(Copy, Clone, Debug, Hash)]
#[derive(salsa::Update)]
pub struct TypeHintToken<'db> {
    pub name: InternedText<'db>,
}

#[derive(Copy, Clone, Debug, Hash)]
#[derive(salsa::Update)]
pub struct TypeHintAnonTuple {
}

#[derive(Copy, Clone, Debug, Hash)]
#[derive(salsa::Update)]
pub struct TypeHintNamedTuple<'db> {
    pub name: InternedText<'db>,
}

#[salsa::tracked]
pub struct ExprAndHeap<'db> {
    pub heap: Heap,
    pub expr: Expr,
}

#[derive(Copy, Clone, Debug, Hash)]
#[derive(salsa::Update)]
pub enum Expr {
    True,
    False,
}

