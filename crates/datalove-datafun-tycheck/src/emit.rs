//! Diagnostic emission infrastructure.
//!
//! Provides the SpanLookup trait and shared emit_pending_diagnostics function
//! that can be used with either local spans or module graph spans.

use bct::text::{InternedText, TextSpan};
use salsa::plumbing::FromId;

use datalove_datafun_ast::ast::ExprFun;

use crate::{DatafunSpans, ModuleId, ParsedModuleGraph, PendingDiagnostic};

/// Trait for looking up spans during diagnostic emission.
///
/// Abstracts over local spans (DatafunSpans) and module graph spans
/// (ParsedModuleGraph + ModuleId) so diagnostic emission can be shared.
pub trait SpanLookup<'db> {
    /// Look up span for an expression by its salsa ID index.
    fn lookup_expr(&self, db: &'db dyn crate::Db, expr_id: u32) -> Option<TextSpan<'db>>;

    /// Look up span for a break statement by local_index.
    fn lookup_break(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan<'db>>;

    /// Look up span for a continue statement by local_index.
    fn lookup_continue(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan<'db>>;

    /// Look up span for a return statement by local_index.
    fn lookup_ret(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan<'db>>;

    /// Look up span for a set statement by local_index.
    fn lookup_set(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan<'db>>;

    /// Look up span for a function definition by local_index.
    fn lookup_fun(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan<'db>>;

    /// Look up span for a function definition in a specific module.
    ///
    /// For local spans, module_id is ignored. For module graph spans,
    /// this looks up the function in the specified module (for cross-module
    /// secondary labels in arity mismatch errors).
    fn lookup_fun_in_module(
        &self,
        db: &'db dyn crate::Db,
        module_id: Option<ModuleId>,
        local_index: u32,
    ) -> Option<TextSpan<'db>>;
}

/// SpanLookup implementation for local spans (DatafunSpans).
pub struct LocalSpanLookup<'a> {
    spans: &'a DatafunSpans,
}

impl<'a> LocalSpanLookup<'a> {
    pub fn new(spans: &'a DatafunSpans) -> Self {
        Self { spans }
    }
}

impl<'db> SpanLookup<'db> for LocalSpanLookup<'_> {
    fn lookup_expr(&self, db: &'db dyn crate::Db, expr_id: u32) -> Option<TextSpan<'db>> {
        let id = unsafe { salsa::Id::from_index(expr_id) };
        let expr = ExprFun::from_id(id);
        self.spans.lookup(expr).map(|entry| {
            let (text, span) = entry.to_text_and_span(db);
            TextSpan::new(text, span)
        })
    }

    fn lookup_break(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan<'db>> {
        self.spans.lookup_break(local_index).map(|entry| {
            let (text, span) = entry.to_text_and_span(db);
            TextSpan::new(text, span)
        })
    }

    fn lookup_continue(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan<'db>> {
        self.spans.lookup_continue(local_index).map(|entry| {
            let (text, span) = entry.to_text_and_span(db);
            TextSpan::new(text, span)
        })
    }

    fn lookup_ret(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan<'db>> {
        self.spans.lookup_ret(local_index).map(|entry| {
            let (text, span) = entry.to_text_and_span(db);
            TextSpan::new(text, span)
        })
    }

    fn lookup_set(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan<'db>> {
        self.spans.lookup_set(local_index).map(|entry| {
            let (text, span) = entry.to_text_and_span(db);
            TextSpan::new(text, span)
        })
    }

    fn lookup_fun(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan<'db>> {
        self.spans.lookup_fun(local_index).map(|entry| {
            let (text, span) = entry.to_text_and_span(db);
            TextSpan::new(text, span)
        })
    }

    fn lookup_fun_in_module(
        &self,
        db: &'db dyn crate::Db,
        _module_id: Option<ModuleId>,
        local_index: u32,
    ) -> Option<TextSpan<'db>> {
        // For local spans, ignore module_id and use local spans.
        self.lookup_fun(db, local_index)
    }
}

/// SpanLookup implementation for module graph spans.
pub struct ModuleGraphSpanLookup<'a, 'db> {
    parsed_graph: &'a ParsedModuleGraph<'db>,
    module_id: ModuleId,
}

impl<'a, 'db> ModuleGraphSpanLookup<'a, 'db> {
    pub fn new(parsed_graph: &'a ParsedModuleGraph<'db>, module_id: ModuleId) -> Self {
        Self { parsed_graph, module_id }
    }
}

impl<'db> SpanLookup<'db> for ModuleGraphSpanLookup<'_, 'db> {
    fn lookup_expr(&self, db: &'db dyn crate::Db, expr_id: u32) -> Option<TextSpan<'db>> {
        let spans = self.parsed_graph.get_spans(db, self.module_id)?;
        let id = unsafe { salsa::Id::from_index(expr_id) };
        let expr = ExprFun::from_id(id);
        spans.lookup(expr).map(|entry| {
            let (text, span) = entry.to_text_and_span(db);
            TextSpan::new(text, span)
        })
    }

    fn lookup_break(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan<'db>> {
        let spans = self.parsed_graph.get_spans(db, self.module_id)?;
        spans.lookup_break(local_index).map(|entry| {
            let (text, span) = entry.to_text_and_span(db);
            TextSpan::new(text, span)
        })
    }

    fn lookup_continue(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan<'db>> {
        let spans = self.parsed_graph.get_spans(db, self.module_id)?;
        spans.lookup_continue(local_index).map(|entry| {
            let (text, span) = entry.to_text_and_span(db);
            TextSpan::new(text, span)
        })
    }

    fn lookup_ret(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan<'db>> {
        let spans = self.parsed_graph.get_spans(db, self.module_id)?;
        spans.lookup_ret(local_index).map(|entry| {
            let (text, span) = entry.to_text_and_span(db);
            TextSpan::new(text, span)
        })
    }

    fn lookup_set(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan<'db>> {
        let spans = self.parsed_graph.get_spans(db, self.module_id)?;
        spans.lookup_set(local_index).map(|entry| {
            let (text, span) = entry.to_text_and_span(db);
            TextSpan::new(text, span)
        })
    }

    fn lookup_fun(&self, db: &'db dyn crate::Db, local_index: u32) -> Option<TextSpan<'db>> {
        let spans = self.parsed_graph.get_spans(db, self.module_id)?;
        spans.lookup_fun(local_index).map(|entry| {
            let (text, span) = entry.to_text_and_span(db);
            TextSpan::new(text, span)
        })
    }

    fn lookup_fun_in_module(
        &self,
        db: &'db dyn crate::Db,
        module_id: Option<ModuleId>,
        local_index: u32,
    ) -> Option<TextSpan<'db>> {
        // For module graph, look up in the specified module if provided.
        let target_module = module_id.unwrap_or(self.module_id);
        let spans = self.parsed_graph.get_spans(db, target_module)?;
        spans.lookup_fun(local_index).map(|entry| {
            let (text, span) = entry.to_text_and_span(db);
            TextSpan::new(text, span)
        })
    }
}

/// Emit all pending diagnostics using the provided span lookup.
pub fn emit_pending_diagnostics<'db>(
    db: &'db dyn crate::Db,
    pending: &[PendingDiagnostic<'db>],
    spans: &dyn SpanLookup<'db>,
) {
    for diag in pending {
        emit_single_diagnostic(db, diag, spans);
    }
}

/// Emit a single pending diagnostic.
fn emit_single_diagnostic<'db>(
    db: &'db dyn crate::Db,
    diag: &PendingDiagnostic<'db>,
    spans: &dyn SpanLookup<'db>,
) {
    match diag {
        PendingDiagnostic::UndefinedVariable { expr_id, module_id: _, name } => {
            if let Some(ts) = spans.lookup_expr(db, *expr_id) {
                let msg = format!("cannot find value `{}` in this scope", name.as_str(db));
                datalove_diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("F001")
                    .primary_label(ts, "not found in this scope")
                    .emit_type();
            }
        }
        PendingDiagnostic::UndefinedFunction { expr_id, module_id: _, name } => {
            if let Some(ts) = spans.lookup_expr(db, *expr_id) {
                let msg = format!("cannot find function `{}` in this scope", name.as_str(db));
                datalove_diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("F002")
                    .primary_label(ts, "not found in this scope")
                    .emit_type();
            }
        }
        PendingDiagnostic::CannotSynthesize { expr_id, module_id: _, message } => {
            if let Some(ts) = spans.lookup_expr(db, *expr_id) {
                datalove_diagnostic::DiagnosticBuilder::error(db, message.as_str(db))
                    .code("F011")
                    .primary_label(ts, "cannot infer type")
                    .emit_type();
            }
        }
        PendingDiagnostic::TypeMismatch { expr_id, module_id: _, expected, actual, label } => {
            if let Some(ts) = spans.lookup_expr(db, *expr_id) {
                let msg = format!(
                    "mismatched types: expected `{}`, found `{}`",
                    expected.as_str(db),
                    actual.as_str(db)
                );
                datalove_diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("F016")
                    .primary_label(ts, label.as_str(db))
                    .emit_type();
            }
        }
        PendingDiagnostic::InvalidOperandType { expr_id, module_id: _, op, ty } => {
            if let Some(ts) = spans.lookup_expr(db, *expr_id) {
                let msg = format!(
                    "invalid operand type `{}` for operator `{}`",
                    ty.as_str(db),
                    op.as_str(db)
                );
                datalove_diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("F026")
                    .primary_label(
                        ts,
                        &format!(
                            "operator `{}` cannot be applied to type `{}`",
                            op.as_str(db),
                            ty.as_str(db)
                        ),
                    )
                    .emit_type();
            }
        }
        PendingDiagnostic::ArityMismatch {
            call_expr_id,
            call_module_id: _,
            func_name,
            func_local_index,
            func_module_id,
            expected,
            actual,
        } => {
            emit_arity_mismatch(db, spans, *call_expr_id, *func_name, *func_local_index, *func_module_id, *expected, *actual);
        }
        PendingDiagnostic::ResultRequiresBinding { expr_id, module_id: _ } => {
            if let Some(ts) = spans.lookup_expr(db, *expr_id) {
                datalove_diagnostic::DiagnosticBuilder::error(db, "Result destructuring requires an else binding")
                    .code("F046")
                    .primary_label(ts, "Result type here")
                    .note("use `if result |value| ... else |err| ... end if` to handle both cases")
                    .emit_type();
            }
        }
        PendingDiagnostic::TryTypeMismatch { expr_id, module_id: _, operator, expected, actual } => {
            if let Some(ts) = spans.lookup_expr(db, *expr_id) {
                let msg = format!(
                    "try operator `{}` requires {} type, found `{}`",
                    operator.as_str(db),
                    expected.as_str(db),
                    actual.as_str(db)
                );
                datalove_diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("F048")
                    .primary_label(
                        ts,
                        &format!("expected {}, found `{}`", expected.as_str(db), actual.as_str(db)),
                    )
                    .emit_type();
            }
        }
        PendingDiagnostic::TryReturnTypeMismatch { expr_id, module_id: _, operator, expected, actual } => {
            if let Some(ts) = spans.lookup_expr(db, *expr_id) {
                let msg = format!(
                    "try operator `{}` requires function to return {}, found `{}`",
                    operator.as_str(db),
                    expected.as_str(db),
                    actual.as_str(db)
                );
                datalove_diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("F049")
                    .primary_label(ts, "try operator here")
                    .note(&format!(
                        "function must return {} to use `{}` operator",
                        expected.as_str(db),
                        operator.as_str(db)
                    ))
                    .emit_type();
            }
        }
        PendingDiagnostic::BreakOutsideLoop { local_index, module_id: _ } => {
            if let Some(ts) = spans.lookup_break(db, *local_index) {
                datalove_diagnostic::DiagnosticBuilder::error(db, "`break` used outside of loop")
                    .code("F050")
                    .primary_label(ts, "break statement here")
                    .note("break can only be used inside loop blocks")
                    .emit_type();
            }
        }
        PendingDiagnostic::ContinueOutsideLoop { local_index, module_id: _ } => {
            if let Some(ts) = spans.lookup_continue(db, *local_index) {
                datalove_diagnostic::DiagnosticBuilder::error(db, "`continue` used outside of loop")
                    .code("F051")
                    .primary_label(ts, "continue statement here")
                    .note("continue can only be used inside loop blocks")
                    .emit_type();
            }
        }
        PendingDiagnostic::VoidFunctionReturnsValue { local_index, module_id: _ } => {
            if let Some(ts) = spans.lookup_ret(db, *local_index) {
                datalove_diagnostic::DiagnosticBuilder::error(db, "void function cannot return a value")
                    .code("F052")
                    .primary_label(ts, "return with value in void function")
                    .note("remove the return value or add a return type to the function")
                    .emit_type();
            }
        }
        PendingDiagnostic::FunctionRequiresReturnValue { local_index, module_id: _ } => {
            if let Some(ts) = spans.lookup_ret(db, *local_index) {
                datalove_diagnostic::DiagnosticBuilder::error(db, "function requires return value")
                    .code("F053")
                    .primary_label(ts, "bare return in non-void function")
                    .note("add a return value or change the function to void")
                    .emit_type();
            }
        }
        PendingDiagnostic::UndefinedVariableSet { local_index, module_id: _, name } => {
            if let Some(ts) = spans.lookup_set(db, *local_index) {
                datalove_diagnostic::DiagnosticBuilder::error(
                    db,
                    &format!("undefined variable: {}", name.as_str(db)),
                )
                    .code("F054")
                    .primary_label(ts, "variable not defined")
                    .note("declare the variable with 'var' before assigning to it")
                    .emit_type();
            }
        }
    }
}

/// Emit F045 arity mismatch diagnostic.
#[allow(clippy::too_many_arguments)]
fn emit_arity_mismatch<'db>(
    db: &'db dyn crate::Db,
    spans: &dyn SpanLookup<'db>,
    call_expr_id: u32,
    func_name: InternedText<'db>,
    func_local_index: u32,
    func_module_id: Option<ModuleId>,
    expected: usize,
    actual: usize,
) {
    let Some(primary_ts) = spans.lookup_expr(db, call_expr_id) else {
        return;
    };

    let msg = format!(
        "this function takes {} argument{} but {} {} supplied",
        expected,
        if expected == 1 { "" } else { "s" },
        actual,
        if actual == 1 { "was" } else { "were" }
    );
    let label = format!(
        "expected {} argument{}",
        expected,
        if expected == 1 { "" } else { "s" }
    );

    let mut builder = datalove_diagnostic::DiagnosticBuilder::error(db, &msg)
        .code("F045")
        .primary_label(primary_ts, &label);

    // Add secondary label for function definition.
    if let Some(def_ts) = spans.lookup_fun_in_module(db, func_module_id, func_local_index) {
        let def_label = format!(
            "function `{}` defined here with {} parameter{}",
            func_name.as_str(db),
            expected,
            if expected == 1 { "" } else { "s" }
        );
        builder = builder.secondary_label(def_ts, &def_label);
    }

    builder.emit_type();
}
