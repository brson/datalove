use rmx::prelude::*;
use bct::text::{InternedText, Text};
use std::collections::HashMap;
use crate::ast::*;
use crate::resolve::{ResolvedExpr, Resolution};
use datalove_diagnostic::{ByteSpan, DiagnosticBuilder};

/// Type representation (synthesized types, mirrors TypeHint but without parse errors).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub enum Type<'db> {
    Bool,
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    F32,
    Int,
    String,
    AnonTuple(TypeAnonTuple<'db>),
    NamedTuple(TypeNamedTuple<'db>),
    AnonStruct(TypeAnonStruct<'db>),
    NamedStruct(TypeNamedStruct<'db>),
    AnonEnum(TypeAnonEnum<'db>),
    NamedEnum(TypeNamedEnum<'db>),
    List(TypeList<'db>),
    Map(TypeMap<'db>),
    Set(TypeSet<'db>),
    Option(TypeOption<'db>),
    Result(TypeResult<'db>),
    Tensor(TypeTensor<'db>),
    Data,
    Error,
}

#[salsa::tracked]
pub struct TypeAndHeap<'db> {
    pub heap: Heap,
    #[returns(ref)]
    pub ty: Type<'db>,
}

#[salsa::tracked]
pub struct TypeAnonTuple<'db> {
    pub fields: Vec<TypeAndHeap<'db>>,
}

#[salsa::tracked]
pub struct TypeNamedTuple<'db> {
    pub name: InternedText<'db>,
    pub fields: Vec<TypeAndHeap<'db>>,
}

#[salsa::tracked]
pub struct TypeAnonStruct<'db> {
    pub fields: Vec<TypeNamedField<'db>>,
}

#[salsa::tracked]
pub struct TypeNamedStruct<'db> {
    pub name: InternedText<'db>,
    pub fields: Vec<TypeNamedField<'db>>,
}

#[salsa::tracked]
pub struct TypeNamedField<'db> {
    pub name: InternedText<'db>,
    pub ty: TypeAndHeap<'db>,
}

#[salsa::tracked]
pub struct TypeAnonEnum<'db> {
    pub variants: Vec<TypeEnumVariant<'db>>,
}

#[salsa::tracked]
pub struct TypeNamedEnum<'db> {
    pub name: InternedText<'db>,
    pub variants: Vec<TypeEnumVariant<'db>>,
}

#[salsa::tracked]
pub struct TypeEnumVariant<'db> {
    pub name: InternedText<'db>,
    pub payload: Option<TypeAndHeap<'db>>,
}

#[salsa::tracked]
pub struct TypeList<'db> {
    pub element_type: TypeAndHeap<'db>,
}

#[salsa::tracked]
pub struct TypeMap<'db> {
    pub key_type: TypeAndHeap<'db>,
    pub value_type: TypeAndHeap<'db>,
}

#[salsa::tracked]
pub struct TypeSet<'db> {
    pub element_type: TypeAndHeap<'db>,
}

#[salsa::tracked]
pub struct TypeOption<'db> {
    pub inner_type: TypeAndHeap<'db>,
}

#[salsa::tracked]
pub struct TypeResult<'db> {
    pub inner_type: TypeAndHeap<'db>,
}

#[salsa::tracked]
pub struct TypeTensor<'db> {
    pub element_type: TypeAndHeap<'db>,
    pub rank: u32,
}

/// Type error representation.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum TypeError {
    TypeMismatch { expected: String, actual: String },
    HeapMismatch { expected_heap: String, actual_heap: String },
    CannotSynthesize,
    UnresolvedName(String),
    MissingField(String),
    ExtraField(String),
    FieldOrderMismatch,
    IntOutOfRange,
    VariantNotFound(String),
    ArityMismatch { expected: usize, actual: usize },
}

/// Type error entry with location info.
#[salsa::tracked]
pub struct TypeErrorEntry<'db> {
    pub error: TypeError,
}

/// Result of typechecking.
#[salsa::tracked]
pub struct TypecheckResult<'db> {
    /// The root expression.
    pub root_expr: ExprFull<'db>,

    /// The root expression type (if successfully synthesized).
    pub root_type: Option<TypeAndHeap<'db>>,

    /// Type errors encountered.
    pub errors: Vec<TypeErrorEntry<'db>>,

    /// The resolved expression context.
    ///
    /// Preserved for instantiation of nested types like Data/Error.
    pub resolved: ResolvedExpr<'db>,
}

/// Context for typechecking.
struct TypeContext<'db> {
    db: &'db dyn crate::Db,
    source: bct::input::Source,
    resolutions: HashMap<InternedText<'db>, Resolution<'db>>,
    errors: Vec<TypeError>,
}

impl<'db> TypeContext<'db> {
    fn new(db: &'db dyn crate::Db, resolved: ResolvedExpr<'db>) -> Self {
        let resolutions = resolved
            .resolutions(db)
            .iter()
            .map(|entry| (entry.name(db), entry.resolution(db)))
            .collect();

        TypeContext {
            db,
            source: resolved.source(db),
            resolutions,
            errors: Vec::new(),
        }
    }

    fn add_error(&mut self, error: TypeError) {
        self.errors.push(error);
    }

    fn lookup_resolution(&self, name: InternedText<'db>) -> Option<Resolution<'db>> {
        self.resolutions.get(&name).copied()
    }

    /// Look up the source location for an expression (on-demand).
    fn get_span(&self, expr: ExprFull<'db>) -> Option<(Text<'db>, ByteSpan)> {
        let spans = crate::spans::datalit_spans(self.db, self.source);
        spans.get_text_and_span(self.db, expr)
    }
}

/// Main entry point: typecheck an expression.
#[salsa::tracked]
pub fn type_check<'db>(
    db: &'db dyn crate::Db,
    expr: ExprFull<'db>,
    resolved: ResolvedExpr<'db>,
) -> TypecheckResult<'db> {
    type_check_with_expected(db, expr, resolved, None)
}

/// Type check an expression with an optional expected type.
///
/// When expected type is provided, uses checking mode (bidirectional typing).
/// Otherwise uses synthesis mode.
pub fn type_check_with_expected<'db>(
    db: &'db dyn crate::Db,
    expr: ExprFull<'db>,
    resolved: ResolvedExpr<'db>,
    expected: Option<TypeAndHeap<'db>>,
) -> TypecheckResult<'db> {
    let mut ctx = TypeContext::new(db, resolved);

    // Check for resolution errors first.
    for error_entry in resolved.errors(db) {
        let name = error_entry.name(db).as_str(db).to_string();
        ctx.add_error(TypeError::UnresolvedName(name));
    }

    let root_type = if let Some(expected_ty) = expected {
        // Use checking mode when expected type is provided.
        match check(&mut ctx, expr, expected_ty) {
            Ok(()) => Some(expected_ty),
            Err(e) => {
                ctx.add_error(e);
                None
            }
        }
    } else {
        // Use synthesis mode when no expected type.
        match synthesize(&mut ctx, expr) {
            Ok(ty) => Some(ty),
            Err(e) => {
                ctx.add_error(e);
                None
            }
        }
    };

    let errors = ctx
        .errors
        .into_iter()
        .map(|e| TypeErrorEntry::new(db, e))
        .collect();

    TypecheckResult::new(db, expr, root_type, errors, resolved)
}

/// Synthesize a type for an expression.
fn synthesize<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFull<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;

    // Rule: Syn-TypedExpr - if type hint present, check against it.
    if let Some(type_hint_and_heap) = expr.type_hint(db) {
        let expected_type = convert_type_hint(db, type_hint_and_heap)?;
        check(ctx, expr, expected_type)?;
        return Ok(expected_type);
    }

    // Otherwise, synthesize from the expression.
    let expr_and_heap = expr.expr(db);
    let heap = expr_and_heap.heap(db);
    let expr_inner = expr_and_heap.expr(db);

    let ty = match expr_inner {
        // Rule: Syn-Bool
        Expr::True | Expr::False => Type::Bool,

        // Rule: Syn-String
        Expr::String(_) => Type::String,

        // Rule: Syn-Int - default to u32 with range check.
        Expr::Int(i) => {
            let value_str = i.value(db).as_str(db);
            if value_str.parse::<u32>().is_ok() {
                Type::U32
            } else {
                // T001: Integer out of range.
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "integer literal out of range")
                        .code("T001")
                        .primary_label(text, span, "value too large for u32")
                        .note("integer literals default to u32 type, which has a maximum value of 4,294,967,295")
                        .emit_type();
                }
                return Err(TypeError::IntOutOfRange);
            }
        }

        // Rule: Syn-Float - default to f32.
        Expr::Float(_) => Type::F32,

        // Rule: Syn-Hex - default to u32 (most common use case).
        Expr::Hex(h) => {
            let value_str = h.value(db).as_str(db);
            // Strip 0x/0X prefix and optional leading minus.
            let hex_part = value_str.trim_start_matches('-').trim_start_matches("0x").trim_start_matches("0X");
            if u32::from_str_radix(hex_part, 16).is_ok() && !value_str.starts_with('-') {
                Type::U32
            } else {
                // T001: Hex literal out of range for u32.
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "hex literal out of range")
                        .code("T001")
                        .primary_label(text, span, "value too large for u32")
                        .note("hex literals default to u32 type; use a type hint for other types")
                        .emit_type();
                }
                return Err(TypeError::IntOutOfRange);
            }
        }

        // Rule: Syn-NamedTuple
        Expr::NamedTuple(t) => {
            let name = t.name(db);
            let resolution = ctx
                .lookup_resolution(name)
                .ok_or_else(|| {
                    // T002: Unresolved name (named tuple).
                    if let Some((text, span)) = ctx.get_span(expr) {
                        let msg = format!("cannot find type `{}`", name.as_str(db));
                        DiagnosticBuilder::error(db, &msg)
                            .code("T002")
                            .primary_label(text, span, "not found in this scope")
                            .note("named types must be defined in the type hint before they can be used")
                            .emit_type();
                    }
                    TypeError::UnresolvedName(name.as_str(db).to_string())
                })?;

            let definition = resolution.definition(db);
            let expected_type = convert_type_hint(db, definition)?;

            // Check elements against expected type.
            check(ctx, expr, expected_type)?;

            expected_type.ty(db).clone()
        }

        // Rule: Syn-NamedStruct
        Expr::NamedStruct(s) => {
            let name = s.name(db);
            let resolution = ctx
                .lookup_resolution(name)
                .ok_or_else(|| {
                    // T003: Unresolved name (named struct).
                    if let Some((text, span)) = ctx.get_span(expr) {
                        let msg = format!("cannot find type `{}`", name.as_str(db));
                        DiagnosticBuilder::error(db, &msg)
                            .code("T003")
                            .primary_label(text, span, "not found in this scope")
                            .note("named types must be defined in the type hint before they can be used")
                            .emit_type();
                    }
                    TypeError::UnresolvedName(name.as_str(db).to_string())
                })?;

            let definition = resolution.definition(db);
            let expected_type = convert_type_hint(db, definition)?;

            // Check fields against expected type.
            check(ctx, expr, expected_type)?;

            expected_type.ty(db).clone()
        }

        // Rule: Syn-NamedEnum
        Expr::NamedEnum(e) => {
            let name = e.enum_name(db);
            let resolution = ctx
                .lookup_resolution(name)
                .ok_or_else(|| {
                    // T004: Unresolved name (named enum).
                    if let Some((text, span)) = ctx.get_span(expr) {
                        let msg = format!("cannot find type `{}`", name.as_str(db));
                        DiagnosticBuilder::error(db, &msg)
                            .code("T004")
                            .primary_label(text, span, "not found in this scope")
                            .note("named types must be defined in the type hint before they can be used")
                            .emit_type();
                    }
                    TypeError::UnresolvedName(name.as_str(db).to_string())
                })?;

            let definition = resolution.definition(db);
            let expected_type = convert_type_hint(db, definition)?;

            // Check variant and payload against expected type.
            check(ctx, expr, expected_type)?;

            expected_type.ty(db).clone()
        }

        // Rule: Syn-AnonTuple - synthesize tuple by synthesizing each element.
        Expr::AnonTuple(t) => {
            let elements = t.elements(db);
            let mut element_types = Vec::new();
            for elem in elements {
                let elem_type = synthesize(ctx, elem)?;
                element_types.push(elem_type);
            }
            Type::AnonTuple(TypeAnonTuple::new(db, element_types))
        }

        // Rule: Syn-AnonStruct - synthesize struct by synthesizing each field.
        Expr::AnonStruct(s) => {
            let fields = s.fields(db);
            let mut field_types = Vec::new();
            for field in fields {
                let field_name = field.name(db);
                let field_value = field.value(db);
                let field_type = synthesize(ctx, field_value)?;
                field_types.push(TypeNamedField::new(db, field_name, field_type));
            }
            Type::AnonStruct(TypeAnonStruct::new(db, field_types))
        }

        // Rule: Syn-List - synthesize list by synthesizing all elements (must have same type).
        Expr::List(l) => {
            let elements = l.elements(db);
            if elements.is_empty() {
                // T013: Cannot synthesize type for empty list.
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "cannot infer type for empty list")
                        .code("T013")
                        .primary_label(text, span, "type annotation required")
                        .note("provide a type hint to specify the element type, e.g., ': @list(@u32) / @list()'")
                        .emit_type();
                }
                return Err(TypeError::CannotSynthesize);
            }

            // Synthesize first element to get the expected type.
            let first_type = synthesize(ctx, elements[0])?;

            // Check remaining elements against first type.
            for elem in &elements[1..] {
                let elem_type = synthesize(ctx, *elem)?;
                if !types_equivalent(db, first_type.ty(db), elem_type.ty(db)) {
                    // T018: List element type mismatch.
                    if let Some((text, span)) = ctx.get_span(*elem) {
                        let msg = format!("mismatched types in list");
                        DiagnosticBuilder::error(db, &msg)
                            .code("T018")
                            .primary_label(text, span, &format!("expected `{}`, found `{}`",
                                type_to_string(db, first_type.ty(db)),
                                type_to_string(db, elem_type.ty(db))))
                            .note("all elements in a list must have the same type")
                            .emit_type();
                    }
                    return Err(TypeError::TypeMismatch {
                        expected: type_to_string(db, first_type.ty(db)),
                        actual: type_to_string(db, elem_type.ty(db)),
                    });
                }
                if !heaps_compatible(first_type.heap(db), elem_type.heap(db)) {
                    // T033: List element heap mismatch.
                    if let Some((text, span)) = ctx.get_span(*elem) {
                        DiagnosticBuilder::error(db, "heap allocation mismatch in list")
                            .code("T033")
                            .primary_label(text, span, &format!("expected {}, found {}",
                                heap_to_string(first_type.heap(db)),
                                heap_to_string(elem_type.heap(db))))
                            .note("all elements in a list must have compatible heap allocations")
                            .emit_type();
                    }
                    return Err(TypeError::HeapMismatch {
                        expected_heap: heap_to_string(first_type.heap(db)),
                        actual_heap: heap_to_string(elem_type.heap(db)),
                    });
                }
            }

            Type::List(TypeList::new(db, first_type))
        }

        // Rule: Syn-Set - synthesize set by synthesizing all elements (must have same type).
        Expr::Set(s) => {
            let elements = s.elements(db);
            if elements.is_empty() {
                // T014: Cannot synthesize type for empty set.
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "cannot infer type for empty set")
                        .code("T014")
                        .primary_label(text, span, "type annotation required")
                        .note("provide a type hint to specify the element type, e.g., ': @set(@u32) / @set()'")
                        .emit_type();
                }
                return Err(TypeError::CannotSynthesize);
            }

            // Synthesize first element to get the expected type.
            let first_type = synthesize(ctx, elements[0])?;

            // Check remaining elements against first type.
            for elem in &elements[1..] {
                let elem_type = synthesize(ctx, *elem)?;
                if !types_equivalent(db, first_type.ty(db), elem_type.ty(db)) {
                    // T019: Set element type mismatch.
                    if let Some((text, span)) = ctx.get_span(*elem) {
                        let msg = format!("mismatched types in set");
                        DiagnosticBuilder::error(db, &msg)
                            .code("T019")
                            .primary_label(text, span, &format!("expected `{}`, found `{}`",
                                type_to_string(db, first_type.ty(db)),
                                type_to_string(db, elem_type.ty(db))))
                            .note("all elements in a set must have the same type")
                            .emit_type();
                    }
                    return Err(TypeError::TypeMismatch {
                        expected: type_to_string(db, first_type.ty(db)),
                        actual: type_to_string(db, elem_type.ty(db)),
                    });
                }
                if !heaps_compatible(first_type.heap(db), elem_type.heap(db)) {
                    // T034: Set element heap mismatch.
                    if let Some((text, span)) = ctx.get_span(*elem) {
                        DiagnosticBuilder::error(db, "heap allocation mismatch in set")
                            .code("T034")
                            .primary_label(text, span, &format!("expected {}, found {}",
                                heap_to_string(first_type.heap(db)),
                                heap_to_string(elem_type.heap(db))))
                            .note("all elements in a set must have compatible heap allocations")
                            .emit_type();
                    }
                    return Err(TypeError::HeapMismatch {
                        expected_heap: heap_to_string(first_type.heap(db)),
                        actual_heap: heap_to_string(elem_type.heap(db)),
                    });
                }
            }

            Type::Set(TypeSet::new(db, first_type))
        }

        // Rule: Syn-Map - synthesize map by synthesizing all keys and values (must have same types).
        Expr::Map(m) => {
            let entries = m.entries(db);
            if entries.is_empty() {
                // T015: Cannot synthesize type for empty map.
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "cannot infer type for empty map")
                        .code("T015")
                        .primary_label(text, span, "type annotation required")
                        .note("provide a type hint to specify the key and value types, e.g., ': @map(@string, @u32) / @map()'")
                        .emit_type();
                }
                return Err(TypeError::CannotSynthesize);
            }

            // Synthesize first entry to get the expected key and value types.
            let first_entry = entries[0];
            let first_key_type = synthesize(ctx, first_entry.key(db))?;
            let first_value_type = synthesize(ctx, first_entry.value(db))?;

            // Check remaining entries against first types.
            for entry in &entries[1..] {
                let key_type = synthesize(ctx, entry.key(db))?;
                let value_type = synthesize(ctx, entry.value(db))?;

                if !types_equivalent(db, first_key_type.ty(db), key_type.ty(db)) {
                    // T020: Map key type mismatch.
                    if let Some((text, span)) = ctx.get_span(entry.key(db)) {
                        let msg = format!("mismatched key types in map");
                        DiagnosticBuilder::error(db, &msg)
                            .code("T020")
                            .primary_label(text, span, &format!("expected `{}`, found `{}`",
                                type_to_string(db, first_key_type.ty(db)),
                                type_to_string(db, key_type.ty(db))))
                            .note("all keys in a map must have the same type")
                            .emit_type();
                    }
                    return Err(TypeError::TypeMismatch {
                        expected: type_to_string(db, first_key_type.ty(db)),
                        actual: type_to_string(db, key_type.ty(db)),
                    });
                }
                if !heaps_compatible(first_key_type.heap(db), key_type.heap(db)) {
                    // T035: Map key heap mismatch.
                    if let Some((text, span)) = ctx.get_span(entry.key(db)) {
                        DiagnosticBuilder::error(db, "heap allocation mismatch in map keys")
                            .code("T035")
                            .primary_label(text, span, &format!("expected {}, found {}",
                                heap_to_string(first_key_type.heap(db)),
                                heap_to_string(key_type.heap(db))))
                            .note("all keys in a map must have compatible heap allocations")
                            .emit_type();
                    }
                    return Err(TypeError::HeapMismatch {
                        expected_heap: heap_to_string(first_key_type.heap(db)),
                        actual_heap: heap_to_string(key_type.heap(db)),
                    });
                }

                if !types_equivalent(db, first_value_type.ty(db), value_type.ty(db)) {
                    // T021: Map value type mismatch.
                    if let Some((text, span)) = ctx.get_span(entry.value(db)) {
                        let msg = format!("mismatched value types in map");
                        DiagnosticBuilder::error(db, &msg)
                            .code("T021")
                            .primary_label(text, span, &format!("expected `{}`, found `{}`",
                                type_to_string(db, first_value_type.ty(db)),
                                type_to_string(db, value_type.ty(db))))
                            .note("all values in a map must have the same type")
                            .emit_type();
                    }
                    return Err(TypeError::TypeMismatch {
                        expected: type_to_string(db, first_value_type.ty(db)),
                        actual: type_to_string(db, value_type.ty(db)),
                    });
                }
                if !heaps_compatible(first_value_type.heap(db), value_type.heap(db)) {
                    // T036: Map value heap mismatch.
                    if let Some((text, span)) = ctx.get_span(entry.value(db)) {
                        DiagnosticBuilder::error(db, "heap allocation mismatch in map values")
                            .code("T036")
                            .primary_label(text, span, &format!("expected {}, found {}",
                                heap_to_string(first_value_type.heap(db)),
                                heap_to_string(value_type.heap(db))))
                            .note("all values in a map must have compatible heap allocations")
                            .emit_type();
                    }
                    return Err(TypeError::HeapMismatch {
                        expected_heap: heap_to_string(first_value_type.heap(db)),
                        actual_heap: heap_to_string(value_type.heap(db)),
                    });
                }
            }

            Type::Map(TypeMap::new(db, first_key_type, first_value_type))
        }

        // Rule: Syn-Data - data values synthesize as Type::Data.
        Expr::Data(_) => Type::Data,

        // Rule: Syn-Error - error values synthesize as Type::Error.
        Expr::Err(_) => Type::Error,

        // Cannot synthesize for these - need type context.
        Expr::AnonEnum(_)
        | Expr::None => {
            // T016: Cannot synthesize type for anonymous enum or None.
            if let Some((text, span)) = ctx.get_span(expr) {
                let msg = match expr_and_heap.expr(db) {
                    Expr::None => "cannot infer type for None value",
                    _ => "cannot infer type for anonymous enum",
                };
                DiagnosticBuilder::error(db, msg)
                    .code("T016")
                    .primary_label(text, span, "type annotation required")
                    .note("provide a type hint to specify the expected type")
                    .emit_type();
            }
            return Err(TypeError::CannotSynthesize);
        }

        // Rule: Syn-Tensor - synthesize tensor by synthesizing all elements (must have same type).
        Expr::Tensor(t) => {
            let shape = t.shape(db);
            let elements = t.elements(db);

            if elements.is_empty() {
                // Cannot synthesize type for empty tensor.
                return Err(TypeError::CannotSynthesize);
            }

            // Calculate expected element count from shape.
            let expected_count = shape.iter().map(|&d| d as usize).product::<usize>();
            if elements.len() != expected_count {
                return Err(TypeError::ArityMismatch {
                    expected: expected_count,
                    actual: elements.len(),
                });
            }

            // Synthesize first element to get the expected type.
            let first_type = synthesize(ctx, elements[0])?;

            // Check remaining elements against first type.
            for elem in &elements[1..] {
                let elem_type = synthesize(ctx, *elem)?;
                if !types_equivalent(db, first_type.ty(db), elem_type.ty(db)) {
                    return Err(TypeError::TypeMismatch {
                        expected: type_to_string(db, first_type.ty(db)),
                        actual: type_to_string(db, elem_type.ty(db)),
                    });
                }
                if !heaps_compatible(first_type.heap(db), elem_type.heap(db)) {
                    return Err(TypeError::HeapMismatch {
                        expected_heap: heap_to_string(first_type.heap(db)),
                        actual_heap: heap_to_string(elem_type.heap(db)),
                    });
                }
            }

            // Rank is the length of the shape vector.
            let rank = shape.len() as u32;

            Type::Tensor(TypeTensor::new(db, first_type, rank))
        }

        Expr::ParseError(_) => {
            // T017: Cannot synthesize type for parse error.
            if let Some((text, span)) = ctx.get_span(expr) {
                DiagnosticBuilder::error(db, "cannot type-check expression with parse errors")
                    .code("T017")
                    .primary_label(text, span, "parse error occurred here")
                    .note("fix the parse error before type checking")
                    .emit_type();
            }
            return Err(TypeError::CannotSynthesize);
        }
    };

    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Check an expression against an expected type.
fn check<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFull<'db>,
    expected: TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // First check heap compatibility.
    let expr_and_heap = expr.expr(db);
    let actual_heap = expr_and_heap.heap(db);
    let expected_heap = expected.heap(db);

    if !heaps_compatible(actual_heap, expected_heap) {
        // T037: General heap mismatch.
        if let Some((text, span)) = ctx.get_span(expr) {
            DiagnosticBuilder::error(db, "heap allocation mismatch")
                .code("T037")
                .primary_label(text, span, &format!("expected {}, found {}",
                    heap_to_string(expected_heap),
                    heap_to_string(actual_heap)))
                .note("heap-allocated and stack-allocated values cannot be mixed")
                .emit_type();
        }
        return Err(TypeError::HeapMismatch {
            expected_heap: heap_to_string(expected_heap),
            actual_heap: heap_to_string(actual_heap),
        });
    }

    let expr_inner = expr_and_heap.expr(db);
    let expected_type = expected.ty(db);

    match (expr_inner, expected_type) {
        // Rule: Check-None
        (Expr::None, Type::Option(_)) => Ok(()),

        // Rule: Check-Option (implicit wrapping)
        // IMPORTANT: This must come before Check-Subsume to allow string literals to coerce to Option<string>
        (_, Type::Option(opt)) => {
            // Try to check against inner type (implicit Some wrapping).
            check(ctx, expr, opt.inner_type(db))
        }

        // Rule: Check-ResultErr (implicit Err wrapping)
        (Expr::Err(_), Type::Result(_)) => {
            // Error expressions can check against any Result type (implicit Err wrapping).
            Ok(())
        }

        // Rule: Check-Result (implicit Ok wrapping)
        // IMPORTANT: This must come before Check-Subsume to allow string literals to coerce to Result<string>
        (_, Type::Result(res)) => {
            // Try to check against inner type (implicit Ok wrapping).
            check(ctx, expr, res.inner_type(db))
        }

        // Rule: Check-TypedInt - respect type hints on integer literals.
        // This must come before the bare integer patterns to ensure type hints are honored.
        // Only handles direct integer type hints, not Option/Result wrapped ones.
        (Expr::Int(_), _) if expr.type_hint(db).is_some() && is_direct_integer_type_hint(&expr.type_hint(db).unwrap().type_hint(db)) => {
            // Get the type from the hint and verify the literal value fits.
            let type_hint_and_heap = expr.type_hint(db).unwrap();
            let hinted_type = convert_type_hint(db, type_hint_and_heap)?;
            let hinted_type_inner = hinted_type.ty(db);

            // First check against the hinted type to ensure the literal is valid.
            let expr_without_hint = ExprFull::new(db, None, *expr_and_heap);
            check(ctx, expr_without_hint, hinted_type)?;

            // Now check if the hinted type matches or can widen to the expected type.
            if types_equivalent(db, hinted_type_inner, expected_type) {
                Ok(())
            } else if can_widen_to(hinted_type_inner, expected_type) {
                // Allow widening from hinted type to expected type.
                Ok(())
            } else {
                // T040: Type mismatch - cannot widen from hinted type.
                if let Some((text, span)) = ctx.get_span(expr) {
                    let msg = format!("mismatched types");
                    DiagnosticBuilder::error(db, &msg)
                        .code("T040")
                        .primary_label(text, span, &format!("expected `{}`, found `{}`",
                            type_to_string(db, expected_type),
                            type_to_string(db, hinted_type_inner)))
                        .note("type hints on integer literals are respected; widening is only allowed within the same signedness (unsigned→unsigned or signed→signed)")
                        .emit_type();
                }
                Err(TypeError::TypeMismatch {
                    expected: type_to_string(db, expected_type),
                    actual: type_to_string(db, hinted_type_inner),
                })
            }
        }

        // Rule: Check-Subsume - try synthesis first.
        // IMPORTANT: Synthesize from the inner expression without type hint to avoid infinite recursion.
        (Expr::True | Expr::False | Expr::String(_), _) => {
            let expr_without_hint = ExprFull::new(db, None, *expr_and_heap);
            let synthesized = synthesize(ctx, expr_without_hint)?;
            if !types_equivalent(db, synthesized.ty(db), expected_type) {
                // T022: Type mismatch for primitive literal.
                if let Some((text, span)) = ctx.get_span(expr) {
                    let msg = format!("mismatched types");
                    DiagnosticBuilder::error(db, &msg)
                        .code("T022")
                        .primary_label(text, span, &format!("expected `{}`, found `{}`",
                            type_to_string(db, expected_type),
                            type_to_string(db, synthesized.ty(db))))
                        .emit_type();
                }
                return Err(TypeError::TypeMismatch {
                    expected: type_to_string(db, expected_type),
                    actual: type_to_string(db, synthesized.ty(db)),
                });
            }
            Ok(())
        }

        // Rule: Check-Int
        (Expr::Int(i), Type::U8) => {
            let value_str = i.value(db).as_str(db);
            if value_str.parse::<u8>().is_ok() {
                Ok(())
            } else {
                // T005: Integer out of range for u8.
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "integer literal out of range for type u8")
                        .code("T005")
                        .primary_label(text, span, "value out of range")
                        .note("u8 can represent values from 0 to 255")
                        .emit_type();
                }
                Err(TypeError::IntOutOfRange)
            }
        }

        (Expr::Int(i), Type::I8) => {
            let value_str = i.value(db).as_str(db);
            if value_str.parse::<i8>().is_ok() {
                Ok(())
            } else {
                // T006: Integer out of range for i8.
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "integer literal out of range for type i8")
                        .code("T006")
                        .primary_label(text, span, "value out of range")
                        .note("i8 can represent values from -128 to 127")
                        .emit_type();
                }
                Err(TypeError::IntOutOfRange)
            }
        }

        (Expr::Int(i), Type::U16) => {
            let value_str = i.value(db).as_str(db);
            if value_str.parse::<u16>().is_ok() {
                Ok(())
            } else {
                // T007: Integer out of range for u16.
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "integer literal out of range for type u16")
                        .code("T007")
                        .primary_label(text, span, "value out of range")
                        .note("u16 can represent values from 0 to 65,535")
                        .emit_type();
                }
                Err(TypeError::IntOutOfRange)
            }
        }

        (Expr::Int(i), Type::I16) => {
            let value_str = i.value(db).as_str(db);
            if value_str.parse::<i16>().is_ok() {
                Ok(())
            } else {
                // T008: Integer out of range for i16.
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "integer literal out of range for type i16")
                        .code("T008")
                        .primary_label(text, span, "value out of range")
                        .note("i16 can represent values from -32,768 to 32,767")
                        .emit_type();
                }
                Err(TypeError::IntOutOfRange)
            }
        }

        (Expr::Int(i), Type::U32) => {
            let value_str = i.value(db).as_str(db);
            if value_str.parse::<u32>().is_ok() {
                Ok(())
            } else {
                // T009: Integer out of range for u32.
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "integer literal out of range for type u32")
                        .code("T009")
                        .primary_label(text, span, "value out of range")
                        .note("u32 can represent values from 0 to 4,294,967,295")
                        .emit_type();
                }
                Err(TypeError::IntOutOfRange)
            }
        }

        (Expr::Int(i), Type::I32) => {
            let value_str = i.value(db).as_str(db);
            if value_str.parse::<i32>().is_ok() {
                Ok(())
            } else {
                // T010: Integer out of range for i32.
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "integer literal out of range for type i32")
                        .code("T010")
                        .primary_label(text, span, "value out of range")
                        .note("i32 can represent values from -2,147,483,648 to 2,147,483,647")
                        .emit_type();
                }
                Err(TypeError::IntOutOfRange)
            }
        }

        (Expr::Int(i), Type::U64) => {
            let value_str = i.value(db).as_str(db);
            if value_str.parse::<u64>().is_ok() {
                Ok(())
            } else {
                // T011: Integer out of range for u64.
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "integer literal out of range for type u64")
                        .code("T011")
                        .primary_label(text, span, "value out of range")
                        .note("u64 can represent values from 0 to 18,446,744,073,709,551,615")
                        .emit_type();
                }
                Err(TypeError::IntOutOfRange)
            }
        }

        (Expr::Int(i), Type::I64) => {
            let value_str = i.value(db).as_str(db);
            if value_str.parse::<i64>().is_ok() {
                Ok(())
            } else {
                // T012: Integer out of range for i64.
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "integer literal out of range for type i64")
                        .code("T012")
                        .primary_label(text, span, "value out of range")
                        .note("i64 can represent values from -9,223,372,036,854,775,808 to 9,223,372,036,854,775,807")
                        .emit_type();
                }
                Err(TypeError::IntOutOfRange)
            }
        }

        (Expr::Int(_), Type::Int) => Ok(()),

        // Rule: Check-Float
        (Expr::Float(_), Type::F32) => Ok(()),

        // Rule: Check-Hex - hex literals can check against integer types or f32 (bit pattern).
        (Expr::Hex(h), Type::U8) => {
            let value_str = h.value(db).as_str(db);
            let hex_part = value_str.trim_start_matches("0x").trim_start_matches("0X");
            if u8::from_str_radix(hex_part, 16).is_ok() {
                Ok(())
            } else {
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "hex literal out of range for type u8")
                        .code("T005")
                        .primary_label(text, span, "value out of range")
                        .note("u8 can represent hex values from 0x00 to 0xFF")
                        .emit_type();
                }
                Err(TypeError::IntOutOfRange)
            }
        }
        (Expr::Hex(h), Type::U16) => {
            let value_str = h.value(db).as_str(db);
            let hex_part = value_str.trim_start_matches("0x").trim_start_matches("0X");
            if u16::from_str_radix(hex_part, 16).is_ok() {
                Ok(())
            } else {
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "hex literal out of range for type u16")
                        .code("T007")
                        .primary_label(text, span, "value out of range")
                        .note("u16 can represent hex values from 0x0000 to 0xFFFF")
                        .emit_type();
                }
                Err(TypeError::IntOutOfRange)
            }
        }
        (Expr::Hex(h), Type::U32) => {
            let value_str = h.value(db).as_str(db);
            let hex_part = value_str.trim_start_matches("0x").trim_start_matches("0X");
            if u32::from_str_radix(hex_part, 16).is_ok() {
                Ok(())
            } else {
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "hex literal out of range for type u32")
                        .code("T009")
                        .primary_label(text, span, "value out of range")
                        .note("u32 can represent hex values from 0x00000000 to 0xFFFFFFFF")
                        .emit_type();
                }
                Err(TypeError::IntOutOfRange)
            }
        }
        (Expr::Hex(h), Type::U64) => {
            let value_str = h.value(db).as_str(db);
            let hex_part = value_str.trim_start_matches("0x").trim_start_matches("0X");
            if u64::from_str_radix(hex_part, 16).is_ok() {
                Ok(())
            } else {
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "hex literal out of range for type u64")
                        .code("T011")
                        .primary_label(text, span, "value out of range")
                        .emit_type();
                }
                Err(TypeError::IntOutOfRange)
            }
        }
        (Expr::Hex(_), Type::Int) => Ok(()),
        // Hex as f32 bit pattern - any 32-bit hex value is valid.
        (Expr::Hex(h), Type::F32) => {
            let value_str = h.value(db).as_str(db);
            let hex_part = value_str.trim_start_matches("0x").trim_start_matches("0X");
            if u32::from_str_radix(hex_part, 16).is_ok() {
                Ok(())
            } else {
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "hex literal out of range for f32 bit pattern")
                        .code("T013")
                        .primary_label(text, span, "value out of range")
                        .note("f32 bit patterns must be 32-bit hex values (0x00000000 to 0xFFFFFFFF)")
                        .emit_type();
                }
                Err(TypeError::IntOutOfRange)
            }
        }

        // Rule: Check-AnonTuple
        (Expr::AnonTuple(t), Type::AnonTuple(expected_tuple)) => {
            let elements = t.elements(db);
            let expected_fields = expected_tuple.fields(db);

            if elements.len() != expected_fields.len() {
                // T038: Tuple arity mismatch.
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "tuple has wrong number of elements")
                        .code("T038")
                        .primary_label(text, span, &format!("expected {} element(s), found {}",
                            expected_fields.len(), elements.len()))
                        .emit_type();
                }
                return Err(TypeError::ArityMismatch {
                    expected: expected_fields.len(),
                    actual: elements.len(),
                });
            }

            for (elem, expected_field) in elements.iter().zip(expected_fields.iter()) {
                check(ctx, *elem, *expected_field)?;
            }

            Ok(())
        }

        // Rule: Check-AnonStruct
        (Expr::AnonStruct(s), Type::AnonStruct(expected_struct)) => {
            let fields = s.fields(db);
            let expected_fields = expected_struct.fields(db);

            if fields.len() != expected_fields.len() {
                // T039: Struct arity mismatch.
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "struct has wrong number of fields")
                        .code("T039")
                        .primary_label(text, span, &format!("expected {} field(s), found {}",
                            expected_fields.len(), fields.len()))
                        .emit_type();
                }
                return Err(TypeError::ArityMismatch {
                    expected: expected_fields.len(),
                    actual: fields.len(),
                });
            }

            for (field, expected_field) in fields.iter().zip(expected_fields.iter()) {
                let field_name = field.name(db);
                let expected_name = expected_field.name(db);

                if field_name != expected_name {
                    // T042: Struct field order mismatch.
                    if let Some((text, span)) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, "struct fields in wrong order")
                            .code("T042")
                            .primary_label(text, span, &format!("expected field `{}`, found `{}`",
                                expected_name.as_str(db), field_name.as_str(db)))
                            .note("struct fields must appear in the same order as the type definition")
                            .emit_type();
                    }
                    return Err(TypeError::FieldOrderMismatch);
                }

                check(ctx, field.value(db), expected_field.ty(db))?;
            }

            Ok(())
        }

        // Rule: Check-NamedStruct (anon struct -> named struct coercion)
        (Expr::AnonStruct(s), Type::NamedStruct(expected_struct)) => {
            let fields = s.fields(db);
            let expected_fields = expected_struct.fields(db);

            if fields.len() != expected_fields.len() {
                // T040: Struct arity mismatch (anon -> named coercion).
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "struct has wrong number of fields")
                        .code("T040")
                        .primary_label(text, span, &format!("expected {} field(s), found {}",
                            expected_fields.len(), fields.len()))
                        .emit_type();
                }
                return Err(TypeError::ArityMismatch {
                    expected: expected_fields.len(),
                    actual: fields.len(),
                });
            }

            for (field, expected_field) in fields.iter().zip(expected_fields.iter()) {
                let field_name = field.name(db);
                let expected_name = expected_field.name(db);

                if field_name != expected_name {
                    // T043: Struct field order mismatch (anon -> named coercion).
                    if let Some((text, span)) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, "struct fields in wrong order")
                            .code("T043")
                            .primary_label(text, span, &format!("expected field `{}`, found `{}`",
                                expected_name.as_str(db), field_name.as_str(db)))
                            .note("struct fields must appear in the same order as the type definition")
                            .emit_type();
                    }
                    return Err(TypeError::FieldOrderMismatch);
                }

                check(ctx, field.value(db), expected_field.ty(db))?;
            }

            Ok(())
        }

        // Named struct must match exactly.
        (Expr::NamedStruct(s), Type::NamedStruct(expected_struct)) => {
            let name = s.name(db);
            let expected_name = expected_struct.name(db);

            if name != expected_name {
                // T023: Named struct type mismatch.
                if let Some((text, span)) = ctx.get_span(expr) {
                    let msg = format!("mismatched struct types");
                    DiagnosticBuilder::error(db, &msg)
                        .code("T023")
                        .primary_label(text, span, &format!("expected `@struct {}`, found `@struct {}`",
                            expected_name.as_str(db), name.as_str(db)))
                        .emit_type();
                }
                return Err(TypeError::TypeMismatch {
                    expected: format!("@struct {}", expected_name.as_str(db)),
                    actual: format!("@struct {}", name.as_str(db)),
                });
            }

            let fields = s.fields(db);
            let expected_fields = expected_struct.fields(db);

            for (field, expected_field) in fields.iter().zip(expected_fields.iter()) {
                check(ctx, field.value(db), expected_field.ty(db))?;
            }

            Ok(())
        }

        // Rule: Check-AnonEnum
        (Expr::AnonEnum(e), Type::AnonEnum(expected_enum)) => {
            let variant_name = e.variant_name(db);
            let expected_variants = expected_enum.variants(db);

            let expected_variant = expected_variants
                .iter()
                .find(|v| v.name(db) == variant_name)
                .ok_or_else(|| {
                    // T044: Enum variant not found.
                    if let Some((text, span)) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, &format!("variant `{}` not found in enum", variant_name.as_str(db)))
                            .code("T044")
                            .primary_label(text, span, "variant not defined")
                            .emit_type();
                    }
                    TypeError::VariantNotFound(variant_name.as_str(db).to_string())
                })?;

            match (e.payload(db), expected_variant.payload(db)) {
                (Some(payload), Some(expected_payload)) => {
                    check(ctx, payload, expected_payload)
                }
                (None, None) => Ok(()),
                (Some(_), None) => {
                    // T024: Enum variant payload mismatch (has payload, expected none).
                    if let Some((text, span)) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, "enum variant payload mismatch")
                            .code("T024")
                            .primary_label(text, span, "expected no payload, found payload")
                            .emit_type();
                    }
                    Err(TypeError::TypeMismatch {
                        expected: "no payload".to_string(),
                        actual: "payload".to_string(),
                    })
                }
                (None, Some(_)) => {
                    // T025: Enum variant payload mismatch (no payload, expected payload).
                    if let Some((text, span)) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, "enum variant payload mismatch")
                            .code("T025")
                            .primary_label(text, span, "expected payload, found no payload")
                            .emit_type();
                    }
                    Err(TypeError::TypeMismatch {
                        expected: "payload".to_string(),
                        actual: "no payload".to_string(),
                    })
                }
            }
        }

        // Rule: Check-NamedEnum (anon enum -> named enum coercion)
        (Expr::AnonEnum(e), Type::NamedEnum(expected_enum)) => {
            let variant_name = e.variant_name(db);
            let expected_variants = expected_enum.variants(db);

            let expected_variant = expected_variants
                .iter()
                .find(|v| v.name(db) == variant_name)
                .ok_or_else(|| {
                    // T045: Enum variant not found (anon -> named coercion).
                    if let Some((text, span)) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, &format!("variant `{}` not found in enum", variant_name.as_str(db)))
                            .code("T045")
                            .primary_label(text, span, "variant not defined")
                            .emit_type();
                    }
                    TypeError::VariantNotFound(variant_name.as_str(db).to_string())
                })?;

            match (e.payload(db), expected_variant.payload(db)) {
                (Some(payload), Some(expected_payload)) => {
                    check(ctx, payload, expected_payload)
                }
                (None, None) => Ok(()),
                (Some(_), None) => {
                    // T026: Enum variant payload mismatch (has payload, expected none) - NamedEnum coercion.
                    if let Some((text, span)) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, "enum variant payload mismatch")
                            .code("T026")
                            .primary_label(text, span, "expected no payload, found payload")
                            .emit_type();
                    }
                    Err(TypeError::TypeMismatch {
                        expected: "no payload".to_string(),
                        actual: "payload".to_string(),
                    })
                }
                (None, Some(_)) => {
                    // T027: Enum variant payload mismatch (no payload, expected payload) - NamedEnum coercion.
                    if let Some((text, span)) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, "enum variant payload mismatch")
                            .code("T027")
                            .primary_label(text, span, "expected payload, found no payload")
                            .emit_type();
                    }
                    Err(TypeError::TypeMismatch {
                        expected: "payload".to_string(),
                        actual: "no payload".to_string(),
                    })
                }
            }
        }

        // Named enum must match exactly.
        (Expr::NamedEnum(e), Type::NamedEnum(expected_enum)) => {
            let enum_name = e.enum_name(db);
            let expected_name = expected_enum.name(db);

            if enum_name != expected_name {
                // T028: Named enum type mismatch.
                if let Some((text, span)) = ctx.get_span(expr) {
                    let msg = format!("mismatched enum types");
                    DiagnosticBuilder::error(db, &msg)
                        .code("T028")
                        .primary_label(text, span, &format!("expected `@enum {}`, found `@enum {}`",
                            expected_name.as_str(db), enum_name.as_str(db)))
                        .emit_type();
                }
                return Err(TypeError::TypeMismatch {
                    expected: format!("@enum {}", expected_name.as_str(db)),
                    actual: format!("@enum {}", enum_name.as_str(db)),
                });
            }

            let variant_name = e.variant_name(db);
            let expected_variants = expected_enum.variants(db);

            let expected_variant = expected_variants
                .iter()
                .find(|v| v.name(db) == variant_name)
                .ok_or_else(|| {
                    // T046: Enum variant not found (named enum exact match).
                    if let Some((text, span)) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, &format!("variant `{}` not found in enum", variant_name.as_str(db)))
                            .code("T046")
                            .primary_label(text, span, "variant not defined")
                            .emit_type();
                    }
                    TypeError::VariantNotFound(variant_name.as_str(db).to_string())
                })?;

            match (e.payload(db), expected_variant.payload(db)) {
                (Some(payload), Some(expected_payload)) => {
                    check(ctx, payload, expected_payload)
                }
                (None, None) => Ok(()),
                (Some(_), None) => {
                    // T029: Enum variant payload mismatch (has payload, expected none) - NamedEnum exact.
                    if let Some((text, span)) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, "enum variant payload mismatch")
                            .code("T029")
                            .primary_label(text, span, "expected no payload, found payload")
                            .emit_type();
                    }
                    Err(TypeError::TypeMismatch {
                        expected: "no payload".to_string(),
                        actual: "payload".to_string(),
                    })
                }
                (None, Some(_)) => {
                    // T030: Enum variant payload mismatch (no payload, expected payload) - NamedEnum exact.
                    if let Some((text, span)) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, "enum variant payload mismatch")
                            .code("T030")
                            .primary_label(text, span, "expected payload, found no payload")
                            .emit_type();
                    }
                    Err(TypeError::TypeMismatch {
                        expected: "payload".to_string(),
                        actual: "no payload".to_string(),
                    })
                }
            }
        }

        // Rule: Check-List
        (Expr::List(l), Type::List(expected_list)) => {
            let elements = l.elements(db);
            let element_type = expected_list.element_type(db);

            for elem in elements {
                check(ctx, elem, element_type)?;
            }

            Ok(())
        }

        // Rule: Check-Map
        (Expr::Map(m), Type::Map(expected_map)) => {
            let entries = m.entries(db);
            let key_type = expected_map.key_type(db);
            let value_type = expected_map.value_type(db);

            for entry in entries {
                check(ctx, entry.key(db), key_type)?;
                check(ctx, entry.value(db), value_type)?;
            }

            Ok(())
        }

        // Rule: Check-Set
        (Expr::Set(s), Type::Set(expected_set)) => {
            let elements = s.elements(db);
            let element_type = expected_set.element_type(db);

            for elem in elements {
                check(ctx, elem, element_type)?;
            }

            Ok(())
        }

        // Rule: Check-Tensor
        (Expr::Tensor(t), Type::Tensor(expected_tensor)) => {
            let shape = t.shape(db);
            let elements = t.elements(db);
            let element_type = expected_tensor.element_type(db);

            // Verify rank matches.
            let rank = shape.len() as u32;
            if rank != expected_tensor.rank(db) {
                return Err(TypeError::ArityMismatch {
                    expected: expected_tensor.rank(db) as usize,
                    actual: rank as usize,
                });
            }

            // Calculate expected element count from shape.
            let expected_count = shape.iter().map(|&d| d as usize).product::<usize>();
            if elements.len() != expected_count {
                return Err(TypeError::ArityMismatch {
                    expected: expected_count,
                    actual: elements.len(),
                });
            }

            // Check all elements against expected element type.
            for elem in elements {
                check(ctx, elem, element_type)?;
            }

            Ok(())
        }

        // Rule: Check-Data
        (Expr::Data(_), Type::Data) => Ok(()),

        // Rule: Check-Error
        (Expr::Err(_), Type::Error) => Ok(()),

        // Rule: Check-NamedTuple (anon tuple -> named tuple coercion)
        (Expr::AnonTuple(t), Type::NamedTuple(expected_tuple)) => {
            let elements = t.elements(db);
            let expected_fields = expected_tuple.fields(db);

            if elements.len() != expected_fields.len() {
                // T041: Tuple arity mismatch (anon -> named coercion).
                if let Some((text, span)) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "tuple has wrong number of elements")
                        .code("T041")
                        .primary_label(text, span, &format!("expected {} element(s), found {}",
                            expected_fields.len(), elements.len()))
                        .emit_type();
                }
                return Err(TypeError::ArityMismatch {
                    expected: expected_fields.len(),
                    actual: elements.len(),
                });
            }

            for (elem, expected_field) in elements.iter().zip(expected_fields.iter()) {
                check(ctx, *elem, *expected_field)?;
            }

            Ok(())
        }

        // Named tuple must match exactly.
        (Expr::NamedTuple(t), Type::NamedTuple(expected_tuple)) => {
            let name = t.name(db);
            let expected_name = expected_tuple.name(db);

            if name != expected_name {
                // T031: Named tuple type mismatch.
                if let Some((text, span)) = ctx.get_span(expr) {
                    let msg = format!("mismatched tuple types");
                    DiagnosticBuilder::error(db, &msg)
                        .code("T031")
                        .primary_label(text, span, &format!("expected `@tuple {}`, found `@tuple {}`",
                            expected_name.as_str(db), name.as_str(db)))
                        .emit_type();
                }
                return Err(TypeError::TypeMismatch {
                    expected: format!("@tuple {}", expected_name.as_str(db)),
                    actual: format!("@tuple {}", name.as_str(db)),
                });
            }

            let elements = t.elements(db);
            let expected_fields = expected_tuple.fields(db);

            for (elem, expected_field) in elements.iter().zip(expected_fields.iter()) {
                check(ctx, *elem, *expected_field)?;
            }

            Ok(())
        }

        // Otherwise, try subsumption.
        _ => {
            // Synthesize from the inner expression without type hint to avoid infinite recursion.
            let ty_without_hint = ExprFull::new(db, None, *expr_and_heap);
            let synthesized = synthesize(ctx, ty_without_hint)?;
            if types_equivalent(db, synthesized.ty(db), expected_type) {
                Ok(())
            } else if can_widen_to(synthesized.ty(db), expected_type) {
                // Allow numeric widening.
                Ok(())
            } else {
                // T032: General type mismatch (subsumption fallback).
                if let Some((text, span)) = ctx.get_span(expr) {
                    let msg = format!("mismatched types");
                    DiagnosticBuilder::error(db, &msg)
                        .code("T032")
                        .primary_label(text, span, &format!("expected `{}`, found `{}`",
                            type_to_string(db, expected_type),
                            type_to_string(db, synthesized.ty(db))))
                        .emit_type();
                }
                Err(TypeError::TypeMismatch {
                    expected: type_to_string(db, expected_type),
                    actual: type_to_string(db, synthesized.ty(db)),
                })
            }
        }
    }
}

/// Convert a type hint to a type.
pub fn convert_type_hint<'db>(
    db: &'db dyn crate::Db,
    type_hint_and_heap: TypeHintAndHeap<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let heap = type_hint_and_heap.heap(db);
    let type_hint = type_hint_and_heap.type_hint(db);

    let ty = match type_hint {
        TypeHint::Bool => Type::Bool,
        TypeHint::U8 => Type::U8,
        TypeHint::I8 => Type::I8,
        TypeHint::U16 => Type::U16,
        TypeHint::I16 => Type::I16,
        TypeHint::U32 => Type::U32,
        TypeHint::I32 => Type::I32,
        TypeHint::U64 => Type::U64,
        TypeHint::I64 => Type::I64,
        TypeHint::F32 => Type::F32,
        TypeHint::Int => Type::Int,
        TypeHint::String => Type::String,
        TypeHint::Data => Type::Data,
        TypeHint::Error => Type::Error,

        TypeHint::AnonTuple(t) => {
            let fields: Result<Vec<_>, _> = t
                .fields(db)
                .iter()
                .map(|f| convert_type_hint(db, *f))
                .collect();
            Type::AnonTuple(TypeAnonTuple::new(db, fields?))
        }

        TypeHint::NamedTuple(t) => {
            let name = t.name(db);
            let fields: Result<Vec<_>, _> = t
                .fields(db)
                .iter()
                .map(|f| convert_type_hint(db, *f))
                .collect();
            Type::NamedTuple(TypeNamedTuple::new(db, name, fields?))
        }

        TypeHint::AnonStruct(s) => {
            let fields: Result<Vec<_>, _> = s
                .fields(db)
                .iter()
                .map(|f| {
                    let name = f.name(db);
                    let ty = convert_type_hint(db, f.type_hint(db))?;
                    Ok(TypeNamedField::new(db, name, ty))
                })
                .collect();
            Type::AnonStruct(TypeAnonStruct::new(db, fields?))
        }

        TypeHint::NamedStruct(s) => {
            let name = s.name(db);
            let fields: Result<Vec<_>, _> = s
                .fields(db)
                .iter()
                .map(|f| {
                    let field_name = f.name(db);
                    let ty = convert_type_hint(db, f.type_hint(db))?;
                    Ok(TypeNamedField::new(db, field_name, ty))
                })
                .collect();
            Type::NamedStruct(TypeNamedStruct::new(db, name, fields?))
        }

        TypeHint::AnonEnum(e) => {
            let variants: Result<Vec<_>, _> = e
                .variants(db)
                .iter()
                .map(|v| {
                    let name = v.name(db);
                    let payload = v
                        .payload(db)
                        .map(|p| convert_type_hint(db, p))
                        .transpose()?;
                    Ok(TypeEnumVariant::new(db, name, payload))
                })
                .collect();
            Type::AnonEnum(TypeAnonEnum::new(db, variants?))
        }

        TypeHint::NamedEnum(e) => {
            let name = e.name(db);
            let variants: Result<Vec<_>, _> = e
                .variants(db)
                .iter()
                .map(|v| {
                    let variant_name = v.name(db);
                    let payload = v
                        .payload(db)
                        .map(|p| convert_type_hint(db, p))
                        .transpose()?;
                    Ok(TypeEnumVariant::new(db, variant_name, payload))
                })
                .collect();
            Type::NamedEnum(TypeNamedEnum::new(db, name, variants?))
        }

        TypeHint::List(l) => {
            let element_type = convert_type_hint(db, l.element_type(db))?;
            Type::List(TypeList::new(db, element_type))
        }

        TypeHint::Map(m) => {
            let key_type = convert_type_hint(db, m.key_type(db))?;
            let value_type = convert_type_hint(db, m.value_type(db))?;
            Type::Map(TypeMap::new(db, key_type, value_type))
        }

        TypeHint::Set(s) => {
            let element_type = convert_type_hint(db, s.element_type(db))?;
            Type::Set(TypeSet::new(db, element_type))
        }

        TypeHint::Option(o) => {
            let inner_type = convert_type_hint(db, o.inner_type(db))?;
            Type::Option(TypeOption::new(db, inner_type))
        }

        TypeHint::Result(r) => {
            let inner_type = convert_type_hint(db, r.inner_type(db))?;
            Type::Result(TypeResult::new(db, inner_type))
        }

        TypeHint::Tensor(t) => {
            let element_type = convert_type_hint(db, t.element_type(db))?;
            Type::Tensor(TypeTensor::new(db, element_type, t.rank(db)))
        }

        TypeHint::ParseError(_) => return Err(TypeError::CannotSynthesize),
    };

    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Check if two heaps are compatible.
/// Omitted heap is generic and compatible with any heap.
fn heaps_compatible(h1: Heap, h2: Heap) -> bool {
    match (h1, h2) {
        (Heap::Local, Heap::Local) => true,
        (Heap::Global, Heap::Global) => true,
        // Omitted is compatible with any heap (generic).
        (Heap::Omitted, _) => true,
        (_, Heap::Omitted) => true,
        _ => false,
    }
}

/// Check if two types are equivalent.
pub fn types_equivalent<'db>(db: &'db dyn crate::Db, t1: &Type<'db>, t2: &Type<'db>) -> bool {
    match (t1, t2) {
        (Type::Bool, Type::Bool) => true,
        (Type::U8, Type::U8) => true,
        (Type::I8, Type::I8) => true,
        (Type::U16, Type::U16) => true,
        (Type::I16, Type::I16) => true,
        (Type::U32, Type::U32) => true,
        (Type::I32, Type::I32) => true,
        (Type::U64, Type::U64) => true,
        (Type::I64, Type::I64) => true,
        (Type::F32, Type::F32) => true,
        (Type::Int, Type::Int) => true,
        (Type::String, Type::String) => true,
        (Type::Data, Type::Data) => true,
        (Type::Error, Type::Error) => true,

        (Type::AnonTuple(t1), Type::AnonTuple(t2)) => {
            let f1 = t1.fields(db);
            let f2 = t2.fields(db);
            f1.len() == f2.len()
                && f1
                    .iter()
                    .zip(f2.iter())
                    .all(|(a, b)| types_and_heaps_equivalent(db, a, b))
        }

        (Type::NamedTuple(t1), Type::NamedTuple(t2)) => {
            t1.name(db) == t2.name(db)
                && t1.fields(db).len() == t2.fields(db).len()
                && t1
                    .fields(db)
                    .iter()
                    .zip(t2.fields(db).iter())
                    .all(|(a, b)| types_and_heaps_equivalent(db, a, b))
        }

        (Type::AnonStruct(s1), Type::AnonStruct(s2)) => {
            let f1 = s1.fields(db);
            let f2 = s2.fields(db);
            f1.len() == f2.len()
                && f1.iter().zip(f2.iter()).all(|(a, b)| {
                    a.name(db) == b.name(db) && types_and_heaps_equivalent(db, &a.ty(db), &b.ty(db))
                })
        }

        (Type::NamedStruct(s1), Type::NamedStruct(s2)) => {
            s1.name(db) == s2.name(db)
                && s1.fields(db).len() == s2.fields(db).len()
                && s1.fields(db).iter().zip(s2.fields(db).iter()).all(|(a, b)| {
                    a.name(db) == b.name(db) && types_and_heaps_equivalent(db, &a.ty(db), &b.ty(db))
                })
        }

        (Type::AnonEnum(e1), Type::AnonEnum(e2)) => {
            // Enum variants are order-independent.
            let v1 = e1.variants(db);
            let v2 = e2.variants(db);
            v1.len() == v2.len()
                && v1.iter().all(|var1| {
                    v2.iter().any(|var2| {
                        var1.name(db) == var2.name(db)
                            && match (var1.payload(db), var2.payload(db)) {
                                (Some(p1), Some(p2)) => types_and_heaps_equivalent(db, &p1, &p2),
                                (None, None) => true,
                                _ => false,
                            }
                    })
                })
        }

        (Type::NamedEnum(e1), Type::NamedEnum(e2)) => {
            e1.name(db) == e2.name(db)
                && e1.variants(db).len() == e2.variants(db).len()
                && e1.variants(db).iter().all(|var1| {
                    e2.variants(db).iter().any(|var2| {
                        var1.name(db) == var2.name(db)
                            && match (var1.payload(db), var2.payload(db)) {
                                (Some(p1), Some(p2)) => types_and_heaps_equivalent(db, &p1, &p2),
                                (None, None) => true,
                                _ => false,
                            }
                    })
                })
        }

        (Type::List(l1), Type::List(l2)) => {
            types_and_heaps_equivalent(db, &l1.element_type(db), &l2.element_type(db))
        }

        (Type::Map(m1), Type::Map(m2)) => {
            types_and_heaps_equivalent(db, &m1.key_type(db), &m2.key_type(db))
                && types_and_heaps_equivalent(db, &m1.value_type(db), &m2.value_type(db))
        }

        (Type::Set(s1), Type::Set(s2)) => {
            types_and_heaps_equivalent(db, &s1.element_type(db), &s2.element_type(db))
        }

        (Type::Option(o1), Type::Option(o2)) => {
            types_and_heaps_equivalent(db, &o1.inner_type(db), &o2.inner_type(db))
        }

        (Type::Result(r1), Type::Result(r2)) => {
            types_and_heaps_equivalent(db, &r1.inner_type(db), &r2.inner_type(db))
        }

        (Type::Tensor(t1), Type::Tensor(t2)) => {
            t1.rank(db) == t2.rank(db)
                && types_and_heaps_equivalent(db, &t1.element_type(db), &t2.element_type(db))
        }

        _ => false,
    }
}

fn types_and_heaps_equivalent<'db>(
    db: &'db dyn crate::Db,
    t1: &TypeAndHeap<'db>,
    t2: &TypeAndHeap<'db>,
) -> bool {
    heaps_compatible(t1.heap(db), t2.heap(db)) && types_equivalent(db, t1.ty(db), t2.ty(db))
}

/// Check if a type hint is a direct integer type (not wrapped in Option/Result).
fn is_direct_integer_type_hint<'db>(type_hint: &TypeHint<'db>) -> bool {
    matches!(
        type_hint,
        TypeHint::U8 | TypeHint::I8 | TypeHint::U16 | TypeHint::I16 |
        TypeHint::U32 | TypeHint::I32 | TypeHint::U64 | TypeHint::I64 | TypeHint::Int
    )
}

/// Check if a type can widen to another type.
///
/// Supports numeric widening chains:
/// - u8 → u16 → u32 → u64 → int
/// - i8 → i16 → i32 → i64 → int
///
/// No cross-widening between unsigned and signed types.
pub fn can_widen_to<'db>(from: &Type<'db>, to: &Type<'db>) -> bool {
    match (from, to) {
        // Unsigned widening chain.
        (Type::U8, Type::U16 | Type::U32 | Type::U64 | Type::Int) => true,
        (Type::U16, Type::U32 | Type::U64 | Type::Int) => true,
        (Type::U32, Type::U64 | Type::Int) => true,
        (Type::U64, Type::Int) => true,

        // Signed widening chain.
        (Type::I8, Type::I16 | Type::I32 | Type::I64 | Type::Int) => true,
        (Type::I16, Type::I32 | Type::I64 | Type::Int) => true,
        (Type::I32, Type::I64 | Type::Int) => true,
        (Type::I64, Type::Int) => true,

        // No widening for other types.
        _ => false,
    }
}

/// Convert a heap to a string for error messages.
fn heap_to_string(heap: Heap) -> String {
    match heap {
        Heap::Local => "@".to_string(),
        Heap::Global => "#".to_string(),
        Heap::Omitted => "".to_string(),
    }
}

/// Convert a type to a string for error messages.
pub fn type_to_string<'db>(db: &'db dyn crate::Db, ty: &Type<'db>) -> String {
    match ty {
        Type::Bool => "bool".to_string(),
        Type::U8 => "u8".to_string(),
        Type::I8 => "i8".to_string(),
        Type::U16 => "u16".to_string(),
        Type::I16 => "i16".to_string(),
        Type::U32 => "u32".to_string(),
        Type::I32 => "i32".to_string(),
        Type::U64 => "u64".to_string(),
        Type::I64 => "i64".to_string(),
        Type::F32 => "f32".to_string(),
        Type::Int => "int".to_string(),
        Type::String => "string".to_string(),
        Type::Data => "data".to_string(),
        Type::Error => "error".to_string(),
        Type::AnonTuple(t) => {
            let fields: Vec<_> = t.fields(db)
                .iter()
                .map(|f| {
                    let heap = heap_to_string(f.heap(db));
                    let ty_str = type_to_string(db, f.ty(db));
                    format!("{}{}", heap, ty_str)
                })
                .collect();
            format!("({})", fields.join(", "))
        }
        Type::NamedTuple(t) => {
            let name = t.name(db).as_str(db);
            format!("@tuple {}", name)
        }
        Type::AnonStruct(s) => {
            let fields: Vec<_> = s.fields(db)
                .iter()
                .map(|f| {
                    let name = f.name(db).as_str(db);
                    let heap = heap_to_string(f.ty(db).heap(db));
                    let ty_str = type_to_string(db, f.ty(db).ty(db));
                    format!("{}: {}{}", name, heap, ty_str)
                })
                .collect();
            format!("{{{}}}", fields.join(", "))
        }
        Type::NamedStruct(s) => {
            let name = s.name(db).as_str(db);
            format!("@struct {}", name)
        }
        Type::AnonEnum(e) => {
            format!("@enum{{...}}")
        }
        Type::NamedEnum(e) => {
            let name = e.name(db).as_str(db);
            format!("@enum {}", name)
        }
        Type::List(l) => {
            let elem = l.element_type(db);
            let heap = heap_to_string(elem.heap(db));
            let ty_str = type_to_string(db, elem.ty(db));
            format!("[{}{}]", heap, ty_str)
        }
        Type::Map(m) => {
            let key = m.key_type(db);
            let value = m.value_type(db);
            format!("@map<{}, {}>",
                format!("{}{}", heap_to_string(key.heap(db)), type_to_string(db, key.ty(db))),
                format!("{}{}", heap_to_string(value.heap(db)), type_to_string(db, value.ty(db))))
        }
        Type::Set(s) => {
            let elem = s.element_type(db);
            let heap = heap_to_string(elem.heap(db));
            let ty_str = type_to_string(db, elem.ty(db));
            format!("@set<{}{}>", heap, ty_str)
        }
        Type::Option(o) => {
            let inner = o.inner_type(db);
            let heap = heap_to_string(inner.heap(db));
            let ty_str = type_to_string(db, inner.ty(db));
            format!("@?{}{}", heap, ty_str)
        }
        Type::Result(r) => {
            let inner = r.inner_type(db);
            let heap = heap_to_string(inner.heap(db));
            let ty_str = type_to_string(db, inner.ty(db));
            format!("@!{}{}", heap, ty_str)
        }
        Type::Tensor(t) => {
            let elem = t.element_type(db);
            let heap = heap_to_string(elem.heap(db));
            let ty_str = type_to_string(db, elem.ty(db));
            format!("@tensor<{}{}, {}>", heap, ty_str, t.rank(db))
        }
    }
}
