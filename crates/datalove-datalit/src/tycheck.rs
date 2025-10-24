use rmx::prelude::*;
use bct::text::InternedText;
use std::collections::HashMap;
use crate::ast::*;
use crate::resolve::{ResolvedExpr, Resolution};

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

#[derive(Copy, Clone, Hash, Debug, PartialEq, Eq)]
#[derive(salsa::Update)]
pub enum TensorLayout {
    RowMajor,
    ColMajor,
}

#[salsa::tracked]
pub struct TypeTensor<'db> {
    pub element_type: TypeAndHeap<'db>,
    pub rank: u32,
    pub layout: TensorLayout,
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
}

/// Context for typechecking.
struct TypeContext<'db> {
    db: &'db dyn crate::Db,
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

    TypecheckResult::new(db, expr, root_type, errors)
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
                return Err(TypeError::IntOutOfRange);
            }
        }

        // Rule: Syn-Float - default to f32.
        Expr::Float(_) => Type::F32,

        // Rule: Syn-NamedTuple
        Expr::NamedTuple(t) => {
            let name = t.name(db);
            let resolution = ctx
                .lookup_resolution(name)
                .ok_or_else(|| TypeError::UnresolvedName(name.as_str(db).to_string()))?;

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
                .ok_or_else(|| TypeError::UnresolvedName(name.as_str(db).to_string()))?;

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
                .ok_or_else(|| TypeError::UnresolvedName(name.as_str(db).to_string()))?;

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
                // Cannot synthesize type for empty list.
                return Err(TypeError::CannotSynthesize);
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

            Type::List(TypeList::new(db, first_type))
        }

        // Rule: Syn-Set - synthesize set by synthesizing all elements (must have same type).
        Expr::Set(s) => {
            let elements = s.elements(db);
            if elements.is_empty() {
                // Cannot synthesize type for empty set.
                return Err(TypeError::CannotSynthesize);
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

            Type::Set(TypeSet::new(db, first_type))
        }

        // Rule: Syn-Map - synthesize map by synthesizing all keys and values (must have same types).
        Expr::Map(m) => {
            let entries = m.entries(db);
            if entries.is_empty() {
                // Cannot synthesize type for empty map.
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
                    return Err(TypeError::TypeMismatch {
                        expected: type_to_string(db, first_key_type.ty(db)),
                        actual: type_to_string(db, key_type.ty(db)),
                    });
                }
                if !heaps_compatible(first_key_type.heap(db), key_type.heap(db)) {
                    return Err(TypeError::HeapMismatch {
                        expected_heap: heap_to_string(first_key_type.heap(db)),
                        actual_heap: heap_to_string(key_type.heap(db)),
                    });
                }

                if !types_equivalent(db, first_value_type.ty(db), value_type.ty(db)) {
                    return Err(TypeError::TypeMismatch {
                        expected: type_to_string(db, first_value_type.ty(db)),
                        actual: type_to_string(db, value_type.ty(db)),
                    });
                }
                if !heaps_compatible(first_value_type.heap(db), value_type.heap(db)) {
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
        | Expr::None => return Err(TypeError::CannotSynthesize),

        // Tensor synthesis - TODO: implement proper synthesis.
        Expr::Tensor(_) => todo!("Tensor type synthesis"),

        Expr::ParseError(_) => return Err(TypeError::CannotSynthesize),
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

        // Rule: Check-Subsume - try synthesis first.
        // IMPORTANT: Synthesize from the inner expression without type hint to avoid infinite recursion.
        (Expr::True | Expr::False | Expr::String(_), _) => {
            let expr_without_hint = ExprFull::new(db, None, *expr_and_heap);
            let synthesized = synthesize(ctx, expr_without_hint)?;
            if !types_equivalent(db, synthesized.ty(db), expected_type) {
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
                Err(TypeError::IntOutOfRange)
            }
        }

        (Expr::Int(i), Type::I8) => {
            let value_str = i.value(db).as_str(db);
            if value_str.parse::<i8>().is_ok() {
                Ok(())
            } else {
                Err(TypeError::IntOutOfRange)
            }
        }

        (Expr::Int(i), Type::U16) => {
            let value_str = i.value(db).as_str(db);
            if value_str.parse::<u16>().is_ok() {
                Ok(())
            } else {
                Err(TypeError::IntOutOfRange)
            }
        }

        (Expr::Int(i), Type::I16) => {
            let value_str = i.value(db).as_str(db);
            if value_str.parse::<i16>().is_ok() {
                Ok(())
            } else {
                Err(TypeError::IntOutOfRange)
            }
        }

        (Expr::Int(i), Type::U32) => {
            let value_str = i.value(db).as_str(db);
            if value_str.parse::<u32>().is_ok() {
                Ok(())
            } else {
                Err(TypeError::IntOutOfRange)
            }
        }

        (Expr::Int(i), Type::I32) => {
            let value_str = i.value(db).as_str(db);
            if value_str.parse::<i32>().is_ok() {
                Ok(())
            } else {
                Err(TypeError::IntOutOfRange)
            }
        }

        (Expr::Int(i), Type::U64) => {
            let value_str = i.value(db).as_str(db);
            if value_str.parse::<u64>().is_ok() {
                Ok(())
            } else {
                Err(TypeError::IntOutOfRange)
            }
        }

        (Expr::Int(i), Type::I64) => {
            let value_str = i.value(db).as_str(db);
            if value_str.parse::<i64>().is_ok() {
                Ok(())
            } else {
                Err(TypeError::IntOutOfRange)
            }
        }

        (Expr::Int(_), Type::Int) => Ok(()),

        // Rule: Check-Float
        (Expr::Float(_), Type::F32) => Ok(()),

        // Rule: Check-AnonTuple
        (Expr::AnonTuple(t), Type::AnonTuple(expected_tuple)) => {
            let elements = t.elements(db);
            let expected_fields = expected_tuple.fields(db);

            if elements.len() != expected_fields.len() {
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
                return Err(TypeError::ArityMismatch {
                    expected: expected_fields.len(),
                    actual: fields.len(),
                });
            }

            for (field, expected_field) in fields.iter().zip(expected_fields.iter()) {
                let field_name = field.name(db);
                let expected_name = expected_field.name(db);

                if field_name != expected_name {
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
                return Err(TypeError::ArityMismatch {
                    expected: expected_fields.len(),
                    actual: fields.len(),
                });
            }

            for (field, expected_field) in fields.iter().zip(expected_fields.iter()) {
                let field_name = field.name(db);
                let expected_name = expected_field.name(db);

                if field_name != expected_name {
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
                    TypeError::VariantNotFound(variant_name.as_str(db).to_string())
                })?;

            match (e.payload(db), expected_variant.payload(db)) {
                (Some(payload), Some(expected_payload)) => {
                    check(ctx, payload, expected_payload)
                }
                (None, None) => Ok(()),
                (Some(_), None) => Err(TypeError::TypeMismatch {
                    expected: "no payload".to_string(),
                    actual: "payload".to_string(),
                }),
                (None, Some(_)) => Err(TypeError::TypeMismatch {
                    expected: "payload".to_string(),
                    actual: "no payload".to_string(),
                }),
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
                    TypeError::VariantNotFound(variant_name.as_str(db).to_string())
                })?;

            match (e.payload(db), expected_variant.payload(db)) {
                (Some(payload), Some(expected_payload)) => {
                    check(ctx, payload, expected_payload)
                }
                (None, None) => Ok(()),
                (Some(_), None) => Err(TypeError::TypeMismatch {
                    expected: "no payload".to_string(),
                    actual: "payload".to_string(),
                }),
                (None, Some(_)) => Err(TypeError::TypeMismatch {
                    expected: "payload".to_string(),
                    actual: "no payload".to_string(),
                }),
            }
        }

        // Named enum must match exactly.
        (Expr::NamedEnum(e), Type::NamedEnum(expected_enum)) => {
            let enum_name = e.enum_name(db);
            let expected_name = expected_enum.name(db);

            if enum_name != expected_name {
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
                    TypeError::VariantNotFound(variant_name.as_str(db).to_string())
                })?;

            match (e.payload(db), expected_variant.payload(db)) {
                (Some(payload), Some(expected_payload)) => {
                    check(ctx, payload, expected_payload)
                }
                (None, None) => Ok(()),
                (Some(_), None) => Err(TypeError::TypeMismatch {
                    expected: "no payload".to_string(),
                    actual: "payload".to_string(),
                }),
                (None, Some(_)) => Err(TypeError::TypeMismatch {
                    expected: "payload".to_string(),
                    actual: "no payload".to_string(),
                }),
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

        // Rule: Check-Data
        (Expr::Data(_), Type::Data) => Ok(()),

        // Rule: Check-Error
        (Expr::Err(_), Type::Error) => Ok(()),

        // Rule: Check-NamedTuple (anon tuple -> named tuple coercion)
        (Expr::AnonTuple(t), Type::NamedTuple(expected_tuple)) => {
            let elements = t.elements(db);
            let expected_fields = expected_tuple.fields(db);

            if elements.len() != expected_fields.len() {
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
            } else {
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
            let layout = match t.layout(db) {
                Some(TensorLayoutHint::RowMajor) => TensorLayout::RowMajor,
                Some(TensorLayoutHint::ColMajor) => TensorLayout::ColMajor,
                None => TensorLayout::RowMajor, // Default to row-major
            };
            Type::Tensor(TypeTensor::new(db, element_type, t.rank(db), layout))
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
                && t1.layout(db) == t2.layout(db)
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
            let layout = match t.layout(db) {
                TensorLayout::RowMajor => "",
                TensorLayout::ColMajor => ", col_major",
            };
            format!("@tensor<{}{}, {}{}>", heap, ty_str, t.rank(db), layout)
        }
    }
}
