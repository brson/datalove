//! Diagnostic emission infrastructure.
//!
//! Provides the SpanLookup trait and shared emit_pending_diagnostics function
//! that can be used with either local spans or module graph spans.

use bct::diagnostic::DiagnosticBuilder;
use bct::text::TextSpan;
use rmx::prelude::*;
use datalove_diagnostic::DiagnosticBuilderExt;

use datalove_datafun_ast::ast::ExprKey;

use crate::{DatafunSpans, ModuleId, ParsedModuleGraph, PendingDiagnostic, RecoveryHint};

/// Trait for looking up spans during diagnostic emission.
///
/// Abstracts over local spans (DatafunSpans) and module graph spans
/// (ParsedModuleGraph + ModuleId) so diagnostic emission can be shared.
pub trait SpanLookup<'db> {
    /// Look up span for an expression by its stable key.
    fn lookup_expr(&self, db: &'db dyn crate::Db, expr_key: ExprKey<'db>) -> Option<TextSpan>;

    /// Look up span for a break statement by local_index.
    fn lookup_break(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan>;

    /// Look up span for a continue statement by local_index.
    fn lookup_continue(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan>;

    /// Look up span for a return statement by local_index.
    fn lookup_ret(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan>;

    /// Look up span for a set statement by local_index.
    fn lookup_set(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan>;

    /// Look up span for a function definition by local_index.
    fn lookup_fun(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan>;

    /// Look up span for an import statement by local_index.
    fn lookup_import(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan>;

    /// Look up span for a require statement by local_index.
    fn lookup_require(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan>;

    /// Look up the span of a bare name in type position by local_index.
    fn lookup_alias(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan>;

    /// Look up the span of a type alias definition by local_index.
    fn lookup_type_alias(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan>;

    /// Look up span for a function definition in a specific module.
    ///
    /// For local spans, module_id is ignored. For module graph spans,
    /// this looks up the function in the specified module (for cross-module
    /// secondary labels in arity mismatch errors).
    fn lookup_fun_in_module(
        &self,
        db: &'db dyn crate::Db,
        module_id: Option<ModuleId<'db>>,
        local_index: u32,
    ) -> Option<TextSpan>;
}

/// SpanLookup implementation for local spans (DatafunSpans).
pub struct LocalSpanLookup<'a, 'db> {
    spans: &'a DatafunSpans<'db>,
}

impl<'a, 'db> LocalSpanLookup<'a, 'db> {
    pub fn new(spans: &'a DatafunSpans<'db>) -> Self {
        Self { spans }
    }
}

impl<'db> SpanLookup<'db> for LocalSpanLookup<'_, 'db> {
    fn lookup_expr(&self, _db: &'db dyn crate::Db, expr_key: ExprKey<'db>) -> Option<TextSpan> {
        self.spans.lookup_key(expr_key)
    }

    fn lookup_break(&self, _db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        self.spans.lookup_break(local_index).cloned()
    }

    fn lookup_continue(&self, _db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        self.spans.lookup_continue(local_index).cloned()
    }

    fn lookup_ret(&self, _db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        self.spans.lookup_ret(local_index).cloned()
    }

    fn lookup_set(&self, _db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        self.spans.lookup_set(local_index).cloned()
    }

    fn lookup_import(&self, _db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        self.spans.lookup_import(local_index).cloned()
    }

    fn lookup_require(&self, _db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        self.spans.lookup_require(local_index).cloned()
    }

    fn lookup_fun(&self, _db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        self.spans.lookup_fun(local_index).cloned()
    }

    fn lookup_alias(&self, _db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        self.spans.lookup_alias(local_index).cloned()
    }

    fn lookup_type_alias(&self, _db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        self.spans.lookup_type_alias(local_index).cloned()
    }

    fn lookup_fun_in_module(
        &self,
        db: &'db dyn crate::Db,
        _module_id: Option<ModuleId<'db>>,
        local_index: u32,
    ) -> Option<TextSpan> {
        // For local spans, ignore module_id and use local spans.
        self.lookup_fun(db, local_index)
    }
}

/// SpanLookup implementation for module graph spans.
pub struct ModuleGraphSpanLookup<'a, 'db> {
    parsed_graph: &'a ParsedModuleGraph<'db>,
    module_id: ModuleId<'db>,
}

impl<'a, 'db> ModuleGraphSpanLookup<'a, 'db> {
    pub fn new(parsed_graph: &'a ParsedModuleGraph<'db>, module_id: ModuleId<'db>) -> Self {
        Self { parsed_graph, module_id }
    }

    /// The spans of one module, built on first ask and memoized after.
    ///
    /// A module nobody reports a diagnostic against never builds its table.
    fn spans(
        &self,
        db: &'db dyn crate::Db,
        module_id: ModuleId<'db>,
    ) -> Option<&'db DatafunSpans<'db>> {
        let module = self.parsed_graph.get_module(db, module_id)?;
        Some(datalove_datafun_parser::module_spans(db, module))
    }
}

impl<'db> SpanLookup<'db> for ModuleGraphSpanLookup<'_, 'db> {
    fn lookup_expr(&self, db: &'db dyn crate::Db, expr_key: ExprKey<'db>) -> Option<TextSpan> {
        let spans = self.spans(db, self.module_id)?;
        spans.lookup_key(expr_key)
    }

    fn lookup_break(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        let spans = self.spans(db, self.module_id)?;
        spans.lookup_break(local_index).cloned()
    }

    fn lookup_continue(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        let spans = self.spans(db, self.module_id)?;
        spans.lookup_continue(local_index).cloned()
    }

    fn lookup_ret(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        let spans = self.spans(db, self.module_id)?;
        spans.lookup_ret(local_index).cloned()
    }

    fn lookup_set(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        let spans = self.spans(db, self.module_id)?;
        spans.lookup_set(local_index).cloned()
    }

    fn lookup_import(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        let spans = self.spans(db, self.module_id)?;
        spans.lookup_import(local_index).cloned()
    }

    fn lookup_require(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        let spans = self.spans(db, self.module_id)?;
        spans.lookup_require(local_index).cloned()
    }

    fn lookup_fun(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        let spans = self.spans(db, self.module_id)?;
        spans.lookup_fun(local_index).cloned()
    }

    fn lookup_alias(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        let spans = self.spans(db, self.module_id)?;
        spans.lookup_alias(local_index).cloned()
    }

    fn lookup_type_alias(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan> {
        let spans = self.spans(db, self.module_id)?;
        spans.lookup_type_alias(local_index).cloned()
    }

    fn lookup_fun_in_module(
        &self,
        db: &'db dyn crate::Db,
        module_id: Option<ModuleId<'db>>,
        local_index: u32,
    ) -> Option<TextSpan> {
        // For module graph, look up in the specified module if provided.
        let target_module = module_id.unwrap_or(self.module_id);
        let spans = self.spans(db, target_module)?;
        spans.lookup_fun(local_index).cloned()
    }
}

/// Emit all pending diagnostics using the provided span lookup.
pub fn emit_pending_diagnostics<'db>(
    db: &'db dyn crate::Db,
    pending: &[PendingDiagnostic<'db>],
    spans: &dyn SpanLookup<'db>,
) {
    for diag in pending {
        if let Some(builder) = build(db, diag, spans) {
            builder.emit_type();
        }
    }
}

/// Format pending diagnostics as human-readable strings with location info.
///
/// The same diagnostics [`emit_pending_diagnostics`] files, each as one line
/// -- `line 3, col 7: error[F016]: ...` -- with its help after it, for test
/// harnesses and error collection.
pub fn format_pending_diagnostics<'db>(
    db: &'db dyn crate::Db,
    pending: &[PendingDiagnostic<'db>],
    spans: &dyn SpanLookup<'db>,
) -> Vec<String> {
    pending.iter()
        .filter_map(|diag| build(db, diag, spans))
        .map(|builder| {
            let diagnostic = builder.build();
            let label = diagnostic.labels.iter()
                .find(|label| label.style == bct::diagnostic::LabelStyle::Primary)
                .expect("every typecheck diagnostic has a primary label");
            let loc = format_location(db, &TextSpan::new(label.source, label.span.C()));
            let code = diagnostic.code.expect("every typecheck diagnostic has a code");
            let mut line = format!("{}: error[{}]: {}", loc, code.as_str(db), diagnostic.message.as_str(db));
            for help in &diagnostic.helps {
                line.push_str(&format!(" (help: {})", help.as_str(db)));
            }
            line
        })
        .collect()
}

/// A name's diagnostic, with why it is not in scope when it is declared below.
fn note_below<'db>(builder: DiagnosticBuilder<'db>, below: bool, name: &str) -> DiagnosticBuilder<'db> {
    match below {
        true => builder.note(&crate::context::declared_below_note(name)),
        false => builder,
    }
}

/// The diagnostic for one pending diagnostic, the one place each is worded;
/// `None` where what it points at has no span.
fn build<'db>(
    db: &'db dyn crate::Db,
    diag: &PendingDiagnostic<'db>,
    spans: &dyn SpanLookup<'db>,
) -> Option<DiagnosticBuilder<'db>> {
    let builder = match diag {
        PendingDiagnostic::UndefinedVariable { expr_key, module_id: _, name, below } => {
            let builder = DiagnosticBuilder::error(db, &format!("cannot find value `{}` in this scope", name.as_str(db)))
                .code("F001")
                .primary_label(spans.lookup_expr(db, *expr_key)?, "not found in this scope");
            note_below(builder, *below, name.as_str(db))
        }
        PendingDiagnostic::UndefinedFunction { expr_key, module_id: _, name, below } => {
            let builder = DiagnosticBuilder::error(db, &format!("cannot find function `{}` in this scope", name.as_str(db)))
                .code("F002")
                .primary_label(spans.lookup_expr(db, *expr_key)?, "not found in this scope");
            note_below(builder, *below, name.as_str(db))
        }
        PendingDiagnostic::DuplicateImport { local_index, module_id: _, name, first } => {
            DiagnosticBuilder::error(db, &format!("`{}` is imported already, from `{}`", name.as_str(db), first.as_str(db)))
                .code("F059")
                .primary_label(spans.lookup_import(db, *local_index)?, "a name binds one function")
        }
        PendingDiagnostic::ImportShadowsFunction { local_index, module_id: _, name } => {
            DiagnosticBuilder::error(db, &format!("`{}` is imported, and also defined here as a function", name.as_str(db)))
                .code("F086")
                .primary_label(spans.lookup_import(db, *local_index)?, "a name binds one function")
                .note("the function defined here is the one that is used")
        }
        PendingDiagnostic::CollectionKeyNotOrdered { local_index, module_id: _, param, position } => {
            let msg = format!(
                "`{}` has not said it can be ordered, and {} puts it in a set or a map",
                param.as_str(db), position.as_str(db),
            );
            DiagnosticBuilder::error(db, &msg)
                .code("F061")
                .primary_label(spans.lookup_fun(db, *local_index)?, "this signature does not resolve, so calls to it are not found either")
                .note("a set keeps its elements in order and a map keeps its keys in order, so a \
collection written over a type parameter asks that parameter for an ordering. Say so with `where { \
<param> is ord, }`, which every type there is satisfies.")
        }
        PendingDiagnostic::MissingReturn { local_index, module_id: _, name } => {
            DiagnosticBuilder::error(db, &format!("`{}` can reach the end of its body without returning a value", name.as_str(db)))
                .code("F063")
                .primary_label(spans.lookup_fun(db, *local_index)?, "this signature promises a value")
                .note("the end of a body is a return like any other, and one that owes a \
value has to be reached only where a `ret` has already left. Return from every path out, or \
drop the return type.")
        }
        PendingDiagnostic::UnresolvedTypeAlias { local_index, module_id: _, name, below: true } => {
            let builder = DiagnosticBuilder::error(db, &format!("unknown type `{}`", name.as_str(db)))
                .code("F064")
                .primary_label(spans.lookup_alias(db, *local_index)?, "no type of this name is in scope");
            note_below(builder, true, name.as_str(db))
        }
        PendingDiagnostic::UnresolvedTypeAlias { local_index, module_id: _, name, below: false } => {
            let mut builder = DiagnosticBuilder::error(db, &format!("unknown type `{}`", name.as_str(db)))
                .code("F064")
                .primary_label(spans.lookup_alias(db, *local_index)?, "no type of this name is in scope")
                .note("a bare name in type position is a type alias, written `type Name: \
<type>`, or one of the type parameters of the generic function it appears in.");
            if let Some(suggestion) = datalove_datalit::parser_util::type_name_suggestion(name.as_str(db)) {
                builder = builder.note(&suggestion);
            }
            builder
        }
        PendingDiagnostic::Coded { site, module_id: _, code, message, label, note } => {
            let ts = match site {
                crate::ErrorSite::Expr(key) => spans.lookup_expr(db, *key),
                crate::ErrorSite::Set(local_index) => spans.lookup_set(db, *local_index),
                crate::ErrorSite::Fun(local_index) => spans.lookup_fun(db, *local_index),
                crate::ErrorSite::Import(local_index) => spans.lookup_import(db, *local_index),
                crate::ErrorSite::Require(local_index) => spans.lookup_require(db, *local_index),
            }?;
            let mut builder = DiagnosticBuilder::error(db, message.as_str(db))
                .code(code.as_str(db))
                .primary_label(ts, label.as_str(db));
            if let Some(note) = note {
                builder = builder.note(note.as_str(db));
            }
            builder
        }
        PendingDiagnostic::DuplicateTypeAlias { local_index, module_id: _, name } => {
            DiagnosticBuilder::error(db, &format!("`{}` is defined twice", name.as_str(db)))
                .code("F066")
                .primary_label(spans.lookup_type_alias(db, *local_index)?, "a type alias is defined once")
        }
        PendingDiagnostic::DuplicateFunction { local_index, module_id: _, name } => {
            DiagnosticBuilder::error(db, &format!("function `{}` is defined twice", name.as_str(db)))
                .code("F086")
                .primary_label(spans.lookup_fun(db, *local_index)?, "a function is defined once")
                .note("the first definition is the one that is used")
        }
        PendingDiagnostic::NativeFunOutsideRider { local_index, module_id: _, name } => {
            let msg = format!(
                "`{}` is a native function declared where there is no rider to implement it",
                name.as_str(db),
            );
            DiagnosticBuilder::error(db, &msg)
                .code("F062")
                .primary_label(spans.lookup_fun(db, *local_index)?, "nothing supplies a body for this")
                .note("a `native fun` names a Rust function, and the only place one is supplied \
is a package's rider interface. Declare it in the package's `rider/rider.dli` and reach it from a module \
with `require rider <name>` and `import <name>.<function>`; a script has no rider of its own.")
        }
        PendingDiagnostic::CannotSynthesize { expr_key, module_id: _, message } => {
            DiagnosticBuilder::error(db, message.as_str(db))
                .code("F011")
                .primary_label(spans.lookup_expr(db, *expr_key)?, "cannot infer type")
        }
        PendingDiagnostic::TypeMismatch { expr_key, module_id: _, expected, actual, label, recovery_hint } => {
            let msg = format!("mismatched types: expected `{}`, found `{}`", expected.as_str(db), actual.as_str(db));
            let mut builder = DiagnosticBuilder::error(db, &msg)
                .code("F016")
                .primary_label(spans.lookup_expr(db, *expr_key)?, label.as_str(db));
            if let RecoveryHint::InsertAdapt { description } = recovery_hint {
                builder = builder.help(&format!("use `@` to {}", description));
            }
            builder
        }
        PendingDiagnostic::PatternMismatch { expr_key, module_id: _, message, label } => {
            DiagnosticBuilder::error(db, message.as_str(db))
                .code("F071")
                .primary_label(spans.lookup_expr(db, *expr_key)?, label.as_str(db))
        }
        PendingDiagnostic::LiteralOutOfRange { expr_key, module_id: _, message, note } => {
            DiagnosticBuilder::error(db, message.as_str(db))
                .code("F065")
                .primary_label(spans.lookup_expr(db, *expr_key)?, "value out of range")
                .note(note.as_str(db))
        }
        PendingDiagnostic::InvalidOperandType { expr_key, module_id: _, op, ty, note } => {
            let msg = format!("invalid operand type `{}` for operator `{}`", ty.as_str(db), op.as_str(db));
            let label = format!("operator `{}` cannot be applied to type `{}`", op.as_str(db), ty.as_str(db));
            let mut builder = DiagnosticBuilder::error(db, &msg)
                .code("F026")
                .primary_label(spans.lookup_expr(db, *expr_key)?, &label);
            if let Some(note) = note {
                builder = builder.note(note.as_str(db));
            }
            builder
        }
        PendingDiagnostic::ArityMismatch {
            call_expr_key,
            call_module_id: _,
            func_name,
            func_local_index,
            func_module_id,
            expected,
            actual,
        } => {
            let plural = |n: usize| if n == 1 { "" } else { "s" };
            let msg = format!(
                "this function takes {} argument{} but {} {} supplied",
                expected, plural(*expected), actual, if *actual == 1 { "was" } else { "were" },
            );
            let mut builder = DiagnosticBuilder::error(db, &msg)
                .code("F045")
                .primary_label(spans.lookup_expr(db, *call_expr_key)?, &format!("expected {} argument{}", expected, plural(*expected)));
            if let Some(def_ts) = spans.lookup_fun_in_module(db, *func_module_id, *func_local_index) {
                let def_label = format!(
                    "function `{}` defined here with {} parameter{}",
                    func_name.as_str(db), expected, plural(*expected),
                );
                builder = builder.secondary_label(def_ts, &def_label);
            }
            builder
        }
        PendingDiagnostic::ResultRequiresBinding { expr_key, module_id: _ } => {
            DiagnosticBuilder::error(db, "a result taken apart needs an `else` binding for its error")
                .code("F046")
                .primary_label(spans.lookup_expr(db, *expr_key)?, "this is a result, `!T`")
                .note("use `if result |value| ... else |err| ... end if` to handle both cases")
        }
        PendingDiagnostic::TryTypeMismatch { expr_key, module_id: _, operator, expected, actual } => {
            let msg = format!("`{}` unwraps {}, and this is `{}`", operator.as_str(db), expected.as_str(db), actual.as_str(db));
            DiagnosticBuilder::error(db, &msg)
                .code("F048")
                .primary_label(spans.lookup_expr(db, *expr_key)?, &format!("this is `{}`", actual.as_str(db)))
        }
        PendingDiagnostic::TryReturnTypeMismatch { expr_key, module_id: _, operator, expected, actual } => {
            let ts = spans.lookup_expr(db, *expr_key)?;
            match actual {
                Some(actual) => {
                    let msg = format!(
                        "`{}` can return early, so the function has to return {}, and it returns `{}`",
                        operator.as_str(db), expected.as_str(db), actual.as_str(db),
                    );
                    DiagnosticBuilder::error(db, &msg)
                        .code("F049")
                        .primary_label(ts, "this can return early")
                }
                None => {
                    let msg = format!("`{}` can return early, and there is no function here to return from", operator.as_str(db));
                    DiagnosticBuilder::error(db, &msg)
                        .code("F049")
                        .primary_label(ts, "this can return early")
                }
            }
        }
        PendingDiagnostic::BreakOutsideLoop { local_index, module_id: _ } => {
            DiagnosticBuilder::error(db, "`break` outside of a loop")
                .code("F050")
                .primary_label(spans.lookup_break(db, *local_index)?, "break statement here")
                .note("break can only be used inside loop blocks")
        }
        PendingDiagnostic::ContinueOutsideLoop { local_index, module_id: _ } => {
            DiagnosticBuilder::error(db, "`continue` outside of a loop")
                .code("F051")
                .primary_label(spans.lookup_continue(db, *local_index)?, "continue statement here")
                .note("continue can only be used inside loop blocks")
        }
        PendingDiagnostic::VoidFunctionReturnsValue { local_index, module_id: _ } => {
            DiagnosticBuilder::error(db, "void function cannot return a value")
                .code("F052")
                .primary_label(spans.lookup_ret(db, *local_index)?, "return with value in void function")
                .note("remove the return value or add a return type to the function")
        }
        PendingDiagnostic::FunctionRequiresReturnValue { local_index, module_id: _ } => {
            DiagnosticBuilder::error(db, "function requires return value")
                .code("F053")
                .primary_label(spans.lookup_ret(db, *local_index)?, "bare return in non-void function")
                .note("add a return value or change the function to void")
        }
        PendingDiagnostic::UndefinedVariableSet { local_index, module_id: _, name } => {
            DiagnosticBuilder::error(db, &format!("cannot find variable `{}` in this scope", name.as_str(db)))
                .code("F054")
                .primary_label(spans.lookup_set(db, *local_index)?, "variable not defined")
                .note("declare the variable with 'var' before assigning to it")
        }
        PendingDiagnostic::VariableNotMutable { local_index, module_id: _, name } => {
            DiagnosticBuilder::error(db, &format!("cannot assign to immutable variable `{}`", name.as_str(db)))
                .code("F055")
                .primary_label(spans.lookup_set(db, *local_index)?, "cannot assign to immutable variable")
                .note("consider declaring the variable with 'var' instead of 'let'")
        }
        PendingDiagnostic::NonConstInConstExpr { expr_key, module_id: _, name } => {
            DiagnosticBuilder::error(db, &format!("`{}` is not a const binding", name.as_str(db)))
                .code("F058")
                .primary_label(spans.lookup_expr(db, *expr_key)?, "not a const binding")
                .note("a const expression is evaluated at compile time, so it can only name other consts")
        }
        PendingDiagnostic::ArgumentModeMismatch { expr_key, module_id: _, expected, found } => {
            DiagnosticBuilder::error(db, &format!("argument mode mismatch: expected `{}`, found `{}`", expected.as_str(db), found.as_str(db)))
                .code("F057")
                .primary_label(spans.lookup_expr(db, *expr_key)?, &format!("this argument must be passed as `{}`", expected.as_str(db)))
                .note("the marker at a call site must match the parameter's declared mode")
        }
    };
    Some(builder)
}

/// Format a TextSpan as a location string (e.g., "line 10, col 5").
fn format_location<'db>(db: &'db dyn crate::Db, ts: &bct::text::TextSpan) -> String {
    let span = &ts.span;
    let text = ts.source.text(db);

    // Calculate line and column from byte offset.
    let start = span.start as usize;
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in text.char_indices() {
        if i >= start {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }

    format!("line {}, col {}", line, col)
}
