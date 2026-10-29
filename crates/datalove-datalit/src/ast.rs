use rmx::prelude::*;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use bct::text::InternedText;
use bct::text::ByteSpan;
use crate::type_syntax::TypeSyntax;

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
    #[returns(clone)]
    pub expr: ExprFull<'db>,
    #[returns(clone)]
    pub expr_spans: Vec<ParseSpanEntry>,
}

/// An expression, with its type hint and its position in the parse.
///
/// A plain tree shared by reference count rather than a salsa struct: a data
/// file is hundreds of thousands of these, and giving each a salsa identity
/// cost more than the rest of parsing. Each node keeps a hash of its contents,
/// worked out from its children's when it is made, so that a tree passed to a
/// query is keyed in constant time; equality is by contents.
#[derive(Clone)]
#[derive(salsa::SalsaValue)]
pub struct ExprFull<'db>(Arc<ExprNode<'db>>);

#[derive(salsa::SalsaValue)]
struct ExprNode<'db> {
    hash: u64,
    local_index: Option<u32>,
    type_hint: Option<TypeHint<'db>>,
    expr: Expr<'db>,
}

impl<'db> ExprFull<'db> {
    pub fn new(
        local_index: Option<u32>,
        type_hint: Option<TypeHint<'db>>,
        expr: Expr<'db>,
    ) -> ExprFull<'db> {
        let mut hasher = rustc_hash::FxHasher::default();
        (local_index, &type_hint, &expr).hash(&mut hasher);
        let hash = hasher.finish();
        ExprFull(Arc::new(ExprNode { hash, local_index, type_hint, expr }))
    }

    /// Position of this expression in the parse that produced it.
    ///
    /// `None` for an expression that came from no source: the typechecker
    /// builds a few to re-check a literal without its type hint, and the AST
    /// generator makes them up wholesale. Neither has a span to look up.
    pub fn local_index(&self) -> Option<u32> {
        self.0.local_index
    }

    pub fn type_hint(&self) -> Option<TypeHint<'db>> {
        self.0.type_hint.clone()
    }

    pub fn expr(&self) -> &Expr<'db> {
        &self.0.expr
    }
}

impl<'db> Hash for ExprFull<'db> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.0.hash);
    }
}

impl<'db> PartialEq for ExprFull<'db> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
            || (self.0.hash == other.0.hash
                && self.0.local_index == other.0.local_index
                && self.0.type_hint == other.0.type_hint
                && self.0.expr == other.0.expr)
    }
}

impl<'db> Eq for ExprFull<'db> {}

impl<'db> fmt::Debug for ExprFull<'db> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExprFull")
            .field("local_index", &self.0.local_index)
            .field("hash", &self.0.hash)
            .finish_non_exhaustive()
    }
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
    /// `rec Name T`: `T`, in which `Name` is `T` itself.
    Rec(TypeHintRec<'db>),
    ParseError(TypeHintParseError<'db>),
}


impl<'db> TypeHint<'db> {
    /// This type as written, shaped as datalove writes it; see
    /// [`crate::type_syntax`]. An alias stays its name, and a type that did
    /// not parse says so where it stood.
    pub fn syntax(&self, db: &'db dyn salsa::Database) -> TypeSyntax<'db> {
        use std::borrow::Cow::Borrowed;
        let boxed = |hint: &TypeHint<'db>| Box::new(hint.syntax(db));
        let fields = |fields: &[TypeHintNamedField<'db>]| fields.iter()
            .map(|field| (Borrowed(field.name.as_str(db)), field.type_hint.syntax(db)))
            .collect();
        match self {
            TypeHint::Bool => TypeSyntax::word("bool"),
            TypeHint::U8 => TypeSyntax::word("u8"),
            TypeHint::I8 => TypeSyntax::word("i8"),
            TypeHint::U16 => TypeSyntax::word("u16"),
            TypeHint::I16 => TypeSyntax::word("i16"),
            TypeHint::U32 => TypeSyntax::word("u32"),
            TypeHint::I32 => TypeSyntax::word("i32"),
            TypeHint::U64 => TypeSyntax::word("u64"),
            TypeHint::I64 => TypeSyntax::word("i64"),
            TypeHint::Index => TypeSyntax::word("index"),
            TypeHint::Offset => TypeSyntax::word("offset"),
            TypeHint::F32 => TypeSyntax::word("f32"),
            TypeHint::F64 => TypeSyntax::word("f64"),
            TypeHint::Int => TypeSyntax::word("int"),
            TypeHint::String => TypeSyntax::word("string"),
            TypeHint::Data => TypeSyntax::word("data"),
            TypeHint::Error => TypeSyntax::word("error"),
            TypeHint::Alias(alias) => TypeSyntax::word(alias.name.as_str(db)),
            TypeHint::ParseError(e) => TypeSyntax::word(fmt!("<parse-error: {}>", e.message.as_str(db))),
            TypeHint::AnonTuple(t) => TypeSyntax::Tuple(t.fields.iter().map(|f| f.syntax(db)).collect()),
            TypeHint::AnonStruct(s) => TypeSyntax::Struct(fields(&s.fields)),
            TypeHint::Table(t) => TypeSyntax::Table(fields(&t.columns)),
            TypeHint::List(l) => TypeSyntax::List(boxed(&l.element_type)),
            TypeHint::Set(s) => TypeSyntax::Set(boxed(&s.element_type)),
            TypeHint::Map(m) => TypeSyntax::Map(boxed(&m.key_type), boxed(&m.value_type)),
            TypeHint::Option(o) => TypeSyntax::Option(boxed(&o.inner_type)),
            TypeHint::Result(r) => TypeSyntax::Result(boxed(&r.inner_type)),
            TypeHint::Tensor(t) => TypeSyntax::Tensor(boxed(&t.element_type), t.rank),
            TypeHint::Atom(a) => TypeSyntax::Atom(Borrowed(a.name.as_str(db))),
            TypeHint::Term(t) => TypeSyntax::Term(Borrowed(t.name.as_str(db)), boxed(&t.payload)),
            TypeHint::Enum(e) => TypeSyntax::Enum(e.variants.iter()
                .map(|v| (Borrowed(v.name.as_str(db)), v.payload.as_ref().map(|p| p.syntax(db))))
                .collect()),
            TypeHint::Rec(r) => TypeSyntax::Rec(Borrowed(r.name.as_str(db)), boxed(&r.body)),
        }
    }
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

/// A recursive type: a name for the type, bound within its body.
///
/// The name's uses in the body are parsed as aliases, which is what a bare
/// name in type position is; converting the hint is what tells them apart.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintRec<'db> {
    pub name: InternedText<'db>,
    pub body: Box<TypeHint<'db>>,
    /// Position of the binder among the names in type position the parse
    /// filed, as for [`TypeHintAlias::local_index`].
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
    Atom(ExprAtom<'db>),
    Term(ExprTerm<'db>),
    Enum(ExprEnum<'db>),
    Group(ExprGroup<'db>),
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
    /// Whether the shape was written in a header, `[| 2 3 | ... |]`, rather
    /// than read off the body's separators.
    pub header: bool,
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
    /// Shared, so that a row parsed in a chunk goes into the table for a
    /// reference count rather than a copy; see `bct::chunk`.
    pub elements: Arc<[ExprFull<'db>]>,
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
pub struct ExprAtom<'db> {
    pub name: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprTerm<'db> {
    pub name: InternedText<'db>,
    pub payload: ExprFull<'db>,
}

/// A parenthesized expression that keeps a hint of its own.
///
/// Parentheses group without meaning anything, so `(e)` is read as `e`,
/// except where `e` has a hint and the parentheses are under another:
/// `: u32 / (: u8 / 1)` has two hints, and an expression holds one.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprGroup<'db> {
    pub inner: ExprFull<'db>,
}

/// An enum literal, `enum { atom A }`, naming the one variant it holds.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprEnum<'db> {
    pub variant: ExprFull<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeHintParseError<'db> {
    pub source: bct::input::Source,
    pub span: ByteSpan,
    pub message: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ExprParseError<'db> {
    pub source: bct::input::Source,
    pub span: ByteSpan,
    pub message: InternedText<'db>,
}
