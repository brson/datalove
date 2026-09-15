use rmx::prelude::*;
use bct::text::{InternedText, Text};
use bct::text::ByteSpan;

/// Span entry for a parsed expression.
///
/// Keyed by the expression's position in the parse rather than its salsa id.
/// An id is only meaningful in the revision that minted it, so a table that
/// stores one is correct only while the table and the ids in it are produced
/// together; an index is ours and means the same thing whenever it is read.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ParseSpanEntry {
    pub expr_index: u32,
    pub source: bct::input::Source,
    pub span: ByteSpan,
}

impl ParseSpanEntry {
    pub fn new(expr_index: u32, source: bct::input::Source, span: ByteSpan) -> Self {
        ParseSpanEntry { expr_index, source, span }
    }
}

/// Result of parsing containing the root expression and span side table.
#[salsa::tracked]
pub struct ParseResult<'db> {
    #[returns(copy)]
    pub expr: ExprFull<'db>,
    #[returns(clone)]
    pub expr_spans: Vec<ParseSpanEntry>,
}

#[salsa::tracked]
pub struct ExprFull<'db> {
    /// Position of this expression in the parse that produced it.
    ///
    /// `None` for an expression that came from no source: the typechecker
    /// builds a few to re-check a literal without its type hint, and the AST
    /// generator makes them up wholesale. Neither has a span to look up.
    #[returns(copy)]
    pub local_index: Option<u32>,
    #[returns(clone)]
    pub type_hint: Option<TypeHint<'db>>,
    #[returns(ref)]
    pub expr: Expr<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
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
    Index,
    Offset,
    F32,
    F64,
    Int,
    String,
    AnonTuple(TypeHintAnonTuple<'db>),
    AnonStruct(TypeHintAnonStruct<'db>),

    List(TypeHintList<'db>),
    Map(TypeHintMap<'db>),
    Set(TypeHintSet<'db>),
    Option(TypeHintOption<'db>),
    Result(TypeHintResult<'db>),
    Tensor(TypeHintTensor<'db>),
    Table(TypeHintTable<'db>),
    Data,
    Error,
    Alias(TypeHintAlias<'db>),
    Atom(TypeHintAtom<'db>),
    Term(TypeHintTerm<'db>),
    Enum(TypeHintEnum<'db>),
    ParseError(TypeHintParseError<'db>),
}


#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintAnonTuple<'db> {
    pub fields: Vec<TypeHint<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintAnonStruct<'db> {
    pub fields: Vec<TypeHintNamedField<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintNamedField<'db> {
    pub name: InternedText<'db>,
    pub type_hint: Box<TypeHint<'db>>,
}


#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintList<'db> {
    pub element_type: Box<TypeHint<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintMap<'db> {
    pub key_type: Box<TypeHint<'db>>,
    pub value_type: Box<TypeHint<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintSet<'db> {
    pub element_type: Box<TypeHint<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintOption<'db> {
    pub inner_type: Box<TypeHint<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintResult<'db> {
    pub inner_type: Box<TypeHint<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintTensor<'db> {
    pub element_type: Box<TypeHint<'db>>,
    pub rank: u32,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintTable<'db> {
    pub columns: Vec<TypeHintNamedField<'db>>,
}

/// A bare name in type position: a type alias, or a generic's type parameter.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintAlias<'db> {
    pub name: InternedText<'db>,
    /// Position of this alias in the parse that produced it, which is what a
    /// span table is keyed by.
    ///
    /// The span itself is kept out of the AST so that an edit which moves
    /// text without changing it leaves memoized work alone; see
    /// `DatafunSpans`. `None` for an alias that came from no source, which is
    /// what the worldfile generator builds to print.
    pub local_index: Option<u32>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintAtom<'db> {
    pub name: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintTerm<'db> {
    pub name: InternedText<'db>,
    pub payload: Box<TypeHint<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintEnum<'db> {
    pub variants: Vec<TypeHintEnumVariant<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintEnumVariant<'db> {
    pub name: InternedText<'db>,
    pub payload: Option<Box<TypeHint<'db>>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
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

    List(ExprList<'db>),
    Map(ExprMap<'db>),
    Set(ExprSet<'db>),
    Tensor(ExprTensor<'db>),
    Table(ExprTable<'db>),
    None,
    Some(ExprSome<'db>),
    Ok(ExprOk<'db>),
    Er(ExprEr<'db>),
    Data(ExprData<'db>),
    Error(ExprError<'db>),
    ParseError(ExprParseError<'db>),
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprInt<'db> {
    pub value: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprFloat<'db> {
    pub value: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprHex<'db> {
    pub value: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprString<'db> {
    pub value: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprAnonTuple<'db> {
    pub elements: Vec<ExprFull<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprAnonStruct<'db> {
    pub fields: Vec<ExprStructField<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprStructField<'db> {
    pub name: InternedText<'db>,
    pub value: ExprFull<'db>,
}


#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprList<'db> {
    pub elements: Vec<ExprFull<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprMap<'db> {
    pub entries: Vec<ExprMapEntry<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprMapEntry<'db> {
    pub key: ExprFull<'db>,
    pub value: ExprFull<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprSet<'db> {
    pub elements: Vec<ExprFull<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprTensor<'db> {
    pub shape: Vec<u32>,
    pub elements: Vec<ExprFull<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprTable<'db> {
    pub header: Vec<InternedText<'db>>,
    pub rows: Vec<ExprTableRow<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprTableRow<'db> {
    pub elements: Vec<ExprFull<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprSome<'db> {
    pub payload: ExprFull<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprOk<'db> {
    pub payload: ExprFull<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprEr<'db> {
    pub payload: ExprFull<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprData<'db> {
    pub value: ExprFull<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprError<'db> {
    pub value: ExprFull<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintParseError<'db> {
    pub text: Text<'db>,
    pub span: ByteSpan,
    pub message: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprParseError<'db> {
    pub text: Text<'db>,
    pub span: ByteSpan,
    pub message: InternedText<'db>,
}
