//! Data files, as `require data` binds them.
//!
//! `require data lib/pkg/name: T` is the const `name: T` whose value is the
//! `.dlt` file at that path, so what is checked here is that file against `T`,
//! or, with no `T`, what type the file is of. Datalit's own checker does the
//! work: the file is a datalit expression, and the types are the same types.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;

use bct::input::Source;
use datalove_datafun_ast::ast::{ExprDataFile, ExprFun, ExprKey};
use datalove_datalit as datalit;
use datalove_datalit::tycheck::TypeError as DatalitTypeError;

use crate::context::TypeContext;
use crate::{ErrorSite, Type, TypeError};

/// A data file, and what it is expected to be.
#[salsa::interned]
pub struct DataFileQuery<'db> {
    pub source: Source,
    #[returns(ref)]
    pub expected: Option<datalit::tycheck::Type<'db>>,
}

/// The type of a data file, checked against what it is expected to be.
///
/// Memoized on the source, so a file required twice, or by an unchanged module
/// after an edit elsewhere, is read once.
#[salsa::tracked(returns(ref))]
pub fn data_file_type<'db>(
    db: &'db dyn crate::Db,
    query: DataFileQuery<'db>,
) -> Result<datalit::tycheck::Type<'db>, Vec<String>> {
    let source = *query.source(db);
    let expr = datalit::parser::parse(db, source).expr(db);
    let resolved = datalit::resolve::resolve_names(db, source, expr.clone());
    let result = datalit::tycheck::type_check_with_expected(db, expr, resolved, query.expected(db).clone());
    let errors: Vec<String> = result.errors(db).iter()
        .map(|entry| describe(&entry.error(db)))
        .collect();
    match result.root_type(db) {
        Some(ty) if errors.is_empty() => Ok(ty.C()),
        _ => Err(errors),
    }
}

/// What a datalit type error says, as a sentence.
fn describe(error: &DatalitTypeError) -> String {
    match error {
        DatalitTypeError::TypeMismatch { expected, actual } => fmt!("expected `{expected}`, found `{actual}`"),
        DatalitTypeError::CannotSynthesize => S("cannot work out the type of a value; give the data a type"),
        DatalitTypeError::MissingField(field) => fmt!("a struct is missing the field `{field}`"),
        DatalitTypeError::ExtraField(field) => fmt!("a struct has the field `{field}`, which the type does not"),
        DatalitTypeError::FieldOrderMismatch => S("a struct's fields are not in the order the type gives them"),
        DatalitTypeError::IntOutOfRange => S("an integer is out of range for its type"),
        DatalitTypeError::VariantNotFound(variant) => fmt!("the type has no variant `{variant}`"),
        DatalitTypeError::ArityMismatch { expected, actual } => {
            fmt!("expected {expected} elements, found {actual}")
        }
    }
}

/// Check a data file against the type expected of it, or work out its type
/// when nothing is expected, and record it as the expression's type.
pub(crate) fn type_data_file<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    file: &ExprDataFile<'db>,
    expected: Option<&Type<'db>>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    let path = file.path(db);
    let site = ErrorSite::Expr(ExprKey::of(db, expr));

    let Some(source) = ctx.data_files.get(&path).copied() else {
        let message = fmt!("no data `{path}` to require");
        ctx.push_coded(site, "F079", message.C(), S("no such data"), None);
        return Err(TypeError::DatalitError(message));
    };

    let expected = match expected {
        None => None,
        Some(Type::Datalit(ty)) => Some(ty.C()),
        Some(Type::Function(_)) => {
            let message = fmt!("data `{path}` cannot be a function");
            ctx.push_coded(site, "F084", message.C(), S("data is a value"), None);
            return Err(TypeError::DatalitError(message));
        }
    };

    ctx.resolved_data.insert(path.C(), source);
    let query = DataFileQuery::new(db, source, expected.C());
    match data_file_type(db, query) {
        Ok(ty) => {
            let ty = Type::Datalit(ty.C());
            ctx.store_expr_type(expr, &ty);
            Ok(ty)
        }
        Err(errors) => {
            let message = match &expected {
                Some(ty) => fmt!(
                    "data `{path}` is not a `{}`",
                    datalit::tycheck::type_to_string(db, ty),
                ),
                None => fmt!("data `{path}` has no type of its own"),
            };
            let label = errors.first().cloned().unwrap_or_else(|| S("the data does not check"));
            let note = (errors.len() > 1).then(|| errors[1..].join("; "));
            ctx.push_coded(site, "F084", message.C(), label, note);
            Err(TypeError::DatalitError(message))
        }
    }
}

/// The data files of a world, by path, for a context to resolve paths against.
pub type DataFiles = BTreeMap<String, Source>;
