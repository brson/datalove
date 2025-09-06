use rmx::prelude::*;
use bct::text::InternedText;

#[salsa::tracked]
pub struct ExprFull<'db> {
    #[returns(ref)]
    pub type_hint: TypeHintAndHeap<'db>,
    #[returns(ref)]
    pub expr: ExprAndHeap<'db>,
}

#[salsa::tracked]
pub struct TypeHintAndHeap<'db> {
    pub heap: Heap,
    pub type_hint: TypeHint,
}

#[derive(Copy, Clone, Debug, Hash)]
#[derive(salsa::Update)]
pub enum TypeHint {
    Bool,
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

