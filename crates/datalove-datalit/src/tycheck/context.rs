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
            if let Some(ts) = self.get_span(expr) {
                if let Some(builder) = rec_diagnostic(self.db, &error, ts.clone()) {
                    builder.emit_type();
                    return error;
                }
            }
            if let (Some(name), Some(ts)) = (first_alias(hint, &mut Vec::new()), self.get_span(expr)) {
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
    pub fn get_span(&self, expr: &ExprFull<'db>) -> Option<TextSpan> {
        let spans = crate::spans::datalit_spans(self.db, self.source);
        spans.get_text_span(self.db, expr)
    }
}

/// The diagnostic for a malformed recursive type, if the error is one.
///
/// The span is the expression's: datalit keeps no spans for the names in a
/// hint.
pub fn rec_diagnostic<'db>(db: &'db dyn crate::Db, error: &TypeError, ts: TextSpan) -> Option<DiagnosticBuilder<'db>> {
    let TypeError::Rec(rec) = error else { return None };
    let (code, label, note) = rec.kind.diagnostic();
    Some(DiagnosticBuilder::error(db, &rmx::std::format!("{error}"))
        .code(code)
        .primary_label(ts, label)
        .note(note))
}

/// The first bare name in a type hint that no `rec` around it binds, if it
/// has one.
fn first_alias<'db>(hint: &TypeHint<'db>, bound: &mut Vec<InternedText<'db>>) -> Option<InternedText<'db>> {
    match hint {
        TypeHint::Alias(alias) if bound.contains(&alias.name) => None,
        TypeHint::Alias(alias) => Some(alias.name),
        TypeHint::Rec(r) => {
            bound.push(r.name);
            let found = first_alias(&r.body, bound);
            bound.pop();
            found
        }
        TypeHint::AnonTuple(t) => t.fields.iter().find_map(|f| first_alias(f, bound)),
        TypeHint::AnonStruct(s) => s.fields.iter().find_map(|f| first_alias(&f.type_hint, bound)),
        TypeHint::Table(t) => t.columns.iter().find_map(|c| first_alias(&c.type_hint, bound)),
        TypeHint::List(l) => first_alias(&l.element_type, bound),
        TypeHint::Set(s) => first_alias(&s.element_type, bound),
        TypeHint::Map(m) => first_alias(&m.key_type, bound).or_else(|| first_alias(&m.value_type, bound)),
        TypeHint::Option(o) => first_alias(&o.inner_type, bound),
        TypeHint::Result(r) => first_alias(&r.inner_type, bound),
        TypeHint::Tensor(t) => first_alias(&t.element_type, bound),
        TypeHint::Term(t) => first_alias(&t.payload, bound),
        TypeHint::Enum(e) => e.variants.iter().find_map(|v| v.payload.as_deref().and_then(|p| first_alias(p, bound))),
        _ => None,
    }
}
