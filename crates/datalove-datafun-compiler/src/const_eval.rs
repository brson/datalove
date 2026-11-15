//! Evaluating const bindings: lowering each initializer and running it.
//!
//! Consts are evaluated in three places -- a module's or a script's top level,
//! a function body, and a comptime function's body once an instantiation has
//! given its const parameters values -- by the module compiler and by the
//! script compiler alike. Each binding is lowered to a small unit, or read
//! straight off when it is a literal or names a const already evaluated, and
//! the unit is run by the CTFE evaluator. Both compilers do it through here.

use rmx::prelude::*;
use std::collections::BTreeSet;
use bct::diagnostic::{DiagnosticBuilder, StoredDiagnostic};
use bct::lookup::LookupMap;
use std::sync::Arc;

use datalove_datafun_ast::ast::{Body, ExprFun, ExprFunKind, Statement, StmtFun};
use datalove_datafun_ast::spans::DatafunSpans;
use datalove_diagnostic_codes::CCode;
use datalove_datafun_const::evaluate_const_unit;
use datalove_datafun_ir::{
    ConstValue, CtfeError, CtfeEvaluator, FuncId, IrCodeUnit, IrType,
};
use datalove_datafun_sema::{CallTargets, ExprTypes};

use crate::lower::{self, LowerError};
use crate::tracked_lower::FuncIdLookup;
use crate::IrTypeExt;

/// What a body's consts are evaluated against.
pub struct ConstEvalEnv<'a, 'db> {
    pub db: &'db dyn salsa::Database,
    pub expr_types: &'db ExprTypes<'db>,
    pub call_targets: &'db CallTargets<'db>,
    pub evaluator: &'a mut dyn CtfeEvaluator,
    /// The functions a const may call, lowered already.
    pub lowered: &'a [Arc<IrCodeUnit>],
    pub func_name_to_id: &'a LookupMap<String, FuncId>,
    /// For calls into other modules.
    pub func_id_map: &'db FuncIdLookup<'db>,
    /// For a script const's calls into earlier units: each function they
    /// define, by name, as the unit's id and the function's id there. Empty
    /// for a module's.
    pub earlier_functions: &'a LookupMap<String, (u32, FuncId)>,
    /// The data files a `require data` const may name, by path.
    pub data_files: &'db lower::DataFiles,
}

/// Why a const could not be evaluated.
#[derive(Debug)]
pub enum ConstError {
    /// It names something with no value here: a const that failed, or a
    /// binding of an earlier script unit that did not compile.
    Unbound {
        name: String,
        /// Where the initializer writes it; see `LowerError::BindingNotAvailable`.
        at: Option<lower::WrittenAt>,
    },
    /// Running it gave no value.
    Ctfe(CtfeError),
}

/// A const that could not be evaluated, for want of anything but another
/// const that could not be.
#[derive(Debug)]
pub struct ConstFailure<'db> {
    pub name: String,
    pub init: ExprFun<'db>,
    pub error: ConstError,
}

impl<'db> ConstFailure<'db> {
    /// The diagnostic for a const that could not be evaluated, at its
    /// initializer in `spans`.
    ///
    /// A name with no value is reported where the name is known to be
    /// missing, which a module never is: everything its consts see is its
    /// own, so the only const it names with no value is one that failed.
    pub fn diagnostic(&self, db: &'db dyn salsa::Database, spans: &DatafunSpans<'db>) -> StoredDiagnostic {
        let ConstError::Ctfe(error) = &self.error else {
            panic!("`{}` names a binding with no value, which only a script unit can", self.name);
        };
        ctfe_diagnostic(db, spans, &self.name, self.init, error)
    }
}

/// The diagnostic for running `name`'s initializer `init` failing with `error`.
pub fn ctfe_diagnostic<'db>(
    db: &'db dyn salsa::Database,
    spans: &DatafunSpans<'db>,
    name: &str,
    init: ExprFun<'db>,
    error: &CtfeError,
) -> StoredDiagnostic {
    let at = spans.lookup_key(init.key())
        .expect("a const's initializer was parsed with a span");
    let builder = match error {
        CtfeError::EarlyReturn(returned) => {
            DiagnosticBuilder::error(db, &fmt!("evaluating `{name}` returned early with {returned}"))
                .code(CCode::ConstReturnedEarly.as_str())
                .primary_label(at, "this returned early")
        }
        CtfeError::StackOverflow => {
            DiagnosticBuilder::error(db, &fmt!("evaluating `{name}` overflowed the stack"))
                .code(CCode::ConstStackOverflow.as_str())
                .primary_label(at, "this recursed too deeply")
        }
        CtfeError::Failed(message) => {
            DiagnosticBuilder::error(db, &fmt!("evaluating `{name}` failed: {message}"))
                .code(CCode::ConstFailed.as_str())
                .primary_label(at, "this failed")
        }
    };
    builder
        .note("a const is evaluated as the program is compiled, by running it")
        .build_stored()
}

/// The diagnostic for function `func` being given more values for its const
/// parameters than it may be copied for.
pub fn too_many_instantiations<'db>(
    db: &'db dyn salsa::Database,
    spans: &DatafunSpans<'db>,
    func: &'db StmtFun<'db>,
    count: usize,
) -> StoredDiagnostic {
    let at = spans.lookup_fun(func.key().local_index)
        .expect("a function was parsed with a span")
        .C();
    let limit = crate::specialize::MAX_INSTANTIATIONS;
    DiagnosticBuilder::error(db, &fmt!(
        "`{}` is given {count} different values for its const parameters, over the limit of {limit}",
        func.name().text(db),
    ))
        .code(CCode::TooManyInstantiations.as_str())
        .primary_label(at, "each is a copy of this function")
        .build_stored()
}

/// The consts these statements bind to data files, with their values.
///
/// Data needs no evaluating, so these have values before anything is lowered,
/// and a function that reads one does not wait for the consts as a function
/// naming any other const does. Which is what lets a const be worked out from
/// data by a function that reads it: waiting, that function would have been
/// out of reach of the evaluation it was needed for. A const whose type did not
/// check has no value, as an evaluated one would not.
pub fn data_consts<'db>(
    db: &'db dyn salsa::Database,
    body: &'db Body<'db>,
    expr_types: &ExprTypes<'db>,
    data_files: &lower::DataFiles,
) -> LookupMap<String, (IrType, Arc<ConstValue>)> {
    body.statements.iter()
        .filter_map(|statement| {
            let Statement::Const(binding) = statement else { return None };
            let value = body.arena.get(binding.value);
            if !matches!(value.kind(), ExprFunKind::DataFile(_)) {
                return None;
            }
            let ty = expr_types.get(value)?;
            let ty = IrType::from_tycheck(db, ty);
            let value = lower::try_extract_literal(db, value, &ty, data_files)
                .expect("a data file is a literal");
            Some((binding.name.text(db).O(), (ty, value)))
        })
        .collect()
}

/// The type the typechecker gave a const's initializer.
pub fn const_type(env: &ConstEvalEnv<'_, '_>, init: ExprFun<'_>) -> IrType {
    let ty = env.expr_types.get(init)
        .expect("the typechecker types every const it accepts");
    IrType::from_tycheck(env.db, ty)
}

/// Evaluate one const's initializer.
///
/// `resolved` is the consts it may name, and `deferred` the names that have no
/// value here: a function's const parameters, and the consts naming them. A
/// const naming one has a value per instantiation rather than one, so it is
/// not evaluated, and `None` says so; it lowers as an ordinary binding and is
/// evaluated in each copy. `func_return_type` is what an early-return
/// operator in it is checked against.
pub fn evaluate_const<'db>(
    env: &mut ConstEvalEnv<'_, 'db>,
    init: ExprFun<'db>,
    ty: &IrType,
    resolved: &LookupMap<String, (IrType, Arc<ConstValue>)>,
    func_return_type: Option<IrType>,
    deferred: &BTreeSet<String>,
) -> Result<Option<Arc<ConstValue>>, ConstError> {
    let lowered = lower::lower_const_binding(
        env.db, init, ty, env.expr_types, env.call_targets, resolved, func_return_type,
        env.lowered, env.func_name_to_id, Some(env.func_id_map), env.earlier_functions, env.data_files,
    );
    let (unit, value) = match lowered {
        Ok(pair) => pair,
        Err(LowerError::BindingNotAvailable { name, .. }) if deferred.contains(&name) => {
            return Ok(None);
        }
        Err(LowerError::BindingNotAvailable { name, at }) => return Err(ConstError::Unbound { name, at }),
    };
    match (unit, value) {
        (None, Some(value)) => Ok(Some(value)),
        (Some(unit), None) => {
            // Every function this reaches is lowered already: the typechecker
            // refused a const that depends on one waiting on a const at or
            // below it (F089), and lowering orders the rest.
            evaluate_const_unit(&unit, ty, &mut *env.evaluator)
                .map(Some)
                .map_err(ConstError::Ctfe)
        }
        _ => unreachable!("lower_const_binding returns exactly one of unit or value"),
    }
}

/// Evaluate the consts in a function body, in order, so that each may name
/// those above it and those in `scope`.
///
/// `deferred` is the names with no value here, a comptime function's const
/// parameters; a const naming one, or naming such a const, is left out. An
/// instantiation gives its parameters values in `scope` instead and defers
/// nothing.
///
/// `failed` is the consts outside the body that could not be evaluated. A
/// const naming one of those, or one of the body's that could not be, fails
/// with it and is not reported: what it would say follows from the first.
pub fn evaluate_body_consts<'db>(
    env: &mut ConstEvalEnv<'_, 'db>,
    func_stmt: &'db StmtFun<'db>,
    mut scope: LookupMap<String, (IrType, Arc<ConstValue>)>,
    mut deferred: BTreeSet<String>,
    mut failed: BTreeSet<String>,
) -> (Vec<(String, IrType, Arc<ConstValue>)>, Vec<ConstFailure<'db>>) {
    let db = env.db;
    let func_return_type = func_stmt.return_type().map(|ty| IrType::from_type_hint(db, &ty));
    let mut consts = Vec::new();
    let mut failures = Vec::new();
    let body = func_stmt.body();
    for stmt in &body.statements {
        let Statement::Const(const_stmt) = stmt else { continue };
        let name = const_stmt.name.text(db).S();
        let init = body.arena.get(const_stmt.value);
        let ty = const_type(env, init);
        match evaluate_const(env, init, &ty, &scope, func_return_type.C(), &deferred) {
            Ok(Some(value)) => {
                scope.insert(name.C(), (ty.C(), value.C()));
                consts.push((name, ty, value));
            }
            Ok(None) => {
                deferred.insert(name);
            }
            Err(ConstError::Unbound { name: missing, .. }) if failed.contains(&missing) => {
                failed.insert(name);
            }
            Err(error) => {
                failed.insert(name.C());
                failures.push(ConstFailure { name, init, error });
            }
        }
    }
    (consts, failures)
}

/// Evaluate a comptime function's consts for one instantiation.
///
/// A const naming a const parameter has a value per instantiation rather than
/// one, so evaluating the body leaves it alone: there is nothing to evaluate
/// while the parameter is still a parameter. Here there is. Seeding the
/// parameters with what this instantiation passes makes every const in the
/// body evaluable by the same CTFE that evaluates every other const, which is
/// what keeps `const` meaning the same thing inside a comptime function as
/// outside one. Nothing is deferred, so a const that still cannot be evaluated
/// is an error.
///
/// Returns the values under their local names, which is how the copy's
/// `const_values` records them, and the consts that could not be evaluated.
pub fn evaluate_instantiation_consts<'db>(
    env: &mut ConstEvalEnv<'_, 'db>,
    func_stmt: &'db StmtFun<'db>,
    mut scope: LookupMap<String, (IrType, Arc<ConstValue>)>,
    comptime_param_indices: &[usize],
    values: &[ConstValue],
) -> (LookupMap<String, Arc<ConstValue>>, Vec<ConstFailure<'db>>) {
    let db = env.db;
    let params = func_stmt.params();
    for (&param_idx, value) in comptime_param_indices.iter().zip(values.iter()) {
        scope.insert(
            params[param_idx].name.text(db).S(),
            (datalove_datafun_ir::ir_type_of_const_value(value), Arc::new(value.C())),
        );
    }
    let (consts, failures) = evaluate_body_consts(env, func_stmt, scope, BTreeSet::new(), BTreeSet::new());
    (consts.into_iter().map(|(name, _, value)| (name, value)).collect(), failures)
}
