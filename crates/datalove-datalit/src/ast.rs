use rmx::prelude::*;
use bct::text::{InternedText, Text};
use datalove_diagnostic::ByteSpan;

/// Span entry for a parsed expression, using salsa IDs for storage.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ParseSpanEntry {
    pub expr_id: salsa::Id,
    pub text_id: salsa::Id,
    pub span: ByteSpan,
}

impl ParseSpanEntry {
    pub fn new(expr_id: salsa::Id, text_id: salsa::Id, span: ByteSpan) -> Self {
        ParseSpanEntry { expr_id, text_id, span }
    }
}

/// Result of parsing containing the root expression and span side table.
#[salsa::tracked]
pub struct ParseResult<'db> {
    pub expr: ExprFull<'db>,
    pub expr_spans: Vec<ParseSpanEntry>,
}

#[salsa::tracked]
pub struct ExprFull<'db> {
    pub type_hint: Option<TypeHintAndHeap<'db>>,
    #[returns(ref)]
    pub expr: ExprAndHeap<'db>,
}

#[derive(Copy, Clone, Hash, Debug)]
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
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    F32,
    F64,
    Int,
    String,
    AnonTuple(TypeHintAnonTuple<'db>),
    AnonStruct(TypeHintAnonStruct<'db>),
    AnonEnum(TypeHintAnonEnum<'db>),
    List(TypeHintList<'db>),
    Map(TypeHintMap<'db>),
    Set(TypeHintSet<'db>),
    Option(TypeHintOption<'db>),
    Result(TypeHintResult<'db>),
    Tensor(TypeHintTensor<'db>),
    Data,
    Error,
    ParseError(TypeHintParseError<'db>),
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeHintAnonTuple<'db> {
    pub fields: Vec<TypeHintAndHeap<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeHintAnonStruct<'db> {
    pub fields: Vec<TypeHintNamedField<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeHintNamedField<'db> {
    pub name: InternedText<'db>,
    pub type_hint: TypeHintAndHeap<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeHintAnonEnum<'db> {
    pub variants: Vec<TypeHintEnumVariant<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeHintEnumVariant<'db> {
    pub name: InternedText<'db>,
    pub payload: Option<TypeHintAndHeap<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeHintList<'db> {
    pub element_type: TypeHintAndHeap<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeHintMap<'db> {
    pub key_type: TypeHintAndHeap<'db>,
    pub value_type: TypeHintAndHeap<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeHintSet<'db> {
    pub element_type: TypeHintAndHeap<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeHintOption<'db> {
    pub inner_type: TypeHintAndHeap<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeHintResult<'db> {
    pub inner_type: TypeHintAndHeap<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeHintTensor<'db> {
    pub element_type: TypeHintAndHeap<'db>,
    pub rank: u32,
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
    /// Some integer type. Tycheck will decide.
    Int(ExprInt<'db>),
    /// Some float type. Tycheck will decide.
    Float(ExprFloat<'db>),
    /// Hex literal. Can be int or f32 bit pattern depending on type context.
    Hex(ExprHex<'db>),
    String(ExprString<'db>),
    AnonTuple(ExprAnonTuple<'db>),
    AnonStruct(ExprAnonStruct<'db>),
    AnonEnum(ExprAnonEnum<'db>),
    List(ExprList<'db>),
    Map(ExprMap<'db>),
    Set(ExprSet<'db>),
    Tensor(ExprTensor<'db>),
    None,
    Some(ExprSome<'db>),
    Ok(ExprOk<'db>),
    Er(ExprEr<'db>),
    Data(ExprData<'db>),
    Error(ExprError<'db>),
    ParseError(ExprParseError<'db>),
}

#[salsa::tracked]
pub struct ExprInt<'db> {
    pub value: InternedText<'db>,
}

#[salsa::tracked]
pub struct ExprFloat<'db> {
    pub value: InternedText<'db>,
}

#[salsa::tracked]
pub struct ExprHex<'db> {
    pub value: InternedText<'db>,
}

#[salsa::tracked]
pub struct ExprString<'db> {
    pub value: InternedText<'db>,
}

#[salsa::tracked]
pub struct ExprAnonTuple<'db> {
    pub elements: Vec<ExprFull<'db>>,
}

#[salsa::tracked]
pub struct ExprAnonStruct<'db> {
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
pub struct ExprTensor<'db> {
    pub shape: Vec<u32>,
    pub elements: Vec<ExprFull<'db>>,
}

#[salsa::tracked]
pub struct ExprSome<'db> {
    pub payload: ExprFull<'db>,
}

#[salsa::tracked]
pub struct ExprOk<'db> {
    pub payload: ExprFull<'db>,
}

#[salsa::tracked]
pub struct ExprEr<'db> {
    pub payload: ExprFull<'db>,
}

#[salsa::tracked]
pub struct ExprData<'db> {
    pub value: ExprFull<'db>,
}

#[salsa::tracked]
pub struct ExprError<'db> {
    pub value: ExprFull<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeHintParseError<'db> {
    pub text: Text<'db>,
    pub span: ByteSpan,
    pub message: InternedText<'db>,
}

#[salsa::tracked]
pub struct ExprParseError<'db> {
    pub text: Text<'db>,
    pub span: ByteSpan,
    pub message: InternedText<'db>,
}
