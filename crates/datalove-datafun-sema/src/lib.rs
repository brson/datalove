//! Semantic analysis types shared between ownership analysis and lowering.
//!
//! This crate provides core types used by both:
//! - `datalove-datafun-ownership`: Ownership and liveness analysis
//! - Lowering (in `datalove-datafun-compiler`): IR generation
//!
//! By extracting these types, lowering can eventually be moved to its own crate
//! without depending on the full ownership analysis module.

use rmx::std::collections::BTreeMap;
use bct::module_graph::ModuleId;
use bct::text::InternedText;
use datalove_datafun_ast::ast::{Binding, ExprArena, ExprFun, ExprFunctionCall, FunHead, Statement, ParamMode, ExprKey};
use datalove_datafun_ir::IrType;

/// Types of expressions, by expression.
///
/// This used to be a `Vec` indexed by the raw salsa id, which sized it by
/// however many ids the database had handed out rather than by how many
/// expressions there are, and then a `BTreeMap` keyed by `ExprKey`, whose
/// inserts and lookups were the largest single item in typechecking and a
/// percent of lowering. An expression's key is now its body and its place in
/// that body's arena, so the table is one dense vector per body.
pub type ExprTypes<'db> = BodyTable<'db, datalove_datafun_common::Type<'db>>;

/// Resolved call targets, by call.
///
/// Calls are numbered separately from other expressions, so a key from one
/// of these tables means nothing against the other.
pub type CallTargets<'db> = BodyTable<'db, ResolvedCallTarget<'db>>;

/// A value for some of the expressions, or calls, of a module or a script unit,
/// in a vector per body indexed by `local_index`.
///
/// The bodies are indexed by `ExprArena::body`, so reaching a value is two
/// indexes. Every expression in one table belongs to the same module, and the
/// layout depends only on what was inserted, so two tables with the same
/// contents compare equal.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct BodyTable<'db, T> {
    /// The module every expression belongs to, once one has been inserted.
    module_id: Option<Option<ModuleId<'db>>>,
    bodies: Vec<Vec<Option<T>>>,
}

impl<'db, T> Default for BodyTable<'db, T> {
    fn default() -> Self {
        BodyTable { module_id: None, bodies: Vec::new() }
    }
}

impl<'db, T> BodyTable<'db, T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// The value for `expr`.
    pub fn get(&self, expr: ExprFun<'db>) -> Option<&T> {
        self.get_at(expr.arena(), expr.local_index())
    }

    pub fn insert(&mut self, expr: ExprFun<'db>, value: T) {
        let exprs = expr.arena().exprs.len();
        self.insert_at(expr.arena(), expr.local_index(), exprs, value)
    }

    /// The value for `call`, which `expr` is; calls are numbered apart from
    /// expressions.
    pub fn get_call(&self, expr: ExprFun<'db>, call: &ExprFunctionCall<'db>) -> Option<&T> {
        self.get_at(expr.arena(), call.local_index)
    }

    pub fn insert_call(&mut self, expr: ExprFun<'db>, call: &ExprFunctionCall<'db>, value: T) {
        self.insert_at(expr.arena(), call.local_index, 0, value)
    }

    fn get_at(&self, arena: &ExprArena<'db>, index: u32) -> Option<&T> {
        debug_assert!(
            self.module_id.is_none_or(|m| m == arena.module_id),
            "one table holds one module's expressions",
        );
        self.bodies.get(arena.body as usize)?.get(index as usize)?.as_ref()
    }

    /// `slots` is how many the body will have, if known: the arena's length
    /// for its expressions.
    fn insert_at(&mut self, arena: &ExprArena<'db>, index: u32, slots: usize, value: T) {
        let module_id = *self.module_id.get_or_insert(arena.module_id);
        assert_eq!(module_id, arena.module_id, "one table holds one module's expressions");
        let body = arena.body as usize;
        if self.bodies.len() <= body {
            self.bodies.resize_with(body + 1, Vec::new);
        }
        let body = &mut self.bodies[body];
        let index = index as usize;
        if body.len() <= index {
            body.resize_with(slots.max(index + 1), || None);
        }
        body[index] = Some(value);
    }

    /// How many expressions have a value.
    pub fn len(&self) -> usize {
        self.bodies.iter().map(|body| body.iter().filter(|v| v.is_some()).count()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ============================================================================
// Resolved call target
// ============================================================================

/// Resolved call target from typechecking.
///
/// What a call resolved to, so nothing after typechecking looks a name up
/// again. A plain value in the call-target table: it used to be a salsa
/// tracked struct, minted per call with its identity hashed, though nothing
/// ever needed its identity.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ResolvedCallTarget<'db> {
    /// What the call site knows of the function it resolved to.
    pub func: FunHead<'db>,
    /// The source module (None for script-local functions).
    pub module_id: Option<ModuleId<'db>>,
    /// What this call site bound the callee's type parameters to, in the
    /// callee's declaration order.
    ///
    /// Empty for a callee with no type parameters. This is the only place the
    /// answer exists: the call site worked it out from the arguments and what
    /// it expected back, and nothing downstream can work it out again.
    pub type_args: Vec<datalove_datalit::tycheck::Type<'db>>,
}

impl<'db> ResolvedCallTarget<'db> {
    pub fn func(&self) -> &FunHead<'db> {
        &self.func
    }

    pub fn module_id(&self) -> Option<ModuleId<'db>> {
        self.module_id
    }

    pub fn type_args(&self) -> &Vec<datalove_datalit::tycheck::Type<'db>> {
        &self.type_args
    }
}

// ============================================================================
// Destructuring
// ============================================================================

/// How a `let` or `var` takes its value apart.
///
/// Ownership analysis and lowering both read this, and they have to allocate
/// the bindings in one order: the order the pattern names them, which is the
/// order `names` lists them in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destructure<'db> {
    /// The whole value, under one name.
    Whole(InternedText<'db>),
    /// Every field of a tuple or struct. Each name is listed with the index of
    /// its field and the field's type; `field_count` is how many there are,
    /// which a pattern naming every field makes the same as `names.len()`.
    Fields {
        names: Vec<(InternedText<'db>, u32, IrType)>,
        field_count: usize,
    },
    /// The payload of a term.
    Payload(InternedText<'db>, IrType),
    /// An atom or `()`, which binds nothing.
    Nothing,
}

impl<'db> Destructure<'db> {
    /// Work out how `binding` takes apart a value of type `ty`.
    ///
    /// The typechecker has said the two fit.
    pub fn of(db: &'db dyn salsa::Database, binding: &Binding<'db>, ty: &IrType) -> Self {
        match (binding, ty) {
            (Binding::Name(name), _) => Destructure::Whole(*name),
            (Binding::Tuple(names), IrType::Unit) if names.is_empty() => Destructure::Nothing,
            (Binding::Tuple(names), IrType::Tuple(fields)) => Destructure::Fields {
                names: names.iter().zip(fields)
                    .enumerate()
                    .map(|(i, (name, ty))| (*name, i as u32, ty.clone()))
                    .collect(),
                field_count: fields.len(),
            },
            (Binding::Struct(bindings), IrType::Struct(fields)) => Destructure::Fields {
                names: bindings.iter()
                    .map(|b| {
                        let field = b.field.as_str(db);
                        let index = fields.iter().position(|(name, _)| name == field)
                            .unwrap_or_else(|| panic!("field `{field}` not in {ty:?}"));
                        (b.binding, index as u32, fields[index].1.clone())
                    })
                    .collect(),
                field_count: fields.len(),
            },
            (Binding::Atom(_), IrType::Atom(_)) => Destructure::Nothing,
            (Binding::Term { binding, .. }, IrType::Term(_, payload)) => {
                Destructure::Payload(*binding, (**payload).clone())
            }
            _ => panic!("pattern does not fit {ty:?}; the typechecker should have caught it"),
        }
    }
}

// ============================================================================
// Statement identity for debug verification
// ============================================================================

/// Key identifying a statement for debug verification.
///
/// Used to verify that lowering processes statements in the same order as
/// ownership analysis. This catches bugs where statement IDs get out of sync.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StmtKey {
    /// Discriminant of the Statement enum.
    pub kind: u8,
    /// local_index if the statement type has one, for additional discrimination.
    pub local_index: Option<u32>,
}

impl StmtKey {
    /// Extract a key from a statement.
    pub fn from_stmt(stmt: &Statement<'_>) -> Self {
        let (kind, local_index) = match stmt {
            Statement::Let(_) => (0, None),
            Statement::Var(_) => (1, None),
            Statement::Set(s) => (2, Some(s.local_index)),
            Statement::Fun(s) => (3, Some(s.local_index())),
            Statement::Ret(s) => (4, Some(s.local_index)),
            Statement::Require(_) => (5, None),
            Statement::Import(_) => (6, None),
            Statement::If(_) => (7, None),
            Statement::Loop(_) => (8, None),
            Statement::Break(s) => (9, Some(s.local_index)),
            Statement::Continue(s) => (10, Some(s.local_index)),
            Statement::DebugLog(_) => (11, None),
            Statement::TypeAlias(s) => (12, Some(s.local_index)),
            Statement::ParseError(_) => (13, None),
            Statement::Const(_) => (14, None),
            Statement::Match(_) => (15, None),
            Statement::NativeFun(_) => (16, None),
            Statement::ExprStatement(_) => (17, None),
            Statement::For(s) => (18, Some(s.iteratee.0)),
        };
        Self { kind, local_index }
    }

    /// Get a human-readable name for the statement kind.
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            0 => "Let",
            1 => "Var",
            2 => "Set",
            3 => "Fun",
            4 => "Ret",
            5 => "Require",
            6 => "Import",
            7 => "If",
            8 => "Loop",
            9 => "Break",
            10 => "Continue",
            11 => "DebugLog",
            12 => "TypeAlias",
            13 => "ParseError",
            14 => "Const",
            15 => "Match",
            16 => "NativeFun",
            17 => "ExprStatement",
            18 => "For",
            _ => "Unknown",
        }
    }
}

// ============================================================================
// Core types
// ============================================================================

/// Identifies a binding (parameter or let/var).
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, PartialOrd, Ord)]
pub struct BindingId(pub u32);

/// A value for some of a body's bindings, by `BindingId`.
///
/// Binding ids are numbered from zero in each body, so this is a vector
/// indexed by them. Iteration is in id order, as a `BTreeMap`'s was, which is
/// the order drops are emitted in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingMap<T> {
    slots: Vec<Option<T>>,
    len: usize,
}

impl<T> Default for BindingMap<T> {
    fn default() -> Self {
        BindingMap { slots: Vec::new(), len: 0 }
    }
}

impl<T> BindingMap<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: &BindingId) -> Option<&T> {
        self.slots.get(id.0 as usize)?.as_ref()
    }

    pub fn get_mut(&mut self, id: &BindingId) -> Option<&mut T> {
        self.slots.get_mut(id.0 as usize)?.as_mut()
    }

    pub fn contains_key(&self, id: &BindingId) -> bool {
        self.get(id).is_some()
    }

    pub fn insert(&mut self, id: BindingId, value: T) -> Option<T> {
        let index = id.0 as usize;
        if self.slots.len() <= index {
            self.slots.resize_with(index + 1, || None);
        }
        let old = self.slots[index].replace(value);
        if old.is_none() {
            self.len += 1;
        }
        old
    }

    pub fn remove(&mut self, id: &BindingId) -> Option<T> {
        let old = self.slots.get_mut(id.0 as usize)?.take();
        if old.is_some() {
            self.len -= 1;
        }
        old
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn clear(&mut self) {
        self.slots.clear();
        self.len = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The entries, in id order.
    pub fn iter(&self) -> impl Iterator<Item = (BindingId, &T)> + '_ {
        self.slots.iter().enumerate()
            .filter_map(|(i, v)| v.as_ref().map(|v| (BindingId(i as u32), v)))
    }

    pub fn keys(&self) -> impl Iterator<Item = BindingId> + '_ {
        self.iter().map(|(id, _)| id)
    }

    pub fn values(&self) -> impl Iterator<Item = &T> + '_ {
        self.slots.iter().filter_map(Option::as_ref)
    }
}

impl<T> Extend<(BindingId, T)> for BindingMap<T> {
    fn extend<I: IntoIterator<Item = (BindingId, T)>>(&mut self, iter: I) {
        for (id, value) in iter {
            self.insert(id, value);
        }
    }
}

impl<T> IntoIterator for BindingMap<T> {
    type Item = (BindingId, T);
    type IntoIter = std::iter::FilterMap<
        std::iter::Enumerate<std::vec::IntoIter<Option<T>>>,
        fn((usize, Option<T>)) -> Option<(BindingId, T)>,
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.slots.into_iter().enumerate()
            .filter_map(|(i, v)| v.map(|v| (BindingId(i as u32), v)))
    }
}

impl<'a, T> IntoIterator for &'a BindingMap<T> {
    type Item = (BindingId, &'a T);
    type IntoIter = std::iter::FilterMap<
        std::iter::Enumerate<std::slice::Iter<'a, Option<T>>>,
        fn((usize, &'a Option<T>)) -> Option<(BindingId, &'a T)>,
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.slots.iter().enumerate()
            .filter_map(|(i, v)| v.as_ref().map(|v| (BindingId(i as u32), v)))
    }
}

impl<T> FromIterator<(BindingId, T)> for BindingMap<T> {
    fn from_iter<I: IntoIterator<Item = (BindingId, T)>>(iter: I) -> Self {
        let mut map = BindingMap::new();
        map.extend(iter);
        map
    }
}

/// Tracking category for a binding.
///
/// Determines whether the binding needs runtime tracking for moves/drops.
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub enum TrackingCategory {
    /// Copy type - no tracking needed, no drops.
    Copy,
    /// Precise ownership - state is statically known at every program point.
    /// Uses precise move/drop instructions (no runtime checks).
    Precise,
    /// Tracked ownership - state may vary at runtime.
    /// Uses tracked move/drop instructions (with runtime checks).
    /// Examples: exports, out params, conditional moves, mutable slots.
    Tracked,
}

/// Information about a binding.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct BindingInfo {
    /// Name of the binding (for debugging).
    pub name: String,
    /// Type of the binding.
    pub ty: IrType,
    /// Whether this binding is a slot (var) vs value (let/param).
    pub is_slot: bool,
    /// Whether this binding is a ScriptUnit top-level binding (exported, never dropped).
    pub is_script_unit: bool,
    /// Parameter mode if this binding is a param (None for let/var).
    pub param_mode: Option<ParamMode>,
    /// Whether this binding is a const.
    ///
    /// A const is borrowed wherever it is named: it may be read and borrowed
    /// any number of times, and moving out of it needs a clone.
    pub is_const: bool,
}

impl BindingInfo {
    /// True if this binding is borrowed (Ref, Mut, or Out param) and cannot transfer ownership.
    ///
    /// Out params are borrowed because the caller owns the slot and retains ownership
    /// after the call returns.
    pub fn is_borrowed(&self) -> bool {
        matches!(self.param_mode, Some(ParamMode::Ref) | Some(ParamMode::Mut) | Some(ParamMode::Out))
    }
}

/// A binding an earlier script unit made, which the unit being analyzed names.
///
/// The analysis gives each one a binding of its own, ahead of the unit's, so
/// that moving out of it is checked as moving out of any binding is.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ExternalBinding {
    pub name: String,
    pub ty: IrType,
    pub is_slot: bool,
    /// Whether an earlier unit already gave its value away.
    pub dead: bool,
}

// ============================================================================
// Error types
// ============================================================================

/// Hint for how to recover from an @-recoverable ownership error.
///
/// When an ownership error can be fixed by inserting the `@` (adapt) operator
/// to clone the value, a recovery hint is attached to the diagnostic.
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum OwnershipRecoveryHint {
    /// Insert @ before the expression to clone the value.
    InsertAdapt {
        /// Human-readable description of what @ does here.
        description: String,
    },
    /// No automatic recovery available.
    #[default]
    None,
}

/// Error detected during ownership analysis.
///
/// Each variant includes an `expr_key` for span lookup during diagnostic
/// emission, the expression's stable key within its function, used to look
/// up spans in DatafunSpans.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum AnalysisError<'db> {
    /// Using a binding an earlier unit gave its value away.
    /// D013 - Recoverable with @ (clone at the use that gave it away)
    UseAfterMoveInEarlierUnit {
        /// Location of the use in this unit.
        expr_key: ExprKey<'db>,
        name: String,
        recovery_hint: OwnershipRecoveryHint,
    },
    /// Using a value after it was moved.
    /// D001 - Recoverable with @ (clone before first use)
    UseAfterMove {
        /// Location of the second use (where error is detected).
        expr_key: ExprKey<'db>,
        /// Location of the first move (where @ should be inserted).
        moved_at: ExprKey<'db>,
        name: String,
        recovery_hint: OwnershipRecoveryHint,
    },
    /// Moving a value multiple times.
    /// D002 - Recoverable with @ (clone before second move)
    DoubleMove {
        /// Location of the second move (where error is detected).
        expr_key: ExprKey<'db>,
        /// Location of the first move (where @ should be inserted).
        moved_at: ExprKey<'db>,
        name: String,
        recovery_hint: OwnershipRecoveryHint,
    },
    /// Attempting to move a borrowed value (ref/mut/out parameter).
    /// D003
    CannotMoveBorrowed {
        expr_key: ExprKey<'db>,
        name: String,
    },
    /// Attempting to move out of a const, which is borrowed wherever it is
    /// named. Also D003.
    CannotMoveConst {
        expr_key: ExprKey<'db>,
        name: String,
        recovery_hint: OwnershipRecoveryHint,
    },
    /// Changing a collection while a `for` walks it: moving it, assigning to
    /// it or through it, or passing it `mut` or `out`. The walk holds
    /// references into it.
    /// D015
    WalkedCollectionChanged {
        expr_key: ExprKey<'db>,
        name: String,
    },
    /// The same, by a `set` to the collection or through it, reported at its
    /// target: the `set` statement's span index.
    /// D015
    WalkedCollectionAssigned {
        set_index: u32,
        name: String,
    },
    /// Attempting to pass a ref param to a mut param.
    /// D004
    CannotMutFromRef {
        expr_key: ExprKey<'db>,
        name: String,
    },
    /// Reading binding before it was initialized.
    /// D005. Applies to Out params and uninitialized var bindings.
    ReadUninitialized {
        expr_key: ExprKey<'db>,
        name: String,
    },
    /// Function returns without initializing Out param.
    /// D006
    OutParamNotInitialized {
        /// Statement index of return, or None for implicit return.
        ret_stmt_idx: Option<usize>,
        name: String,
    },
    /// Moving an outer-scoped value inside a loop body.
    /// D007 - Recoverable with @ (clone before entering loop)
    MoveInLoop {
        /// Expression where the move occurred.
        expr_key: ExprKey<'db>,
        name: String,
        recovery_hint: OwnershipRecoveryHint,
    },
    /// Branches meeting after an `if` or `match` disagree about whether a
    /// binding is still held.
    ///
    /// Either one gave the binding away and another did not, or, where it was
    /// already given away, one gave it a value again and another did not.
    /// D008
    InconsistentBranchMove {
        /// Statement index of the `if` or `match` statement.
        stmt_idx: usize,
        /// The `if` condition or `match` input.
        at: ExprKey<'db>,
        name: String,
        /// Whether the branch that differs gave the binding away, rather than
        /// giving it a value again.
        gave_away: bool,
        /// The branch that differs, e.g. "in the then branch".
        changed_in: String,
        /// A branch that does not, e.g. "in the else branch".
        unchanged_in: String,
        /// Where to give or move the binding to make the branches agree.
        fix_in: String,
        /// Where the differing branch moved or assigned the binding.
        changed_at: Option<ExprKey<'db>>,
        /// Where the binding was given away before the branches, when one of
        /// them gave it a value again.
        moved_before: Option<ExprKey<'db>>,
    },
    /// Value moved on one way out of a loop and not another.
    ///
    /// Distinct from [`AnalysisError::MoveInLoop`]: the move does not repeat, so
    /// nothing is moved twice. What cannot be said is whether the binding is
    /// still held after the loop, since that depends on which exit ran, and a
    /// drop point has to be decided without knowing.
    ///
    /// D014
    InconsistentLoopExit {
        /// Statement index of the loop statement.
        stmt_idx: usize,
        name: String,
    },
    /// Partial field write to out param (must write whole value).
    /// D009
    OutParamPartialWrite {
        expr_key: ExprKey<'db>,
        name: String,
    },
    /// Two arguments of one call name the same binding, and at least one of
    /// them is passed to a `mut` or `out` parameter.
    /// D010
    AliasedMutableArgument {
        /// Location of the later of the two arguments.
        expr_key: ExprKey<'db>,
        name: String,
    },
    /// Passing an immutable binding to a `mut` or `out` parameter.
    /// D011. Applies to `let` bindings and `in` parameters; a `ref` parameter
    /// reports D004 instead.
    CannotMutateImmutable {
        expr_key: ExprKey<'db>,
        name: String,
    },
    /// Passing a value that is not a place to a `mut` or `out` parameter.
    /// D012. The callee's writes would land in a temporary and be discarded.
    CannotMutateTemporary {
        expr_key: ExprKey<'db>,
    },
}

/// The message for a D008 [`AnalysisError::InconsistentBranchMove`].
pub fn inconsistent_branch_message(error: &AnalysisError) -> Option<String> {
    let AnalysisError::InconsistentBranchMove { name, gave_away, changed_in, unchanged_in, .. } = error else {
        return None;
    };
    let what = match gave_away {
        true => "given away",
        false => "given a new value",
    };
    Some(format!("`{name}` is {what} {changed_in} but not {unchanged_in}"))
}

/// Format analysis errors for display.
pub fn format_analysis_errors(errors: &[AnalysisError]) -> String {
    errors.iter()
        .map(|e| format_single_error(e))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_single_error(error: &AnalysisError) -> String {
    match error {
        AnalysisError::UseAfterMoveInEarlierUnit { expr_key: _, name, recovery_hint } => {
            let base = format!("error[D013]: `{}` was given away by an earlier input", name);
            format_with_hint(base, recovery_hint)
        }
        AnalysisError::UseAfterMove { expr_key: _, moved_at: _, name, recovery_hint } => {
            let base = format!("error[D001]: use of moved value: `{}`", name);
            format_with_hint(base, recovery_hint)
        }
        AnalysisError::DoubleMove { expr_key: _, moved_at: _, name, recovery_hint } => {
            let base = format!("error[D002]: value moved twice: `{}`", name);
            format_with_hint(base, recovery_hint)
        }
        AnalysisError::CannotMoveBorrowed { expr_key: _, name } => {
            format!("error[D003]: cannot move borrowed value: `{}`", name)
        }
        AnalysisError::CannotMoveConst { expr_key: _, name, recovery_hint } => {
            let base = format!("error[D003]: cannot move out of const: `{}`", name);
            format_with_hint(base, recovery_hint)
        }
        AnalysisError::CannotMutFromRef { expr_key: _, name } => {
            format!("error[D004]: cannot get mutable reference from immutable: `{}`", name)
        }
        AnalysisError::WalkedCollectionChanged { expr_key: _, name }
        | AnalysisError::WalkedCollectionAssigned { set_index: _, name } => {
            format!("error[D015]: cannot change `{}` while a `for` walks it", name)
        }
        AnalysisError::ReadUninitialized { expr_key: _, name } => {
            format!("error[D005]: read of uninitialized binding: `{}`", name)
        }
        AnalysisError::OutParamNotInitialized { ret_stmt_idx: _, name } => {
            format!("error[D006]: out parameter not initialized: `{}`", name)
        }
        AnalysisError::MoveInLoop { expr_key: _, name, recovery_hint } => {
            let base = format!("error[D007]: cannot move `{}` in loop", name);
            format_with_hint(base, recovery_hint)
        }
        AnalysisError::InconsistentBranchMove { .. } => {
            format!("error[D008]: {}", inconsistent_branch_message(error).expect("is D008"))
        }
        AnalysisError::InconsistentLoopExit { stmt_idx: _, name } => {
            format!("error[D014]: `{}` is given away on one way out of this loop and not another", name)
        }
        AnalysisError::OutParamPartialWrite { expr_key: _, name } => {
            format!("error[D009]: cannot partially write to out parameter: `{}`", name)
        }
        AnalysisError::AliasedMutableArgument { expr_key: _, name } => {
            format!("error[D010]: aliased mutable argument: `{}`", name)
        }
        AnalysisError::CannotMutateImmutable { expr_key: _, name } => {
            format!("error[D011]: cannot pass immutable binding as mutable: `{}`", name)
        }
        AnalysisError::CannotMutateTemporary { expr_key: _ } => {
            "error[D012]: cannot pass a temporary as mutable".to_string()
        }
    }
}

fn format_with_hint(base: String, hint: &OwnershipRecoveryHint) -> String {
    match hint {
        OwnershipRecoveryHint::InsertAdapt { description } => {
            format!("{}\n  help: use `@` to {}", base, description)
        }
        OwnershipRecoveryHint::None => base,
    }
}

// ============================================================================
// Analysis results
// ============================================================================

/// Drop schedule computed by analysis.
///
/// Maps statement indices to bindings that need to be dropped at various
/// control flow points (branch exits, returns, loops, etc.).
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct DropSchedule {
    /// Drops to emit at the end of a then-branch before jumping to join.
    /// Key is the StmtIf index in the body.
    pub then_branch_exit: BTreeMap<usize, Vec<BindingId>>,

    /// Drops to emit at the end of an else-branch before jumping to join.
    /// Key is the StmtIf index in the body.
    pub else_branch_exit: BTreeMap<usize, Vec<BindingId>>,

    /// Drops to emit before a return statement.
    /// Key is the return statement index in the body.
    pub before_return: BTreeMap<usize, Vec<BindingId>>,

    /// Drops to emit before TryReturn in checked/optional operators.
    /// Key is the statement index containing the expression with try.
    pub before_try_return: BTreeMap<usize, Vec<BindingId>>,

    /// Bindings a statement's expression moves, each with the expression index
    /// of the place that moves it.
    ///
    /// `before_try_return` leaves them out, being what is live once the
    /// expression is done. An early return lowered before the place drops the
    /// binding too, since it still owns it there.
    pub try_return_not_yet_moved: BTreeMap<usize, Vec<(u32, BindingId)>>,

    /// Drops to emit on early return from a set-index bounds check.
    ///
    /// Computed before RHS moves are analyzed, so includes the RHS binding
    /// (which is still live when the bounds check fails).
    pub before_set_target_early_return: BTreeMap<usize, Vec<BindingId>>,

    /// Drops to emit at end of loop body before looping back.
    pub loop_body_end: BTreeMap<usize, Vec<BindingId>>,

    /// Drops to emit before break.
    pub before_break: BTreeMap<usize, Vec<BindingId>>,

    /// Drops to emit before continue.
    pub before_continue: BTreeMap<usize, Vec<BindingId>>,

    /// Drops to emit at the end of a match arm before jumping to join.
    /// Key is (match_stmt_idx, arm_idx).
    pub match_arm_exit: BTreeMap<(usize, usize), Vec<BindingId>>,

    /// Drops to emit where control runs off the end of a function body.
    ///
    /// A `ret` schedules its own drops against its statement index, but a body
    /// that simply ends has no statement to hang them on, so they go here. It
    /// is empty when every path returns explicitly, since those paths took
    /// their bindings with them.
    pub at_function_exit: Vec<BindingId>,

    /// Statement keys in allocation order, for verifying lowering traversal.
    /// Only present in debug builds.
    #[cfg(debug_assertions)]
    pub stmt_order: Vec<StmtKey>,
}

/// Result of analyzing a function.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct FunctionAnalysis<'db> {
    /// Errors detected during analysis.
    pub errors: Vec<AnalysisError<'db>>,
    /// Computed drop schedule.
    pub schedule: DropSchedule,
    /// Information about each binding (indexed by BindingId).
    pub bindings: Vec<BindingInfo>,
    /// Tracking category for each binding (indexed by BindingId).
    /// Determines whether precise or tracked move/drop instructions are used.
    pub tracking: Vec<TrackingCategory>,
}

/// Analysis result for script-level statements.
///
/// Similar to FunctionAnalysis but includes script-specific data like unit_end
/// drops for bindings that should be dropped when the script unit ends.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ScriptAnalysisData {
    /// Computed drop schedule.
    pub schedule: DropSchedule,
    /// Information about each binding (indexed by BindingId).
    pub bindings: Vec<BindingInfo>,
    /// Tracking category for each binding (indexed by BindingId).
    pub tracking: Vec<TrackingCategory>,
    /// Bindings to drop at unit end (for AOT cleanup).
    pub unit_end: Vec<BindingId>,
    /// Names whose value this unit gave away: its own exports, and earlier
    /// units' bindings it moved out of.
    ///
    /// Later units can still see the name, but there is nothing behind it, so
    /// using one is an error.
    pub dead_exports: Vec<String>,
    /// Names an earlier unit gave away that this unit assigned to, which have
    /// a value again.
    pub revived_exports: Vec<String>,
    /// How many of the first bindings are earlier units' bindings, which
    /// lowering makes none of.
    pub externals: u32,
}
