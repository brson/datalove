use rmx::prelude::*;
use bct::text::{InternedText, Text};
use datalove_diagnostic::ByteSpan;

/// Result of parsing containing the root expression and span side table.
///
/// We use Vec for the expr_spans to keep the implementation simple.
/// For lookup, consumers can iterate through the vec to find the span for a given ExprFull.
///
/// This is a regular struct, not a Salsa tracked struct, because Salsa
/// tracked structs cannot contain lifetime-bound collections.
#[derive(Clone, PartialEq, Eq)]
pub struct ParseResult<'db> {
    pub expr: ExprFull<'db>,
    pub expr_spans: Vec<(ExprFull<'db>, Text<'db>, ByteSpan)>,
}

impl<'db> ParseResult<'db> {
    pub fn new(
        expr: ExprFull<'db>,
        expr_spans: Vec<(ExprFull<'db>, Text<'db>, ByteSpan)>,
    ) -> Self {
        ParseResult { expr, expr_spans }
    }
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
    Int,
    String,
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
    Data,
    Error,
    ParseError(TypeHintParseError<'db>),
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
    /// Some integer type. Tycheck will decide.
    Int(ExprInt<'db>),
    /// Some float type. Tycheck will decide.
    Float(ExprFloat<'db>),
    String(ExprString<'db>),
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
    Data(ExprData<'db>),
    Err(ExprErr<'db>),
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
pub struct ExprString<'db> {
    pub value: InternedText<'db>,
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
pub struct ExprData<'db> {
    pub value: ExprFull<'db>,
}

#[salsa::tracked]
pub struct ExprErr<'db> {
    pub value: ExprFull<'db>,
}

#[salsa::tracked]
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
