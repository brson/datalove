//! Type checking context.
//!
//! Provides TypeContext for tracking bindings and errors during typechecking,
//! and ScriptTypeContext for tracking accumulated bindings across script units.

use rmx::prelude::*;
use std::collections::{BTreeMap, HashMap};
use bct::text::{InternedText, TextSpan};

use datalove_datafun_ast::ast::*;
use datalove_datafun_intrinsics::IntrinsicId;

pub use datalove_datafun_ast::spans::DatafunSpans;
use crate::{CallTargets, ExprTypes, ErrorSite};
pub use bct::module_graph::ModuleId;

pub use crate::{
    PendingDiagnostic,
    RecoveryHint,
    Type,
    TypeFunction,
    TypeError,
    ResolvedCallTarget,
    ModuleNameResolution,
    CollectedNames,
    AutoAdaptMode,
};

use datalove_datafun_common::{can_clone_coerce_to, convert_type_hint_with_aliases};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashSet};

/// Where a script unit reads the bindings the units before it left behind.
///
/// Held as an `Option` on [`TypeContext`] because the context is shared with
/// module typechecking and a module has no units before it.
///
/// **A lookup miss is the read, and the read is the dependency.** A unit used to
/// be seeded with every earlier unit's bindings, which put the whole prefix's
/// outputs into a per-unit memo key; asking for one name at a time means a unit
/// depends on the names it actually used. See
/// `botdocs/plan-script-reactivity.md`.
#[derive(Clone, Copy)]
pub struct InheritedBindings<'db> {
    /// The script up to the unit *before* the one being checked.
    pub script: crate::Script<'db>,
    /// The modules the script is checked against.
    pub env: crate::ScriptEnv<'db>,
}

/// What a name in scope is bound to.
#[derive(Clone)]
pub(crate) struct VarBinding<'db> {
    pub(crate) ty: Type<'db>,
    pub(crate) is_mutable: bool,
    /// Whether it is a const, which a const expression and a const argument
    /// may name.
    pub(crate) is_const: bool,
}

/// Context for typechecking.
pub struct TypeContext<'db> {
    pub(crate) db: &'db dyn crate::Db,
    /// The earlier script units' bindings, reached on a lookup miss.
    pub(crate) inherited: Option<InheritedBindings<'db>>,
    /// Whether a function body is being checked.
    ///
    /// **A function body is not a closure.** It sees its parameters, the consts
    /// in scope and the functions, and nothing else, so an inherited `let` or
    /// `var` is refused here. Entering a body drops those from `variables`,
    /// which was enough while the enclosing units' bindings were seeded into it;
    /// now that a miss falls through to the earlier units, the body has to say
    /// so.
    pub(crate) in_function_body: bool,
    /// Pre-computed spans for error reporting.
    pub(crate) spans: DatafunSpans<'db>,
    /// Current module being typechecked (for pending diagnostics).
    pub(crate) current_module_id: Option<ModuleId<'db>>,
    /// Variable bindings, by name.
    pub(crate) variables: HashMap<InternedText<'db>, VarBinding<'db>>,
    /// Function signatures (name -> function type).
    pub(crate) functions: HashMap<InternedText<'db>, TypeFunction<'db>>,
    /// Function ASTs for resolving call targets (name -> (AST, module_id)).
    pub(crate) function_asts: HashMap<InternedText<'db>, (StmtFun<'db>, Option<ModuleId<'db>>)>,
    /// Type aliases (name -> resolved type).
    pub(crate) type_aliases: HashMap<InternedText<'db>, Type<'db>>,
    /// Every name this unit asked the environment about.
    ///
    /// A script unit is typechecked against the bindings the units before it
    /// left behind, and what it *asked for* is what it depends on -- which is
    /// the graph script reactivity needs, and the thing nothing recorded.
    ///
    /// **A name that was not found is in here too, and has to be.** A unit that
    /// asks for `x` and does not find it depends on `x` not being there: edit an
    /// earlier unit to define one and this unit's answer changes.
    ///
    /// It is an over-approximation in the other direction, deliberately. A name
    /// the unit binds itself is recorded as well, because the alternative is
    /// tracking which binding a lookup found through every site that writes one
    /// -- and `statement.rs` writes pattern bindings straight into `variables`,
    /// and scopes save and restore the whole map. Over-reporting costs a
    /// dependency that resolves to no provider; under-reporting would keep a
    /// stale type. See `botdocs/plan-script-reactivity.md`.
    asked_names: RefCell<BTreeSet<InternedText<'db>>>,
    /// The bound each of the enclosing function's type parameters carries.
    ///
    /// A bare parameter is absent here and nothing may be done to a value of
    /// it. A bound one says which types it may be, and the operators those
    /// types have in common are allowed on it.
    pub(crate) type_param_bounds:
        HashMap<InternedText<'db>, datalove_datafun_ast::ast::TypeBound>,
    /// Expected return type for current function (if inside a function).
    pub(crate) expected_return_type: Option<Type<'db>>,
    /// Whether current function has no declared return type (void function).
    pub(crate) is_void_function: bool,
    pub(crate) errors: Vec<TypeError>,
    /// Pending diagnostics for post-hoc span enrichment.
    pub(crate) pending_diagnostics: Vec<PendingDiagnostic<'db>>,
    /// Expression types, indexed by ExprFun ID.
    pub(crate) expr_types: ExprTypes<'db>,
    /// Resolved call targets, indexed by ExprFunctionCall ID.
    pub(crate) call_targets: CallTargets<'db>,
    /// What this unit's qualified calls name.
    pub(crate) qualified: crate::QualifiedScope<'db>,
    /// Aliases whose `require` was refused, and reported there.
    ///
    /// A call through one would otherwise say again, less helpfully, that
    /// nothing was required under it.
    pub(crate) failed_aliases: HashSet<String>,
    /// The data files `require data` may name, by path.
    pub(crate) data_files: crate::DataFiles,
    /// The data files a `require data` const named, which lowering reads.
    pub(crate) resolved_data: crate::DataFiles,
    /// Stack of loop depth (for validating break/continue are inside a loop).
    pub(crate) loop_depth: usize,
    /// Whether we're in a reference context (ref/mut/out param or binop operand).
    /// Move-type field projections are allowed in ref context.
    pub(crate) ref_context: bool,
    /// Whether we're in a mutable binding context (mut/out param).
    /// View types (e.g. tensor sub-views) are not allowed in mut context.
    pub(crate) mut_context: bool,
    /// Resolved intrinsic targets, keyed by the expression's stable key.
    pub(crate) intrinsic_targets: BTreeMap<ExprKey<'db>, IntrinsicId>,
    /// Auto-adapt mode for this context.
    pub(crate) auto_adapt_mode: AutoAdaptMode,
    /// Auto-adaptations that were applied (for reporting and IR lowering).
    pub(crate) auto_adaptations: Vec<AutoAdaptation<'db>>,
    /// Whether we are checking the value of a const binding.
    ///
    /// A const expression may only name other consts, since it is evaluated
    /// before anything a parameter or a let could be bound to exists.
    pub(crate) in_const_expr: bool,
}

/// Record of an auto-adaptation that was applied.
#[derive(Clone)]
pub struct AutoAdaptation<'db> {
    /// The expression that was adapted.
    pub expr_key: ExprKey<'db>,
    /// The original type of the expression.
    pub from_type: Type<'db>,
    /// The type after adaptation.
    pub to_type: Type<'db>,
}

impl<'db> TypeContext<'db> {
    pub fn new(
        db: &'db dyn crate::Db,
        spans: DatafunSpans<'db>,
    ) -> Self {
        Self::with_options(db, spans, None, AutoAdaptMode::Disabled)
    }

    /// Create a TypeContext for a specific module.
    pub fn with_module_id(
        db: &'db dyn crate::Db,
        spans: DatafunSpans<'db>,
        module_id: Option<ModuleId<'db>>,
    ) -> Self {
        Self::with_options(db, spans, module_id, AutoAdaptMode::Disabled)
    }

    /// Create a TypeContext with all options.
    pub fn with_options(
        db: &'db dyn crate::Db,
        spans: DatafunSpans<'db>,
        module_id: Option<ModuleId<'db>>,
        auto_adapt_mode: AutoAdaptMode,
    ) -> Self {
        TypeContext {
            db,
            inherited: None,
            in_function_body: false,
            spans,
            current_module_id: module_id,
            variables: HashMap::new(),
            functions: HashMap::new(),
            asked_names: RefCell::new(BTreeSet::new()),
            function_asts: HashMap::new(),
            type_aliases: HashMap::new(),
            type_param_bounds: HashMap::new(),
            expected_return_type: None,
            is_void_function: false,
            errors: Vec::new(),
            pending_diagnostics: Vec::new(),
            expr_types: ExprTypes::new(),
            call_targets: CallTargets::new(),
            qualified: crate::QualifiedScope::default(),
            failed_aliases: HashSet::new(),
            data_files: crate::DataFiles::new(),
            resolved_data: crate::DataFiles::new(),
            loop_depth: 0,
            ref_context: false,
            mut_context: false,
            intrinsic_targets: BTreeMap::new(),
            auto_adapt_mode,
            auto_adaptations: Vec::new(),
            in_const_expr: false,
        }
    }

    /// Get the auto-adapt mode.
    pub fn auto_adapt_mode(&self) -> AutoAdaptMode {
        self.auto_adapt_mode
    }

    /// Get the auto-adaptations that were applied.
    pub fn auto_adaptations(&self) -> &[AutoAdaptation<'db>] {
        &self.auto_adaptations
    }

    /// Get the pending diagnostics.
    pub fn pending_diagnostics(&self) -> &[PendingDiagnostic<'db>] {
        &self.pending_diagnostics
    }

    /// Get the type errors.
    pub fn errors(&self) -> &[TypeError] {
        &self.errors
    }

    pub fn add_error(&mut self, error: TypeError) {
        self.errors.push(error);
    }

    /// Report a function that can reach the end of its body owing a value.
    pub fn report_missing_return(
        &mut self,
        local_index: u32,
        name: InternedText<'db>,
    ) {
        self.pending_diagnostics.push(PendingDiagnostic::MissingReturn {
            local_index,
            module_id: self.current_module_id,
            name,
        });
        self.add_error(TypeError::MissingReturn {
            name: name.as_str(self.db).to_string(),
            fun_local_index: local_index,
        });
    }

    /// Report a `native fun` declared where nothing can implement it.
    pub fn report_native_fun_outside_rider(
        &mut self,
        local_index: u32,
        name: InternedText<'db>,
    ) {
        self.pending_diagnostics.push(PendingDiagnostic::NativeFunOutsideRider {
            local_index,
            module_id: self.current_module_id,
            name,
        });
        self.add_error(TypeError::NativeFunOutsideRider {
            name: name.as_str(self.db).to_string(),
            fun_local_index: local_index,
        });
    }

    /// Run `attempt`, discarding anything it reported if it does not succeed.
    ///
    /// For speculative checks that fall back to another way of typing the same
    /// expression. Without this the abandoned attempt still reports, and the
    /// reader sees the failure of a path the compiler chose not to take
    /// alongside the error it settled on.
    pub fn try_check<R>(
        &mut self,
        attempt: impl FnOnce(&mut Self) -> Option<R>,
    ) -> Option<R> {
        let errors = self.errors.len();
        let diagnostics = self.pending_diagnostics.len();
        match attempt(self) {
            Some(result) => Some(result),
            None => {
                self.errors.truncate(errors);
                self.pending_diagnostics.truncate(diagnostics);
                None
            }
        }
    }

    // Error collection helpers that collect pending diagnostics and return TypeError.
    // Diagnostics are emitted at the end of typechecking via emit_pending_diagnostics().

    /// F001: Undefined variable.
    pub fn error_undefined_variable(&mut self, expr: ExprFun<'db>, name: InternedText<'db>) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::UndefinedVariable {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            name,
        });
        TypeError::UnresolvedName(name.as_str(self.db).S())
    }

    /// F059: A name imported twice.
    ///
    /// Filed against the second import, which is the one that would have
    /// taken over a name the first had already bound.
    pub fn error_duplicate_import(
        &mut self,
        local_index: u32,
        name: InternedText<'db>,
        first: &str,
        second: &str,
    ) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::DuplicateImport {
            local_index,
            module_id: self.current_module_id,
            name,
            first: InternedText::new(self.db, first.S()),
        });
        TypeError::DuplicateImport {
            name: name.as_str(self.db).S(),
            first: first.S(),
            second: second.S(),
        }
    }

    /// F002: Undefined function.
    pub fn error_undefined_function(&mut self, expr: ExprFun<'db>, name: InternedText<'db>) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::UndefinedFunction {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            name,
        });
        TypeError::UnresolvedName(name.as_str(self.db).S())
    }

    /// F011: Cannot synthesize type.
    /// Convert a type hint under the aliases in scope, reporting a name that
    /// resolves to nothing.
    ///
    /// The one thing in a hint that cannot be worked out from the hint alone
    /// is a bare name, and an unresolved one carried no span, so it rendered
    /// nothing and was dropped whenever any other diagnostic was present.
    pub fn convert_hint(
        &mut self,
        type_hint: datalove_datalit::ast::TypeHint<'db>,
    ) -> Result<Type<'db>, TypeError> {
        let result = convert_type_hint_with_aliases(self.db, type_hint, &self.type_aliases);
        if let Err(TypeError::UnresolvedTypeAlias { name, local_index: Some(local_index) }) = &result {
            self.pending_diagnostics.push(PendingDiagnostic::UnresolvedTypeAlias {
                local_index: *local_index,
                module_id: self.current_module_id,
                name: InternedText::new(self.db, name.C()),
            });
        }
        result
    }

    pub fn error_cannot_synthesize(&mut self, expr: ExprFun<'db>, message: &str) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::CannotSynthesize {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            message: InternedText::new(self.db, message.S()),
        });
        TypeError::CannotSynthesize
    }

    /// F016: Type mismatch (simple version without recovery hint).
    pub fn error_type_mismatch(&mut self, expr: ExprFun<'db>, expected: &str, actual: &str, label: &str) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::TypeMismatch {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            expected: InternedText::new(self.db, expected.S()),
            actual: InternedText::new(self.db, actual.S()),
            label: InternedText::new(self.db, label.S()),
            recovery_hint: RecoveryHint::None,
        });
        TypeError::TypeMismatch {
            expected: expected.S(),
            actual: actual.S(),
        }
    }

    /// Try to auto-adapt a type mismatch, or return an error.
    ///
    /// If auto_adapt_mode is enabled and the mismatch is recoverable via `@`,
    /// records the adaptation and returns Ok(()). Otherwise, creates a pending
    /// diagnostic (with recovery hint if applicable) and returns Err.
    ///
    /// This is the main entry point for handling type mismatches in expressions.
    pub fn check_type_mismatch_or_adapt(
        &mut self,
        expr: ExprFun<'db>,
        expected: &Type<'db>,
        actual: &Type<'db>,
        label: &str,
    ) -> Result<(), TypeError> {
        // Check if @ can fix this mismatch.
        let can_adapt = can_clone_coerce_to(actual, expected, self.db);

        if can_adapt && self.auto_adapt_mode.is_enabled() {
            // Auto-adapt: record the adaptation and store the expected type.
            self.auto_adaptations.push(AutoAdaptation {
                expr_key: ExprKey::of(self.db, expr),
                from_type: actual.clone(),
                to_type: expected.clone(),
            });

            // Store the adapted (expected) type for this expression.
            self.store_expr_type(expr, expected);

            // Optionally report what was adapted.
            if self.auto_adapt_mode.should_report() {
                use datalove_datafun_common::type_to_string;
                let from_str = type_to_string(self.db, actual);
                let to_str = type_to_string(self.db, expected);
                // TODO: Emit an info-level diagnostic for the adaptation.
                // For now, we just silently adapt.
                let _ = (from_str, to_str);
            }

            return Ok(());
        }

        // Cannot auto-adapt (or auto-adapt disabled): emit error with recovery hint.
        Err(self.error_type_mismatch_with_types(expr, expected, actual, label))
    }

    /// F016: Type mismatch with recovery hint computation.
    ///
    /// This version takes Type references and checks if the error is recoverable
    /// via the @ (adapt) operator using can_clone_coerce_to.
    pub fn error_type_mismatch_with_types(
        &mut self,
        expr: ExprFun<'db>,
        expected: &Type<'db>,
        actual: &Type<'db>,
        label: &str,
    ) -> TypeError {
        use datalove_datafun_common::type_to_string;

        let expected_str = type_to_string(self.db, expected);
        let actual_str = type_to_string(self.db, actual);

        // Compute recovery hint: can @ fix this mismatch?
        let recovery_hint = if can_clone_coerce_to(actual, expected, self.db) {
            let description = format_adapt_description(self.db, actual, expected);
            RecoveryHint::InsertAdapt { description }
        } else {
            RecoveryHint::None
        };

        self.pending_diagnostics.push(PendingDiagnostic::TypeMismatch {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            expected: InternedText::new(self.db, expected_str.C()),
            actual: InternedText::new(self.db, actual_str.C()),
            label: InternedText::new(self.db, label.S()),
            recovery_hint,
        });
        TypeError::TypeMismatch {
            expected: expected_str,
            actual: actual_str,
        }
    }

    /// F045: Function arity mismatch.
    pub fn error_arity_mismatch(
        &mut self,
        expr: ExprFun<'db>,
        func_name: InternedText<'db>,
        func_ast: Option<(StmtFun<'db>, Option<ModuleId<'db>>)>,
        expected: usize,
        actual: usize,
    ) -> TypeError {
        let (func_local_index, func_module_id) = func_ast
            .map(|(func_ast, mod_id)| (func_ast.local_index(self.db), mod_id))
            .unwrap_or((0, None));

        self.pending_diagnostics.push(PendingDiagnostic::ArityMismatch {
            call_expr_key: ExprKey::of(self.db, expr),
            call_module_id: self.current_module_id,
            func_name,
            func_local_index,
            func_module_id,
            expected,
            actual,
        });
        TypeError::ArityMismatch { expected, actual }
    }

    /// F071: A destructuring pattern that does not fit the value it takes apart.
    ///
    /// Reported at the value, since a pattern has no span of its own.
    pub fn error_pattern_mismatch(&mut self, value: ExprFun<'db>, message: String, label: &str) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::PatternMismatch {
            expr_key: ExprKey::of(self.db, value),
            module_id: self.current_module_id,
            message: InternedText::new(self.db, message.C()),
            label: InternedText::new(self.db, label.S()),
        });
        TypeError::PatternMismatch { message }
    }

    /// F046: Result destructuring requires error binding.
    pub fn error_result_requires_binding(&mut self, expr: ExprFun<'db>) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::ResultRequiresBinding {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
        });
        TypeError::ResultRequiresErrorBinding
    }

    /// Report a field or index projection that failed, and pass its error on.
    pub fn report_place_error(&mut self, site: crate::ErrorSite<'db>, error: TypeError) -> TypeError {
        let (code, message, label, note) = match &error {
            TypeError::FieldNotFound { field_name, ty } => (
                "F067",
                fmt!("`{ty}` has no field `{field_name}`"),
                S("no such field"),
                None,
            ),
            TypeError::ProjectionOnNonAggregate { ty } => (
                "F068",
                fmt!("`{ty}` has no fields"),
                S("only a struct or a tuple has fields"),
                None,
            ),
            TypeError::FieldIndexOutOfBounds { index, tuple_size } => (
                "F069",
                fmt!("a tuple of {tuple_size} has no element {index}"),
                S("no such element"),
                None,
            ),
            TypeError::NonCopyFieldProjection { field_ty } => (
                "F070",
                fmt!("a field of type `{field_ty}` cannot be read out of what holds it"),
                S("this would move the field out"),
                Some(S("the field's type is not a copy type, so reading it would move it while the \
aggregate still holds it. Borrow it with `ref`, or clone it out with `@`.")),
            ),
            TypeError::NonCopyIndexProjection { elem_ty } => (
                "F072",
                fmt!("an element of type `{elem_ty}` cannot be read out of what holds it"),
                S("this would move the element out"),
                Some(S("the element's type is not a copy type, so reading it would move it while the \
collection still holds it. Borrow it with `ref`, or clone it out with `@`.")),
            ),
            TypeError::ViewTypeMutBinding { view_ty } => (
                "F073",
                fmt!("a view of type `{view_ty}` cannot be written through"),
                S("indexing a tensor of rank above one gives a view into it"),
                Some(S("a view shows part of the tensor it indexes, and writing it whole would \
replace the view rather than the elements it shows. Index down to single elements and write those.")),
            ),
            _ => unreachable!("not a projection error: {error:?}"),
        };
        self.push_coded(site, code, message, label, note);
        error
    }

    /// Report an error about a `set` statement's target, and pass it on.
    ///
    /// For the faults only a write has, which have no `TypeError` of their own
    /// to word them from.
    pub fn report_set_error(
        &mut self,
        stmt: &StmtSet<'db>,
        code: &str,
        message: String,
        label: &str,
        note: Option<&str>,
    ) -> TypeError {
        self.push_coded(ErrorSite::Set(stmt.local_index), code, message.C(), S(label), note.map(S));
        TypeError::DatalitError(message)
    }

    /// Report an import that names nothing. F002 for a missing function, F078
    /// for a missing module or rider.
    pub fn report_unresolved_import(&mut self, unresolved: &crate::UnresolvedImport) {
        let site = ErrorSite::Import(unresolved.local_index);
        let module = &unresolved.module;
        let error = match &unresolved.item {
            Some(item) => {
                self.push_coded(site, "F002", fmt!("`{module}` has no function `{item}`"), S("no such function"), None);
                TypeError::UnresolvedName(fmt!("{module}.{item}"))
            }
            None => {
                self.push_coded(
                    site,
                    "F078",
                    fmt!("nothing named `{module}` is required to import from"),
                    S("no such module or rider"),
                    Some(S("an import names a module or rider required first, with `require module` or `require rider`")),
                );
                TypeError::UnresolvedName(fmt!("module {module}"))
            }
        };
        self.add_error(error);
    }

    /// Report a `require` refused where it is written.
    pub fn report_require_error(&mut self, error: &crate::RequireError) {
        use crate::RequireErrorKind as Kind;
        self.failed_aliases.insert(error.alias.C());
        let site = ErrorSite::Require(error.local_index);
        let (code, message, label, note) = match &error.kind {
            Kind::ModuleNotFound { path } => (
                "F079",
                fmt!("no module `{path}` to require"),
                S("no such module"),
                None,
            ),
            Kind::RiderNotFound { name } => (
                "F079",
                fmt!("no rider `{name}` to require"),
                S("no such rider"),
                None,
            ),
            Kind::Duplicate { what } => (
                "F080",
                fmt!("`{what}` is required twice"),
                S("required already, above"),
                None,
            ),
            Kind::AliasTaken { alias, first, path } => (
                "F081",
                fmt!("`{alias}` already names `{first}`"),
                fmt!("`{path}` would be called through `{alias}` too"),
                Some(S("a module is called through the last part of its path, so two modules of one name cannot be required together")),
            ),
            Kind::RiderInScript { name } => (
                "F082",
                fmt!("a script cannot require a rider, here `{name}`"),
                S("riders are required by modules"),
                Some(S("a rider's functions reach a script through a module of its package, which requires the rider")),
            ),
            Kind::Cycle { cycle } => {
                let target = &cycle[1];
                let path = cycle.iter().map(|p| fmt!("`{p}`")).collect::<Vec<_>>().join(" -> ");
                (
                    "F083",
                    fmt!("requiring `{target}` makes a cycle"),
                    fmt!("{path}"),
                    Some(S("modules cannot require each other in a cycle; move what they share into a module each of them requires")),
                )
            }
        };
        self.push_coded(site, code, message.C(), label, note);
        self.add_error(TypeError::DatalitError(message));
    }

    /// The function a qualified call names, reporting the call if it names none.
    ///
    /// F078 when nothing is required under the alias, as for an import, with a
    /// word for the reader who wrote a value there expecting a method; F002 when
    /// the module or rider has no such function.
    pub fn lookup_qualified(
        &mut self,
        expr: ExprFun<'db>,
        alias: InternedText<'db>,
        name: InternedText<'db>,
    ) -> Result<crate::QualifiedFunction<'db>, TypeError> {
        if let Some(found) = self.qualified.functions.iter().find(|f| f.alias == alias && f.name == name) {
            return Ok(*found);
        }
        let site = ErrorSite::Expr(ExprKey::of(self.db, expr));
        let module = alias.as_str(self.db);
        let item = name.as_str(self.db);
        if self.qualified.aliases.contains(&alias) {
            self.push_coded(site, "F002", fmt!("`{module}` has no function `{item}`"), S("no such function"), None);
            return Err(TypeError::UnresolvedName(fmt!("{module}.{item}")));
        }
        if self.failed_aliases.contains(module) {
            return Err(TypeError::UnresolvedName(fmt!("module {module}")));
        }
        let note = if self.variables.contains_key(&alias) {
            fmt!("`{module}` is a value here, and a value has no functions to call: pass it as an argument instead")
        } else {
            S("a qualified call names a module or rider required first, with `require module` or `require rider`")
        };
        self.push_coded(
            site,
            "F078",
            fmt!("nothing named `{module}` is required to call into"),
            S("no such module or rider"),
            Some(note),
        );
        Err(TypeError::UnresolvedName(fmt!("module {module}")))
    }

    /// Report a diagnostic worded by the caller, at `site`.
    pub fn push_coded(
        &mut self,
        site: ErrorSite<'db>,
        code: &str,
        message: String,
        label: String,
        note: Option<String>,
    ) {
        self.pending_diagnostics.push(PendingDiagnostic::Coded {
            site,
            module_id: self.current_module_id,
            code: InternedText::new(self.db, code.S()),
            message: InternedText::new(self.db, message),
            label: InternedText::new(self.db, label),
            note: note.map(|n| InternedText::new(self.db, n)),
        });
    }

    /// F065: A literal out of range for the type it is checked against.
    ///
    /// Worded as datalit words it, with the range the type has.
    pub fn error_literal_out_of_range(
        &mut self,
        expr: ExprFun<'db>,
        hex: bool,
        ty: &datalove_datalit::tycheck::Type<'db>,
    ) -> TypeError {
        use datalove_datalit::tycheck::{hex_type_range_info, int_type_range_info, type_to_string};
        let ty_str = type_to_string(self.db, ty);
        let (message, (_, note)) = if hex {
            (fmt!("hex literal out of range for type {ty_str}"), hex_type_range_info(ty))
        } else {
            (fmt!("integer literal out of range for type {ty_str}"), int_type_range_info(ty))
        };
        self.pending_diagnostics.push(PendingDiagnostic::LiteralOutOfRange {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            message: InternedText::new(self.db, message),
            note: InternedText::new(self.db, note.S()),
        });
        TypeError::IntOutOfRange
    }

    pub fn error_invalid_operand_type(&mut self, expr: ExprFun<'db>, op: &str, ty: &str) -> TypeError {
        self.error_invalid_operand_type_because(expr, op, ty, None)
    }

    /// F026, with a note saying which part of the type is at fault.
    pub fn error_invalid_operand_type_because(
        &mut self,
        expr: ExprFun<'db>,
        op: &str,
        ty: &str,
        note: Option<&str>,
    ) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::InvalidOperandType {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            op: InternedText::new(self.db, op.S()),
            ty: InternedText::new(self.db, ty.S()),
            note: note.map(|n| InternedText::new(self.db, n.S())),
        });
        TypeError::InvalidOperandType {
            op: op.S(),
            ty: ty.S(),
        }
    }

    /// F048: Try operator type mismatch.
    pub fn error_try_type_mismatch(&mut self, expr: ExprFun<'db>, operator: &str, expected: &str, actual: &str) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::TryTypeMismatch {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            operator: InternedText::new(self.db, operator.S()),
            expected: InternedText::new(self.db, expected.S()),
            actual: InternedText::new(self.db, actual.S()),
        });
        TypeError::TryTypeMismatch {
            operator: operator.S(),
            actual_type: actual.S(),
        }
    }

    /// F049: Try operator return type mismatch.
    pub fn error_try_return_type_mismatch(&mut self, expr: ExprFun<'db>, operator: &str, expected: &str, actual: &str) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::TryReturnTypeMismatch {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            operator: InternedText::new(self.db, operator.S()),
            expected: InternedText::new(self.db, expected.S()),
            actual: InternedText::new(self.db, actual.S()),
        });
        TypeError::TryReturnTypeMismatch {
            operator: operator.S(),
            return_type: actual.S(),
        }
    }

    /// Look up span for a datafun expression.
    pub fn get_span(&self, expr: ExprFun<'db>) -> Option<TextSpan<'db>> {
        self.spans.lookup(self.db, expr).map(|entry| {
            let (text, span) = entry.to_text_and_span(self.db);
            TextSpan::new(text, span)
        })
    }

    /// Emit all pending diagnostics using local spans.
    ///
    /// Called at the end of typechecking for non-module-graph paths.
    pub fn emit_pending_diagnostics(&self) {
        let span_lookup = crate::emit::LocalSpanLookup::new(&self.spans);
        crate::emit::emit_pending_diagnostics(self.db, &self.pending_diagnostics, &span_lookup);
    }

    /// F050: Break outside loop.
    pub fn error_break_outside_loop(&mut self, stmt: &StmtBreak) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::BreakOutsideLoop {
            local_index: stmt.local_index,
            module_id: self.current_module_id,
        });
        TypeError::BreakOutsideLoop
    }

    /// F051: Continue outside loop.
    pub fn error_continue_outside_loop(&mut self, stmt: &StmtContinue) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::ContinueOutsideLoop {
            local_index: stmt.local_index,
            module_id: self.current_module_id,
        });
        TypeError::ContinueOutsideLoop
    }

    /// F052: Void function cannot return a value.
    pub fn error_void_function_returns_value(&mut self, stmt: &StmtRet) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::VoidFunctionReturnsValue {
            local_index: stmt.local_index,
            module_id: self.current_module_id,
        });
        TypeError::VoidFunctionReturnsValue
    }

    /// F053: Non-void function requires return value.
    pub fn error_function_requires_return_value(&mut self, stmt: &StmtRet) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::FunctionRequiresReturnValue {
            local_index: stmt.local_index,
            module_id: self.current_module_id,
        });
        TypeError::FunctionRequiresReturnValue
    }

    /// F054: Undefined variable in set statement.
    pub fn error_undefined_variable_set(&mut self, stmt: &StmtSet, name: &str) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::UndefinedVariableSet {
            local_index: stmt.local_index,
            module_id: self.current_module_id,
            name: InternedText::new(self.db, name.S()),
        });
        TypeError::UndefinedVariable
    }

    /// F055: Cannot assign to immutable variable.
    pub fn error_variable_not_mutable(&mut self, stmt: &StmtSet, name: &str) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::VariableNotMutable {
            local_index: stmt.local_index,
            module_id: self.current_module_id,
            name: InternedText::new(self.db, name.S()),
        });
        TypeError::VariableNotMutable
    }

    /// F058: A const expression referenced a binding that is not itself const.
    pub fn error_non_const_in_const_expr(
        &mut self,
        expr: ExprFun<'db>,
        name: InternedText<'db>,
    ) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::NonConstInConstExpr {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            name,
        });
        TypeError::NonConstInConstExpr(name.as_str(self.db).S())
    }

    /// F057: Call-site mode marker disagrees with the declared parameter mode.
    pub fn error_argument_mode_mismatch(
        &mut self,
        arg: ExprFun<'db>,
        param_idx: usize,
        expected: &str,
        found: &str,
    ) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::ArgumentModeMismatch {
            expr_key: ExprKey::of(self.db, arg),
            module_id: self.current_module_id,
            expected: InternedText::new(self.db, expected.S()),
            found: InternedText::new(self.db, found.S()),
        });
        TypeError::ArgumentModeMismatch {
            param_idx,
            expected: expected.S(),
            found: found.S(),
        }
    }

    /// Check if we're at module top level (not inside a function).
    pub fn is_module_top_level(&self) -> bool {
        self.current_module_id.is_some() && self.expected_return_type.is_none()
    }

    /// Bind a name, replacing whatever it named, a const included.
    pub fn add_variable(&mut self, name: InternedText<'db>, ty: Type<'db>, is_mutable: bool) {
        self.variables.insert(name, VarBinding { ty, is_mutable, is_const: false });
    }

    /// Add a function signature only (for imports where AST comes from another module).
    pub fn add_function(&mut self, name: InternedText<'db>, func_type: TypeFunction<'db>) {
        self.functions.insert(name, func_type);
    }

    /// Add a function with its AST (for local function definitions).
    pub fn add_function_with_ast(
        &mut self,
        name: InternedText<'db>,
        func_type: TypeFunction<'db>,
        func_ast: StmtFun<'db>,
        module_id: Option<ModuleId<'db>>,
    ) {
        self.functions.insert(name, func_type);
        self.function_asts.insert(name, (func_ast, module_id));
    }

    /// Add an imported function with its resolved AST.
    pub fn add_imported_function(
        &mut self,
        name: InternedText<'db>,
        func_type: TypeFunction<'db>,
        func_ast: StmtFun<'db>,
        source_module_id: ModuleId<'db>,
    ) {
        self.functions.insert(name, func_type);
        self.function_asts.insert(name, (func_ast, Some(source_module_id)));
    }

    /// Execute a closure in a new variable scope.
    ///
    /// Saves and restores the variable bindings around the closure,
    /// so any variables added inside are not visible outside.
    pub fn with_scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let saved = self.variables.clone();
        let result = f(self);
        self.variables = saved;
        result
    }

    pub fn lookup_variable(&self, name: InternedText<'db>) -> Option<Type<'db>> {
        self.note_asked(name);
        match self.variables.get(&name) {
            Some(binding) => Some(binding.ty.clone()),
            None => self.inherited_variable(name).map(|(ty, _)| ty),
        }
    }

    pub fn lookup_variable_mutability(&self, name: InternedText<'db>) -> Option<bool> {
        self.note_asked(name);
        match self.variables.get(&name) {
            Some(binding) => Some(binding.is_mutable),
            None => self.inherited_variable(name).map(|(_, is_mutable)| is_mutable),
        }
    }

    pub fn lookup_function(&self, name: InternedText<'db>) -> Option<TypeFunction<'db>> {
        self.note_asked(name);
        match self.functions.get(&name) {
            Some(func_ty) => Some(*func_ty),
            None => self.inherited(name).and_then(|binding| binding.func),
        }
    }

    /// Record that something asked about `name`, found or not.
    fn note_asked(&self, name: InternedText<'db>) {
        self.asked_names.borrow_mut().insert(name);
    }

    /// What the earlier script units left behind under `name`.
    fn inherited(&self, name: InternedText<'db>) -> Option<crate::ScriptBinding<'db>> {
        let inherited = self.inherited?;
        crate::api::binding_at(self.db, inherited.script, inherited.env, name)
    }

    /// The variable an earlier unit left behind under `name`, if a body may see it.
    ///
    /// Inside a function body only a const may be named, for the reason
    /// [`TypeContext::in_function_body`] gives.
    fn inherited_variable(&self, name: InternedText<'db>) -> Option<(Type<'db>, bool)> {
        let binding = self.inherited(name)?;
        if self.in_function_body && !binding.is_const {
            return None;
        }
        binding.var
    }

    /// Every name asked of the environment, in order.
    ///
    /// Ordered because it is part of a memoized value and a `BTreeSet` of
    /// `InternedText` iterates by salsa id; sorting by text is what keeps it the
    /// same between runs. See the determinism section of salsa-patterns.md.
    pub fn asked_names(&self) -> Vec<InternedText<'db>> {
        let mut names: Vec<InternedText<'db>> = self.asked_names.borrow().iter().copied().collect();
        names.sort_by(|a, b| a.as_str(self.db).cmp(b.as_str(self.db)));
        names
    }

    /// Add a type alias to the context.
    pub fn add_type_alias(&mut self, name: InternedText<'db>, ty: Type<'db>) {
        self.type_aliases.insert(name, ty);
    }

    /// Look up a type alias by name.
    pub fn lookup_type_alias(&self, name: InternedText<'db>) -> Option<Type<'db>> {
        self.type_aliases.get(&name).cloned()
    }

    /// Look up the resolved function AST by name.
    pub fn lookup_function_ast(&self, name: InternedText<'db>) -> Option<(StmtFun<'db>, Option<ModuleId<'db>>)> {
        self.note_asked(name);
        match self.function_asts.get(&name) {
            Some(found) => Some(*found),
            None => self.inherited(name).and_then(|binding| binding.func_ast),
        }
    }

    /// Store resolved call target for a function call expression.
    pub fn store_call_target(
        &mut self,
        call: ExprFunctionCall<'db>,
        func: StmtFun<'db>,
        module_id: Option<ModuleId<'db>>,
        type_args: Vec<datalove_datalit::tycheck::Type<'db>>,
    ) {
        self.call_targets.insert(
            ExprKey::of_call(self.db, call),
            ResolvedCallTarget::new(self.db, func, module_id, type_args),
        );
    }

    /// Store the type for an expression.
    pub fn store_expr_type(&mut self, expr: ExprFun<'db>, ty: &Type<'db>) {
        self.expr_types.insert(ExprKey::of(self.db, expr), ty.clone());
    }

    /// Synthesize the type of an expression.
    pub fn synthesize_expr(&mut self, expr: ExprFun<'db>) -> Result<Type<'db>, TypeError> {
        let ty = crate::synthesize::synthesize_expr(self, expr)?;
        self.store_expr_type(expr, &ty);
        Ok(ty)
    }

    /// Synthesize an expression's type leaving aside the hint written on it.
    ///
    /// See [`crate::synthesize::synthesize_unhinted`].
    pub fn synthesize_unhinted(&mut self, expr: ExprFun<'db>) -> Result<Type<'db>, TypeError> {
        let ty = crate::synthesize::synthesize_unhinted(self, expr)?;
        self.store_expr_type(expr, &ty);
        Ok(ty)
    }

    /// Store resolved intrinsic target for an intrinsic call expression.
    ///
    /// Keyed like [`Self::store_expr_type`] rather than indexed by the
    /// expression's salsa id. A `Vec` indexed by `id.index()` grew to the
    /// highest index salsa had handed out - 3764 slots to hold 8 targets in
    /// one measured fixture - and the index alone drops the generation that
    /// tells two expressions in a reused slot apart.
    pub fn store_intrinsic_target(&mut self, expr: ExprFun<'db>, intrinsic: IntrinsicId) {
        self.intrinsic_targets.insert(ExprKey::of(self.db, expr), intrinsic);
    }

    /// Get resolved intrinsic target for an expression.
    pub fn get_intrinsic_target(&self, expr: ExprFun<'db>) -> Option<IntrinsicId> {
        self.intrinsic_targets.get(&ExprKey::of(self.db, expr)).copied()
    }

    // ========================================================================
    // Comptime Support
    // ========================================================================

    /// Mark the name just bound as a const.
    ///
    /// A const whose value did not typecheck bound nothing, and there is
    /// nothing to mark.
    pub fn add_const_binding(&mut self, name: InternedText<'db>) {
        if let Some(binding) = self.variables.get_mut(&name) {
            binding.is_const = true;
        }
    }

    /// Check if a name is a const binding.
    ///
    /// Asked of what the name means here, so a `let` or a parameter that
    /// shadows a const is not one. A const declared by an earlier script unit
    /// counts, which is what lets a function body and a const expression name
    /// one across a unit boundary.
    pub fn is_const_binding(&self, name: InternedText<'db>) -> bool {
        match self.variables.get(&name) {
            Some(binding) => binding.is_const,
            None => self.inherited(name).is_some_and(|binding| binding.is_const),
        }
    }

    /// Seed context from a pre-computed name resolution.
    ///
    /// Populates type aliases, function signatures, and function ASTs from
    /// the memoized name resolution pass. This replaces the inline Pass 0/1
    /// collection in typecheck_module.
    pub fn seed_from_name_resolution(
        &mut self,
        name_resolution: &ModuleNameResolution<'db>,
        module_id: Option<ModuleId<'db>>,
    ) {
        self.record_resolution_errors(name_resolution.errors(self.db), module_id);

        // Add type aliases.
        for (name, ty) in name_resolution.type_aliases(self.db) {
            self.type_aliases.insert(*name, ty.clone());
        }

        // Add function signatures with ASTs.
        // First build a map of ASTs for lookup.
        let ast_map: HashMap<bct::text::InternedText<'db>, StmtFun<'db>> = name_resolution.function_asts(self.db)
            .iter()
            .map(|(name, ast)| (*name, *ast))
            .collect();

        for (name, func_type) in name_resolution.functions(self.db) {
            if let Some(&func_ast) = ast_map.get(name) {
                self.add_function_with_ast(*name, *func_type, func_ast, module_id);
            } else {
                self.add_function(*name, *func_type);
            }
        }
    }

    /// Seed context from collected names (used by scripts).
    ///
    /// Take the errors name resolution found, and give a span to the ones
    /// that have somewhere to point.
    ///
    /// A signature that does not resolve leaves its function unregistered, so
    /// every call to it reports F002 as well. That one names a real symptom
    /// and no cause, which is why the cause is not left as a line in a list.
    fn record_resolution_errors(
        &mut self,
        errors: &[TypeError],
        module_id: Option<ModuleId<'db>>,
    ) {
        for error in errors {
            if let TypeError::TypeParamNotErasable { param, position, fun_local_index } = error {
                self.pending_diagnostics.push(PendingDiagnostic::TypeParamNotErasable {
                    local_index: *fun_local_index,
                    module_id,
                    param: InternedText::new(self.db, param.S()),
                    position: InternedText::new(self.db, position.S()),
                });
            }
            if let TypeError::CollectionKeyNotOrdered { param, position, fun_local_index } = error {
                self.pending_diagnostics.push(PendingDiagnostic::CollectionKeyNotOrdered {
                    local_index: *fun_local_index,
                    module_id,
                    param: InternedText::new(self.db, param.S()),
                    position: InternedText::new(self.db, position.S()),
                });
            }
            if let TypeError::DuplicateTypeAlias { name, local_index } = error {
                self.pending_diagnostics.push(PendingDiagnostic::DuplicateTypeAlias {
                    local_index: *local_index,
                    module_id,
                    name: InternedText::new(self.db, name.C()),
                });
            }
            // Name resolution converts the type hints in signatures and in
            // alias definitions, so a name that resolves to nothing in one of
            // those is found here rather than while checking a body.
            if let TypeError::UnresolvedTypeAlias { name, local_index: Some(local_index) } = error {
                self.pending_diagnostics.push(PendingDiagnostic::UnresolvedTypeAlias {
                    local_index: *local_index,
                    module_id,
                    name: InternedText::new(self.db, name.C()),
                });
            }
            self.add_error(error.clone());
        }
    }

    /// Like `seed_from_name_resolution` but takes `CollectedNames` directly
    /// instead of the tracked `ModuleNameResolution` struct.
    pub fn seed_from_collected_names(
        &mut self,
        collected: &CollectedNames<'db>,
        module_id: Option<ModuleId<'db>>,
    ) {
        self.record_resolution_errors(&collected.errors, module_id);

        // Add type aliases.
        for (name, ty) in &collected.type_aliases {
            self.type_aliases.insert(*name, ty.clone());
        }

        // Add function signatures with ASTs.
        let ast_map: HashMap<bct::text::InternedText<'db>, StmtFun<'db>> = collected.function_asts
            .iter()
            .map(|(name, ast)| (*name, *ast))
            .collect();

        for (name, func_type) in &collected.functions {
            if let Some(&func_ast) = ast_map.get(name) {
                self.add_function_with_ast(*name, *func_type, func_ast, module_id);
            } else {
                self.add_function(*name, *func_type);
            }
        }
    }
}

/// Format a description of what the @ operator does for this conversion.
fn format_adapt_description<'db>(
    db: &'db dyn salsa::Database,
    from: &Type<'db>,
    to: &Type<'db>,
) -> String {
    use datalove_datafun_common::{type_to_string, types_equivalent};

    if types_equivalent(db, from, to) {
        "clone the value".to_string()
    } else {
        format!("convert from `{}` to `{}`", type_to_string(db, from), type_to_string(db, to))
    }
}
