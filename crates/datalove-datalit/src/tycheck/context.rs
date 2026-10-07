//! Type checking context.
//!
//! Provides TypeContext for tracking errors during typechecking.

use bct::text::{InternedText, TextSpan};
use bct::diagnostic::DiagnosticBuilder;
use datalove_diagnostic::DiagnosticBuilderExt;
use crate::ast::{ExprFull, TypeHint};
use crate::resolve::ResolvedExpr;
use super::types::{Type, TypeError, convert_type_hint};

/// Context for typechecking.
pub struct TypeContext<'db> {
    pub(crate) db: &'db dyn crate::Db,
    pub(crate) source: bct::input::Source,
    pub(crate) errors: Vec<TypeError>,
}

impl<'db> TypeContext<'db> {
    pub fn new(db: &'db dyn crate::Db, resolved: ResolvedExpr<'db>) -> Self {
        TypeContext {
            db,
            source: resolved.source(db),
            errors: Vec::new(),
        }
    }

    pub fn add_error(&mut self, error: TypeError) {
        self.errors.push(error);
    }

    /// The type an expression's hint names, or a report of why it names none.
    ///
    /// A bare name is an alias in datafun and nothing in datalit, which has
    /// no way to declare one. A hint that did not parse was reported by the
    /// parser.
    pub fn convert_hint(&self, expr: &ExprFull<'db>, hint: &TypeHint<'db>) -> Result<Type<'db>, TypeError> {
        convert_type_hint(self.db, hint).map_err(|error| {
            if let (Some(name), Some(ts)) = (first_alias(hint), self.get_span(expr)) {
                let name = name.as_str(self.db);
                let mut builder = DiagnosticBuilder::error(self.db, &rmx::std::format!("unknown type `{name}`"))
                    .code("T059")
                    .primary_label(ts, "not a type")
                    .note("datalit has no type aliases, so a type is written out rather than named");
                if let Some(suggestion) = crate::parser_util::type_name_suggestion(name) {
                    builder = builder.note(&suggestion);
                }
                builder.emit_type();
            }
            error
        })
    }

    /// Look up the source location for an expression (on-demand).
    pub fn get_span(&self, expr: &ExprFull<'db>) -> Option<TextSpan<'db>> {
        let spans = crate::spans::datalit_spans(self.db, self.source);
        spans.get_text_span(self.db, expr)
    }
}

/// The first bare name in a type hint, if it has one.
fn first_alias<'db>(hint: &TypeHint<'db>) -> Option<InternedText<'db>> {
    match hint {
        TypeHint::Alias(alias) => Some(alias.name),
        TypeHint::AnonTuple(t) => t.fields.iter().find_map(first_alias),
        TypeHint::AnonStruct(s) => s.fields.iter().find_map(|f| first_alias(&f.type_hint)),
        TypeHint::Table(t) => t.columns.iter().find_map(|c| first_alias(&c.type_hint)),
        TypeHint::List(l) => first_alias(&l.element_type),
        TypeHint::Set(s) => first_alias(&s.element_type),
        TypeHint::Map(m) => first_alias(&m.key_type).or_else(|| first_alias(&m.value_type)),
        TypeHint::Option(o) => first_alias(&o.inner_type),
        TypeHint::Result(r) => first_alias(&r.inner_type),
        TypeHint::Tensor(t) => first_alias(&t.element_type),
        TypeHint::Term(t) => first_alias(&t.payload),
        TypeHint::Enum(e) => e.variants.iter().find_map(|v| v.payload.as_deref().and_then(first_alias)),
        _ => None,
    }
}
