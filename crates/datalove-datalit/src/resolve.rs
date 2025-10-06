use rmx::prelude::*;
use bct::text::InternedText;
use std::collections::HashMap;

use crate::ast::*;

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

/// Resolution result for an expression.
#[salsa::tracked]
pub struct ResolvedExpr<'db> {
    /// Original expression.
    pub expr: ExprFull<'db>,

    /// List of successful resolutions.
    pub resolutions: Vec<ResolutionEntry<'db>>,

    /// List of resolution errors.
    pub errors: Vec<ResolutionErrorEntry<'db>>,
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

    ResolvedExpr::new(db, expr, resolutions, errors)
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

    fn push_child(&self) -> Self {
        Scope {
            bindings: HashMap::new(),
            parent: Some(Box::new(self.clone())),
            depth: self.depth + 1,
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
        TypeHint::NamedTuple(t) => {
            let name = t.name(db);
            let id = BindingId(*next_id);
            *next_id += 1;
            scope.insert(name, id, type_hint_and_heap);

            // Recursively collect from fields.
            let mut child_scope = scope.push_child();
            for field in t.fields(db) {
                collect_type_hint_names(db, field, &mut child_scope, next_id);
            }
        }
        TypeHint::NamedStruct(s) => {
            let name = s.name(db);
            let id = BindingId(*next_id);
            *next_id += 1;
            scope.insert(name, id, type_hint_and_heap);

            // Recursively collect from fields.
            let mut child_scope = scope.push_child();
            for field in s.fields(db) {
                let field_type = field.type_hint(db);
                collect_type_hint_names(db, field_type, &mut child_scope, next_id);
            }
        }
        TypeHint::NamedEnum(e) => {
            let name = e.name(db);
            let id = BindingId(*next_id);
            *next_id += 1;
            scope.insert(name, id, type_hint_and_heap);

            // Recursively collect from variants.
            let mut child_scope = scope.push_child();
            for variant in e.variants(db) {
                if let Some(payload) = variant.payload(db) {
                    collect_type_hint_names(db, payload, &mut child_scope, next_id);
                }
            }
        }
        TypeHint::AnonTuple(t) => {
            let mut child_scope = scope.push_child();
            for field in t.fields(db) {
                collect_type_hint_names(db, field, &mut child_scope, next_id);
            }
        }
        TypeHint::AnonStruct(s) => {
            let mut child_scope = scope.push_child();
            for field in s.fields(db) {
                let field_type = field.type_hint(db);
                collect_type_hint_names(db, field_type, &mut child_scope, next_id);
            }
        }
        TypeHint::AnonEnum(e) => {
            let mut child_scope = scope.push_child();
            for variant in e.variants(db) {
                if let Some(payload) = variant.payload(db) {
                    collect_type_hint_names(db, payload, &mut child_scope, next_id);
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
        TypeHint::Bool
        | TypeHint::U32
        | TypeHint::F32
        | TypeHint::Int
        | TypeHint::String
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
        Expr::NamedTuple(t) => {
            let name = t.name(db);
            if let Some((binding_id, definition, scope_depth)) = scope.lookup(name) {
                let resolution = Resolution::new(db, binding_id, scope_depth, definition);
                resolutions.insert(name, resolution);
            } else {
                errors.insert(name, ResolutionError::UnboundName);
            }

            // Recursively resolve elements.
            for element in t.elements(db) {
                let element_expr = element.expr(db).expr(db);
                resolve_expr_refs(db, element_expr, scope, resolutions, errors);
            }
        }
        Expr::NamedStruct(s) => {
            let name = s.name(db);
            if let Some((binding_id, definition, scope_depth)) = scope.lookup(name) {
                let resolution = Resolution::new(db, binding_id, scope_depth, definition);
                resolutions.insert(name, resolution);
            } else {
                errors.insert(name, ResolutionError::UnboundName);
            }

            // Recursively resolve fields.
            for field in s.fields(db) {
                let field_value = field.value(db).expr(db).expr(db);
                resolve_expr_refs(db, field_value, scope, resolutions, errors);
            }
        }
        Expr::NamedEnum(e) => {
            let name = e.enum_name(db);
            if let Some((binding_id, definition, scope_depth)) = scope.lookup(name) {
                let resolution = Resolution::new(db, binding_id, scope_depth, definition);
                resolutions.insert(name, resolution);
            } else {
                errors.insert(name, ResolutionError::UnboundName);
            }

            // Recursively resolve payload if present.
            if let Some(payload) = e.payload(db) {
                let payload_expr = payload.expr(db).expr(db);
                resolve_expr_refs(db, payload_expr, scope, resolutions, errors);
            }
        }
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
        Expr::Err(e) => {
            let value_expr = e.value(db).expr(db).expr(db);
            resolve_expr_refs(db, value_expr, scope, resolutions, errors);
        }
        Expr::True
        | Expr::False
        | Expr::Int(_)
        | Expr::U32(_)
        | Expr::F32(_)
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
    use crate::parser::parse;
    use bct::input::Source;

    #[test]
    fn test_resolve_named_struct() {
        let ref db = crate::Database::default();
        let source = Source::new(
            db,
            S(": @struct Point { x: @u32, y: @u32 } / @struct Point { x = @1, y = @2 }")
        );
        let ast = parse(db, source);
        let resolved = resolve_names(db, ast);

        // Should have one resolution for "Point".
        assert_eq!(resolved.resolutions(db).len(), 1);
        assert_eq!(resolved.errors(db).len(), 0);

        let point_name = InternedText::new(db, S("Point"));
        let resolutions = resolved.resolutions(db);
        let entry = resolutions.iter().find(|entry| entry.name(db) == point_name).unwrap();
        let resolution = entry.resolution(db);
        assert_eq!(resolution.scope_depth(db), 0);
    }

    #[test]
    fn test_resolve_shadowing() {
        let ref db = crate::Database::default();
        // Create a nested structure where inner scope shadows outer scope.
        // Outer: struct Outer, Inner field contains another struct Inner.
        let source = Source::new(
            db,
            S(": @struct Outer { inner: @struct Inner { x: @u32 } } / @struct Outer { inner = @struct Inner { x = @1 } }")
        );
        let ast = parse(db, source);
        let resolved = resolve_names(db, ast);

        // Should have resolution for "Outer" (inner struct definition is in type hint, not referenced in expr).
        assert_eq!(resolved.resolutions(db).len(), 1);
        assert_eq!(resolved.errors(db).len(), 0);

        let outer_name = InternedText::new(db, S("Outer"));
        assert!(resolved.resolutions(db).iter().any(|entry| entry.name(db) == outer_name));
    }

    #[test]
    fn test_resolve_named_tuple() {
        let ref db = crate::Database::default();
        let source = Source::new(
            db,
            S(": @tuple Pair (@u32, @u32) / @tuple Pair (@1, @2)")
        );
        let ast = parse(db, source);
        let resolved = resolve_names(db, ast);

        // Should have one resolution for "Pair".
        assert_eq!(resolved.resolutions(db).len(), 1);
        assert_eq!(resolved.errors(db).len(), 0);

        let pair_name = InternedText::new(db, S("Pair"));
        assert!(resolved.resolutions(db).iter().any(|entry| entry.name(db) == pair_name));
    }

    #[test]
    fn test_resolve_named_enum() {
        let ref db = crate::Database::default();
        let source = Source::new(
            db,
            S(": @enum Result { Ok: @u32, Err: @string } / @enum Result.Ok(@42)")
        );
        let ast = parse(db, source);
        let resolved = resolve_names(db, ast);

        // Should have one resolution for "Result".
        assert_eq!(resolved.resolutions(db).len(), 1);
        assert_eq!(resolved.errors(db).len(), 0);

        let result_name = InternedText::new(db, S("Result"));
        assert!(resolved.resolutions(db).iter().any(|entry| entry.name(db) == result_name));
    }
}
