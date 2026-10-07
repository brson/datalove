//! Evaluating const bindings: lowering each initializer and running it.
//!
//! Consts are evaluated in three places -- a module's or a script's top level,
//! a function body, and a comptime function's body once an instantiation has
//! given its const parameters values -- by the module pipeline and by the
//! script compiler alike. Each binding is lowered to a small unit, or read
//! straight off when it is a literal or names a const already evaluated, and
//! the unit is run by the CTFE evaluator. Both pipelines do it through here.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use datalove_datafun_ast::ast::{ExprFun, ExprFunKind, ExprKey, Statement, StmtFun};
use datalove_datafun_const::evaluate_const_unit;
use datalove_datafun_ir::{
    CodeRef, ConstValue, CtfeError, CtfeEvaluator, FuncId, Instruction, IrCodeUnit, IrType,
    ModuleFunctionRegistry,
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
    pub evaluator: &'a Rc<RefCell<dyn CtfeEvaluator>>,
    /// The functions a const may call, lowered already.
    pub lowered: &'a [Arc<IrCodeUnit>],
    pub func_name_to_id: &'a HashMap<String, FuncId>,
    /// For calls into other modules.
    pub func_id_map: &'db FuncIdLookup<'db>,
    /// What the evaluator can reach, where some of it may not be lowered yet.
    ///
    /// Only the module pipeline has functions waiting on a module const, so
    /// only it can find a const depending on one: a cycle.
    pub callable: Option<&'a ModuleFunctionRegistry>,
    /// The data files a `require data` const may name, by path.
    pub data_files: &'db lower::DataFiles,
}

/// Why a const could not be evaluated.
#[derive(Debug)]
pub enum ConstError {
    MissingType,
    Lowering(LowerError),
    /// It calls a function that is waiting on a module const, which is a cycle.
    Cycle(String),
    Ctfe(CtfeError),
}

impl fmt::Display for ConstError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConstError::MissingType => write!(f, "missing type information"),
            ConstError::Lowering(e) => write!(f, "lowering error: {}", e),
            ConstError::Cycle(missing) => write!(
                f,
                "depends on a function that is not available yet, which means it and \
                 that function depend on each other: {}",
                missing,
            ),
            ConstError::Ctfe(e) => write!(f, "CTFE error: {}", e),
        }
    }
}

/// A const that could not be evaluated, by name.
#[derive(Debug)]
pub struct NamedConstError {
    pub name: String,
    pub error: ConstError,
}

impl fmt::Display for NamedConstError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "const '{}': {}", self.name, self.error)
    }
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
    statements: &[Statement<'db>],
    expr_types: &ExprTypes<'db>,
    data_files: &lower::DataFiles,
) -> HashMap<String, (IrType, Arc<ConstValue>)> {
    statements.iter()
        .filter_map(|statement| {
            let Statement::Const(binding) = statement else { return None };
            if !matches!(binding.value.expr(db), ExprFunKind::DataFile(_)) {
                return None;
            }
            let ty = expr_types.get(&ExprKey::of(db, binding.value))?;
            let ty = IrType::from_tycheck(db, ty);
            let value = lower::try_extract_literal(db, binding.value, &ty, data_files)
                .expect("a data file is a literal");
            Some((binding.name.text(db).to_owned(), (ty, value)))
        })
        .collect()
}

/// The type the typechecker gave a const's initializer.
pub fn const_type(env: &ConstEvalEnv<'_, '_>, init: ExprFun<'_>) -> Result<IrType, ConstError> {
    env.expr_types.get(&ExprKey::of(env.db, init))
        .map(|ty| IrType::from_tycheck(env.db, ty))
        .ok_or(ConstError::MissingType)
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
    env: &ConstEvalEnv<'_, 'db>,
    init: ExprFun<'db>,
    ty: &IrType,
    resolved: &HashMap<String, (IrType, Arc<ConstValue>)>,
    func_return_type: Option<IrType>,
    deferred: &BTreeSet<String>,
) -> Result<Option<Arc<ConstValue>>, ConstError> {
    let lowered = lower::lower_const_binding(
        env.db, init, ty, env.expr_types, env.call_targets, resolved, func_return_type,
        env.lowered, env.func_name_to_id, Some(env.func_id_map), env.data_files,
    );
    let (unit, value) = match lowered {
        Ok(pair) => pair,
        Err(LowerError::BindingNotAvailable(ref missing)) if deferred.contains(missing) => {
            return Ok(None);
        }
        Err(e) => return Err(ConstError::Lowering(e)),
    };
    match (unit, value) {
        (None, Some(value)) => Ok(Some(value)),
        (Some(unit), None) => {
            // Every function this reaches has to be lowered already, or the
            // interpreter has nothing to call and panics looking for it.
            if let Some(callable) = env.callable {
                if let Some(missing) = first_uncallable_target(&unit, callable) {
                    return Err(ConstError::Cycle(missing));
                }
            }
            evaluate_const_unit(&unit, ty, env.evaluator)
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
pub fn evaluate_body_consts<'db>(
    env: &ConstEvalEnv<'_, 'db>,
    func_stmt: &StmtFun<'db>,
    mut scope: HashMap<String, (IrType, Arc<ConstValue>)>,
    mut deferred: BTreeSet<String>,
) -> (Vec<(String, IrType, Arc<ConstValue>)>, Vec<NamedConstError>) {
    let db = env.db;
    let func_return_type = func_stmt.return_type(db).map(|ty| IrType::from_type_hint(db, &ty));
    let mut consts = Vec::new();
    let mut errors = Vec::new();
    for stmt in func_stmt.body(db).iter() {
        let Statement::Const(const_stmt) = stmt else { continue };
        let name = const_stmt.name.text(db).to_string();
        let evaluated = const_type(env, const_stmt.value).and_then(|ty| {
            evaluate_const(env, const_stmt.value, &ty, &scope, func_return_type.clone(), &deferred)
                .map(|value| value.map(|value| (ty, value)))
        });
        match evaluated {
            Ok(Some((ty, value))) => {
                scope.insert(name.clone(), (ty.clone(), value.clone()));
                consts.push((name, ty, value));
            }
            Ok(None) => {
                deferred.insert(name);
            }
            Err(error) => errors.push(NamedConstError { name, error }),
        }
    }
    (consts, errors)
}

/// The first module function a const's unit calls that is not lowered yet.
///
/// Such a function is waiting on a module const, which is where a cycle
/// between a const and a function shows up. Local and external references are
/// resolved against units the caller already holds, so they cannot be missing.
fn first_uncallable_target(unit: &IrCodeUnit, callable: &ModuleFunctionRegistry) -> Option<String> {
    for block in &unit.blocks {
        for instr in &block.instructions {
            let func = match instr {
                Instruction::Call { func, .. } | Instruction::ComptimeCall { func, .. } => func,
                _ => continue,
            };
            if let CodeRef::Module { module, id } = func {
                if callable.get_module_function_as_unit(*module, *id).is_none() {
                    return Some(format!("module function #{}", id.0));
                }
            }
        }
    }
    None
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
/// `const_values` records them, and the errors, each naming the function.
pub fn evaluate_instantiation_consts<'db>(
    env: &ConstEvalEnv<'_, 'db>,
    func_stmt: &StmtFun<'db>,
    mut scope: HashMap<String, (IrType, Arc<ConstValue>)>,
    comptime_param_indices: &[usize],
    values: &[ConstValue],
) -> (HashMap<String, Arc<ConstValue>>, Vec<String>) {
    let db = env.db;
    let params = func_stmt.params(db);
    for (&param_idx, value) in comptime_param_indices.iter().zip(values.iter()) {
        scope.insert(
            params[param_idx].name.text(db).to_string(),
            (datalove_datafun_ir::ir_type_of_const_value(value), Arc::new(value.clone())),
        );
    }
    let (consts, errors) = evaluate_body_consts(env, func_stmt, scope, BTreeSet::new());
    let func_name = func_stmt.name(db).text(db);
    (
        consts.into_iter().map(|(name, _, value)| (name, value)).collect(),
        errors.iter().map(|e| format!("{}::{}", func_name, e)).collect(),
    )
}
