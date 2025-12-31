//! Scope tracking for drop insertion.
//!
//! Tracks bindings created in each scope to emit proper Drop instructions
//! when scopes exit (function return, break, continue, etc.).

use super::super::{IrType, Operand};

/// Check if an IR type has copy semantics.
///
/// Copy types can be bitwise copied without ownership tracking.
/// Non-copy types require explicit drops.
pub fn is_copy_type(ty: &IrType) -> bool {
    match ty {
        // Scalar primitives are always copy.
        IrType::Unit | IrType::Bool => true,
        IrType::U8 | IrType::U16 | IrType::U32 | IrType::U64 => true,
        IrType::I8 | IrType::I16 | IrType::I32 | IrType::I64 => true,
        IrType::F32 => true,
        // Heap-allocated types are never copy.
        IrType::Int | IrType::String | IrType::Data | IrType::Error => false,
        IrType::List(_) | IrType::Set(_) | IrType::Map(_, _) => false,
        // Option is copy only if inner is copy.
        IrType::Option(inner) => is_copy_type(inner),
        // Result is never copy (conservative).
        IrType::Result(_) => false,
        // Tuple is copy only if all fields are copy.
        IrType::Tuple(fields) => fields.iter().all(is_copy_type),
        // Struct is copy only if all fields are copy.
        IrType::Struct(fields) => fields.iter().all(|(_, ty)| is_copy_type(ty)),
    }
}

/// What kind of scope we're tracking.
#[derive(Clone, Debug)]
pub enum ScopeKind {
    /// Function body scope.
    Function,
    /// Script unit top-level scope (values are exported, not dropped at unit end).
    ScriptUnit,
    /// Loop body scope.
    Loop,
    /// If-then branch scope.
    IfThen,
    /// If-else branch scope.
    IfElse,
}

/// A binding tracked for drop purposes.
#[derive(Clone, Debug)]
pub struct TrackedBinding {
    /// The operand (Value or Slot).
    pub operand: Operand,
    /// The type (for determining if drop is needed).
    pub ty: IrType,
    /// Whether this binding has been moved/consumed.
    pub moved: bool,
}

/// A scope for tracking drops.
#[derive(Clone, Debug)]
pub struct Scope {
    pub kind: ScopeKind,
    /// Bindings created in this scope that may need dropping.
    pub bindings: Vec<TrackedBinding>,
}

impl Scope {
    pub fn new(kind: ScopeKind) -> Self {
        Self {
            kind,
            bindings: Vec::new(),
        }
    }
}

/// Scope tracker for emitting drops at scope exits.
#[derive(Clone, Debug, Default)]
pub struct ScopeTracker {
    pub scopes: Vec<Scope>,
}

impl ScopeTracker {
    pub fn new() -> Self {
        Self { scopes: Vec::new() }
    }

    /// Enter a new scope.
    pub fn enter_scope(&mut self, kind: ScopeKind) {
        self.scopes.push(Scope::new(kind));
    }

    /// Record a binding in the current scope.
    pub fn record_binding(&mut self, operand: Operand, ty: IrType) {
        if let Some(scope) = self.scopes.last_mut() {
            // Only track non-copy types.
            if !is_copy_type(&ty) {
                scope.bindings.push(TrackedBinding {
                    operand,
                    ty,
                    moved: false,
                });
            }
        }
    }

    /// Mark an operand as moved (won't be dropped).
    pub fn mark_moved(&mut self, operand: &Operand) {
        // Search all scopes from innermost to outermost.
        for scope in self.scopes.iter_mut().rev() {
            for binding in &mut scope.bindings {
                if &binding.operand == operand {
                    binding.moved = true;
                    return;
                }
            }
        }
    }

    /// Get the bindings that need dropping when exiting the current scope.
    pub fn bindings_to_drop(&self) -> Vec<Operand> {
        if let Some(scope) = self.scopes.last() {
            // Don't drop script unit top-level bindings (they're exported).
            if matches!(scope.kind, ScopeKind::ScriptUnit) {
                return Vec::new();
            }
            scope.bindings.iter()
                .filter(|b| !b.moved)
                .map(|b| b.operand)
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Exit the current scope, returning bindings that need dropping.
    pub fn exit_scope(&mut self) -> Vec<Operand> {
        let drops = self.bindings_to_drop();
        self.scopes.pop();
        drops
    }

    /// Get bindings to drop for break (all scopes up to and including the loop).
    pub fn bindings_to_drop_for_break(&self) -> Vec<Operand> {
        let mut drops = Vec::new();
        for scope in self.scopes.iter().rev() {
            // Collect bindings from this scope.
            for binding in &scope.bindings {
                if !binding.moved && !is_copy_type(&binding.ty) {
                    drops.push(binding.operand);
                }
            }
            // Stop when we hit a loop scope.
            if matches!(scope.kind, ScopeKind::Loop) {
                break;
            }
        }
        drops
    }

    /// Get bindings to drop for continue (only the current loop iteration).
    pub fn bindings_to_drop_for_continue(&self) -> Vec<Operand> {
        let mut drops = Vec::new();
        for scope in self.scopes.iter().rev() {
            // Collect bindings from this scope.
            for binding in &scope.bindings {
                if !binding.moved && !is_copy_type(&binding.ty) {
                    drops.push(binding.operand);
                }
            }
            // Stop when we hit a loop scope (include it, then stop).
            if matches!(scope.kind, ScopeKind::Loop) {
                break;
            }
        }
        drops
    }

    /// Get all bindings to drop for a function return.
    ///
    /// Returns bindings from all scopes up to and including the function scope.
    pub fn bindings_to_drop_for_return(&self) -> Vec<Operand> {
        let mut drops = Vec::new();
        for scope in self.scopes.iter().rev() {
            // Collect bindings from this scope.
            for binding in &scope.bindings {
                if !binding.moved && !is_copy_type(&binding.ty) {
                    drops.push(binding.operand);
                }
            }
            // Stop when we hit a function scope.
            if matches!(scope.kind, ScopeKind::Function) {
                break;
            }
        }
        drops
    }
}
