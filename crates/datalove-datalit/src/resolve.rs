use rmx::prelude::*;
use bct::text::InternedText;
use std::collections::HashMap;

use crate::ast::*;
use datalove_diagnostic::ByteSpan;

/// Unique identifier for a binding site.
#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct BindingId(pub u32);

/// Resolved reference pointing to a specific type hint definition.
#[salsa::tracked]
pub struct Resolution<'db> {
    /// The binding this reference resolves to.
    pub binding_id: BindingId,

    /// Scope depth where binding was found (0 = outermost).
    pub scope_depth: u32,

    /// Original type hint definition site.
    pub definition: TypeHintAndHeap<'db>,
}

/// Resolution error for a single name.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub enum ResolutionError {
    /// Name was not found in any scope.
    UnboundName,
}

/// A single resolution entry.
#[salsa::tracked]
pub struct ResolutionEntry<'db> {
    pub name: InternedText<'db>,
    pub resolution: Resolution<'db>,
}

/// A single resolution error entry.
#[salsa::tracked]
pub struct ResolutionErrorEntry<'db> {
    pub name: InternedText<'db>,
    pub error: ResolutionError,
}

/// Stored span entry (no lifetimes for Salsa storage).
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct StoredSpan {
    pub expr_id: salsa::Id,
    pub text_id: salsa::Id,
    pub span: ByteSpan,
}

/// Resolution result for an expression.
#[salsa::tracked]
pub struct ResolvedExpr<'db> {
    /// Original expression.
    pub expr: ExprFull<'db>,

    /// List of successful resolutions.
    pub resolutions: Vec<ResolutionEntry<'db>>,

    /// List of resolution errors.
    pub errors: Vec<ResolutionErrorEntry<'db>>,

    /// Source for on-demand span lookup.
    pub source: bct::input::Source,
}

/// Main entry point: resolve all names in an expression.
///
/// This performs a two-pass algorithm:
/// 1. Collect all type hint definitions (struct, enum, tuple, token names)
/// 2. Resolve all expression references to those definitions
///
/// Shadowing is allowed - inner scopes shadow outer scopes.
#[salsa::tracked]
pub fn resolve_names<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
    expr: ExprFull<'db>,
) -> ResolvedExpr<'db> {
    let mut scope = Scope::new();
    let mut next_id = 0u32;
    let mut resolutions_map = HashMap::new();
    let mut errors_map = HashMap::new();

    // Pass 1: Collect type hint definitions if present.
    if let Some(type_hint_and_heap) = expr.type_hint(db) {
        collect_type_hint_names(db, type_hint_and_heap, &mut scope, &mut next_id);
    }

    // Pass 2: Resolve expression references.
    let expr_and_heap = expr.expr(db);
    let expr_inner = expr_and_heap.expr(db);
    resolve_expr_refs(db, expr_inner, &scope, &mut resolutions_map, &mut errors_map);

    // Convert HashMaps to Vecs of tracked structs for salsa.
    let resolutions = resolutions_map
        .into_iter()
        .map(|(name, resolution)| ResolutionEntry::new(db, name, resolution))
        .collect();
    let errors = errors_map
        .into_iter()
        .map(|(name, error)| ResolutionErrorEntry::new(db, name, error))
        .collect();

    ResolvedExpr::new(db, expr, resolutions, errors, source)
}

/// Scope tracking during resolution.
#[derive(Clone)]
struct Scope<'db> {
    /// Bindings visible in this scope.
    /// Later bindings shadow earlier ones at the same level.
    bindings: HashMap<InternedText<'db>, (BindingId, TypeHintAndHeap<'db>)>,

    /// Parent scope (None for outermost).
    parent: Option<Box<Scope<'db>>>,

    /// Depth of this scope (0 = outermost).
    depth: u32,
}

impl<'db> Scope<'db> {
    fn new() -> Self {
        Scope {
            bindings: HashMap::new(),
            parent: None,
            depth: 0,
        }
    }

    /// Lookup name, searching from innermost to outermost scope.
    /// Returns (binding_id, definition, scope_depth) if found.
    fn lookup(&self, name: InternedText<'db>) -> Option<(BindingId, TypeHintAndHeap<'db>, u32)> {
        if let Some((id, def)) = self.bindings.get(&name) {
            Some((*id, *def, self.depth))
        } else if let Some(parent) = &self.parent {
            parent.lookup(name)
        } else {
            None
        }
    }

    /// Insert binding into current scope.
    /// Shadows any previous binding with the same name at this level.
    fn insert(&mut self, name: InternedText<'db>, id: BindingId, def: TypeHintAndHeap<'db>) {
        self.bindings.insert(name, (id, def));
    }
}

/// Walk type hint collecting named definitions.
fn collect_type_hint_names<'db>(
    db: &'db dyn crate::Db,
    type_hint_and_heap: TypeHintAndHeap<'db>,
    scope: &mut Scope<'db>,
    next_id: &mut u32,
) {
    let type_hint = type_hint_and_heap.type_hint(db);
    collect_type_hint_names_inner(db, type_hint, type_hint_and_heap, scope, next_id);
}

fn collect_type_hint_names_inner<'db>(
    db: &'db dyn crate::Db,
    type_hint: TypeHint<'db>,
    type_hint_and_heap: TypeHintAndHeap<'db>,
    scope: &mut Scope<'db>,
    next_id: &mut u32,
) {
    match type_hint {
        TypeHint::AnonTuple(t) => {
            for field in t.fields(db) {
                collect_type_hint_names(db, field, scope, next_id);
            }
        }
        TypeHint::AnonStruct(s) => {
            for field in s.fields(db) {
                let field_type = field.type_hint(db);
                collect_type_hint_names(db, field_type, scope, next_id);
            }
        }
        TypeHint::AnonEnum(e) => {
            for variant in e.variants(db) {
                if let Some(payload) = variant.payload(db) {
                    collect_type_hint_names(db, payload, scope, next_id);
                }
            }
        }
        TypeHint::List(l) => {
            collect_type_hint_names(db, l.element_type(db), scope, next_id);
        }
        TypeHint::Map(m) => {
            collect_type_hint_names(db, m.key_type(db), scope, next_id);
            collect_type_hint_names(db, m.value_type(db), scope, next_id);
        }
        TypeHint::Set(s) => {
            collect_type_hint_names(db, s.element_type(db), scope, next_id);
        }
        TypeHint::Option(o) => {
            collect_type_hint_names(db, o.inner_type(db), scope, next_id);
        }
        TypeHint::Result(r) => {
            collect_type_hint_names(db, r.inner_type(db), scope, next_id);
        }
        TypeHint::Tensor(t) => {
            collect_type_hint_names(db, t.element_type(db), scope, next_id);
        }
        TypeHint::Bool
        | TypeHint::U8
        | TypeHint::I8
        | TypeHint::U16
        | TypeHint::I16
        | TypeHint::U32
        | TypeHint::I32
        | TypeHint::U64
        | TypeHint::I64
        | TypeHint::F32
        | TypeHint::Int
        | TypeHint::String
        | TypeHint::Data
        | TypeHint::Error
        | TypeHint::ParseError(_) => {
            // No names to collect.
        }
    }
}

/// Walk expression resolving token references.
fn resolve_expr_refs<'db>(
    db: &'db dyn crate::Db,
    expr: Expr<'db>,
    scope: &Scope<'db>,
    resolutions: &mut HashMap<InternedText<'db>, Resolution<'db>>,
    errors: &mut HashMap<InternedText<'db>, ResolutionError>,
) {
    match expr {
        Expr::AnonTuple(t) => {
            for element in t.elements(db) {
                let element_expr = element.expr(db).expr(db);
                resolve_expr_refs(db, element_expr, scope, resolutions, errors);
            }
        }
        Expr::AnonStruct(s) => {
            for field in s.fields(db) {
                let field_value = field.value(db).expr(db).expr(db);
                resolve_expr_refs(db, field_value, scope, resolutions, errors);
            }
        }
        Expr::AnonEnum(e) => {
            if let Some(payload) = e.payload(db) {
                let payload_expr = payload.expr(db).expr(db);
                resolve_expr_refs(db, payload_expr, scope, resolutions, errors);
            }
        }
        Expr::List(l) => {
            for element in l.elements(db) {
                let element_expr = element.expr(db).expr(db);
                resolve_expr_refs(db, element_expr, scope, resolutions, errors);
            }
        }
        Expr::Map(m) => {
            for entry in m.entries(db) {
                let key_expr = entry.key(db).expr(db).expr(db);
                let value_expr = entry.value(db).expr(db).expr(db);
                resolve_expr_refs(db, key_expr, scope, resolutions, errors);
                resolve_expr_refs(db, value_expr, scope, resolutions, errors);
            }
        }
        Expr::Set(s) => {
            for element in s.elements(db) {
                let element_expr = element.expr(db).expr(db);
                resolve_expr_refs(db, element_expr, scope, resolutions, errors);
            }
        }
        Expr::Tensor(t) => {
            for element in t.elements(db) {
                let element_expr = element.expr(db).expr(db);
                resolve_expr_refs(db, element_expr, scope, resolutions, errors);
            }
        }
        Expr::Data(d) => {
            let value_expr = d.value(db).expr(db).expr(db);
            resolve_expr_refs(db, value_expr, scope, resolutions, errors);
        }
        Expr::Err(e) => {
            let value_expr = e.value(db).expr(db).expr(db);
            resolve_expr_refs(db, value_expr, scope, resolutions, errors);
        }
        Expr::True
        | Expr::False
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::Hex(_)
        | Expr::String(_)
        | Expr::None
        | Expr::ParseError(_) => {
            // No names to resolve.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_for_test;
    use bct::input::Source;

}
