use rmx::prelude::*;
use bct::text::InternedText;

#[salsa::tracked]
pub struct ExprFull<'db> {
    pub type_hint: Option<TypeHintAndHeap<'db>>,
    #[returns(ref)]
    pub expr: ExprAndHeap<'db>,
}

#[derive(Copy, Clone, Hash)]
#[derive(salsa::Update)]
pub enum Heap {
    Local,
    Global,
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
    String,
    Token(TypeHintToken<'db>),
    AnonTuple(TypeHintAnonTuple<'db>),
    NamedTuple(TypeHintNamedTuple<'db>),
    AnonStruct(TypeHintAnonStruct<'db>),
    NamedStruct(TypeHintNamedStruct<'db>),
    AnonEnum(TypeHintAnonEnum<'db>),
    NamedEnum(TypeHintNamedEnum<'db>),
    List(TypeHintList<'db>),
    Map(TypeHintMap<'db>),
    Set(TypeHintSet<'db>),
    Option(TypeHintOption<'db>),
    Result(TypeHintResult<'db>),
    Error,
    ParseError(TypeHintParseError<'db>),
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
pub struct TypeHintAnonEnum<'db> {
    pub variants: Vec<TypeHintEnumVariant<'db>>,
}

#[salsa::tracked]
pub struct TypeHintNamedEnum<'db> {
    pub name: InternedText<'db>,
    pub variants: Vec<TypeHintEnumVariant<'db>>,
}

#[salsa::tracked]
pub struct TypeHintEnumVariant<'db> {
    pub name: InternedText<'db>,
    pub payload: Option<TypeHintAndHeap<'db>>,
}

#[salsa::tracked]
pub struct TypeHintList<'db> {
    pub element_type: TypeHintAndHeap<'db>,
}

#[salsa::tracked]
pub struct TypeHintMap<'db> {
    pub key_type: TypeHintAndHeap<'db>,
    pub value_type: TypeHintAndHeap<'db>,
}

#[salsa::tracked]
pub struct TypeHintSet<'db> {
    pub element_type: TypeHintAndHeap<'db>,
}

#[salsa::tracked]
pub struct TypeHintOption<'db> {
    pub inner_type: TypeHintAndHeap<'db>,
}

#[salsa::tracked]
pub struct TypeHintResult<'db> {
    pub inner_type: TypeHintAndHeap<'db>,
}

#[salsa::tracked]
pub struct ExprAndHeap<'db> {
    pub heap: Heap,
    pub expr: Expr<'db>,
}

#[derive(Clone, Hash)]
#[derive(salsa::Update)]
pub enum Expr<'db> {
    True,
    False,
    Nil,
    Int(ExprInt<'db>),
    U32(ExprU32<'db>),
    F32(ExprF32<'db>),
    String(ExprString<'db>),
    Token(ExprToken<'db>),
    AnonTuple(ExprAnonTuple<'db>),
    NamedTuple(ExprNamedTuple<'db>),
    AnonStruct(ExprAnonStruct<'db>),
    NamedStruct(ExprNamedStruct<'db>),
    AnonEnum(ExprAnonEnum<'db>),
    NamedEnum(ExprNamedEnum<'db>),
    List(ExprList<'db>),
    Map(ExprMap<'db>),
    Set(ExprSet<'db>),
    None,
    Err(ExprErr<'db>),
    ParseError(ExprParseError<'db>),
}

#[salsa::tracked]
pub struct ExprInt<'db> {
    pub value: InternedText<'db>,
}

#[salsa::tracked]
pub struct ExprU32<'db> {
    pub value: u32,
}

#[salsa::tracked]
pub struct ExprF32<'db> {
    pub value: InternedText<'db>,
}

#[salsa::tracked]
pub struct ExprString<'db> {
    pub value: InternedText<'db>,
}

#[salsa::tracked]
pub struct ExprToken<'db> {
    pub name: InternedText<'db>,
}

#[salsa::tracked]
pub struct ExprAnonTuple<'db> {
    pub elements: Vec<ExprFull<'db>>,
}

#[salsa::tracked]
pub struct ExprNamedTuple<'db> {
    pub name: InternedText<'db>,
    pub elements: Vec<ExprFull<'db>>,
}

#[salsa::tracked]
pub struct ExprAnonStruct<'db> {
    pub fields: Vec<ExprStructField<'db>>,
}

#[salsa::tracked]
pub struct ExprNamedStruct<'db> {
    pub name: InternedText<'db>,
    pub fields: Vec<ExprStructField<'db>>,
}

#[salsa::tracked]
pub struct ExprStructField<'db> {
    pub name: InternedText<'db>,
    pub value: ExprFull<'db>,
}

#[salsa::tracked]
pub struct ExprAnonEnum<'db> {
    pub variant_name: InternedText<'db>,
    pub payload: Option<ExprFull<'db>>,
}

#[salsa::tracked]
pub struct ExprNamedEnum<'db> {
    pub enum_name: InternedText<'db>,
    pub variant_name: InternedText<'db>,
    pub payload: Option<ExprFull<'db>>,
}

#[salsa::tracked]
pub struct ExprList<'db> {
    pub elements: Vec<ExprFull<'db>>,
}

#[salsa::tracked]
pub struct ExprMap<'db> {
    pub entries: Vec<ExprMapEntry<'db>>,
}

#[salsa::tracked]
pub struct ExprMapEntry<'db> {
    pub key: ExprFull<'db>,
    pub value: ExprFull<'db>,
}

#[salsa::tracked]
pub struct ExprSet<'db> {
    pub elements: Vec<ExprFull<'db>>,
}

#[salsa::tracked]
pub struct ExprErr<'db> {
    pub value: ExprFull<'db>,
}

#[salsa::tracked]
pub struct TypeHintParseError<'db> {
    pub message: InternedText<'db>,
}

#[salsa::tracked]
pub struct ExprParseError<'db> {
    pub message: InternedText<'db>,
}
