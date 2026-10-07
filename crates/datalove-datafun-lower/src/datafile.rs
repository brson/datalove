//! The value of a data file, as `require data` binds one.
//!
//! The typechecker has checked the file against its type already, so what is
//! left is reading the literal into a `ConstValue` of that type. The walk is
//! led by the type rather than by the literal: a datalit integer is any
//! integer type until something says which, and here the type says.

use bct::input::Source;
use datalove_datalit as datalit;
use datalove_datalit::ast::{Expr, ExprFull};
use datalove_datafun_ir::{ConstValue, IrType};

use crate::literal::{parse_float_const, parse_hex_const, parse_int_const, string_literal_value};

/// The value of the data file `source`, which checks as `ty`.
///
/// Memoized, because a module's data is asked for wherever it is in scope --
/// before the functions are lowered, by each const evaluation, and by the
/// lowering of the binding itself -- and a dataset is the one constant big
/// enough for reading it again to show.
pub fn data_file_value(db: &dyn salsa::Database, source: Source, ty: &IrType) -> ConstValue {
    data_file_value_tracked(db, source, ty.clone()).clone()
}

#[salsa::tracked(returns(ref))]
fn data_file_value_tracked(db: &dyn salsa::Database, source: Source, ty: IrType) -> ConstValue {
    let expr = datalit::parser::parse(db, source).expr(db);
    let resolved = datalit::resolve::resolve_names(db, source, expr);
    Reader { db, resolved }.read(expr, &ty)
}

struct Reader<'db> {
    db: &'db dyn salsa::Database,
    resolved: datalit::resolve::ResolvedExpr<'db>,
}

impl<'db> Reader<'db> {
    fn read(&self, expr: ExprFull<'db>, ty: &IrType) -> ConstValue {
        let db = self.db;
        let expr_kind = expr.expr(db);
        let text = |value: &bct::text::InternedText<'db>| value.as_str(db).to_owned();
        match (expr_kind, ty) {
            (Expr::Group(group), _) => self.read(group.inner, ty),
            (Expr::Enum(literal), _) => self.read(literal.variant, ty),

            (Expr::True, IrType::Bool) => ConstValue::Bool(true),
            (Expr::False, IrType::Bool) => ConstValue::Bool(false),

            (Expr::Int(int), IrType::F32 | IrType::F64) => parse_float_const(&text(&int.value), ty)
                .expect("the typechecker checked the literal fits"),
            (Expr::Int(int), _) => parse_int_const(&text(&int.value), ty)
                .expect("the typechecker checked the literal fits"),
            (Expr::Float(float), _) => parse_float_const(&text(&float.value), ty)
                .expect("the typechecker checked the literal fits"),
            (Expr::Hex(hex), _) => parse_hex_const(&text(&hex.value), ty)
                .expect("the typechecker checked the literal fits"),
            (Expr::String(string), IrType::String) => {
                ConstValue::String(string_literal_value(string.value.as_str(db)))
            }

            (Expr::AnonTuple(tuple), IrType::Unit) if tuple.elements.is_empty() => ConstValue::Unit,
            (Expr::AnonTuple(tuple), IrType::Tuple(types)) => ConstValue::Tuple(
                tuple.elements.iter().zip(types).map(|(e, t)| self.read(*e, t)).collect(),
            ),
            (Expr::AnonStruct(record), IrType::Struct(fields)) => ConstValue::Struct(
                fields.iter().map(|(name, field_ty)| {
                    let field = record.fields.iter()
                        .find(|f| f.name.as_str(db) == name)
                        .expect("the typechecker checked every field is there");
                    (name.clone(), self.read(field.value, field_ty))
                }).collect(),
            ),

            (Expr::List(list), IrType::List(element)) => ConstValue::List(
                list.elements.iter().map(|e| self.read(*e, element)).collect(),
            ),
            (Expr::Set(set), IrType::Set(element)) => ConstValue::Set(
                set.elements.iter().map(|e| self.read(*e, element)).collect(),
            ),
            (Expr::Map(map), IrType::Map(key, value)) => ConstValue::Map(
                map.entries.iter().map(|entry| (self.read(entry.key, key), self.read(entry.value, value))).collect(),
            ),
            (Expr::Tensor(tensor), IrType::Tensor(element, _)) => ConstValue::Tensor {
                shape: tensor.shape.clone(),
                elements: tensor.elements.iter().map(|e| self.read(*e, element)).collect(),
            },
            (Expr::Table(table), IrType::Table(columns)) => ConstValue::Table {
                columns: columns.iter().map(|(name, _)| name.clone()).collect(),
                rows: table.rows.iter().map(|row| {
                    row.elements.iter().zip(columns)
                        .map(|(e, (_, column_ty))| self.read(*e, column_ty))
                        .collect()
                }).collect(),
            },

            (Expr::None, IrType::Option(_)) => ConstValue::OptionNone,
            (Expr::Some(some), IrType::Option(inner)) => {
                ConstValue::OptionSome(Box::new(self.read(some.payload, inner)))
            }
            (Expr::Ok(ok), IrType::Result(inner)) => ConstValue::ResultOk(Box::new(self.read(ok.payload, inner))),
            // The payload of an `er` is an `error`, and is the error in the slot.
            (Expr::Er(er), IrType::Result(_)) => ConstValue::ResultErr(Box::new(self.read(er.payload, &IrType::Error))),
            (Expr::Data(data), IrType::Data) => {
                let (payload_type, value) = self.read_boxed(data.value);
                ConstValue::Data { payload_type, value }
            }
            (Expr::Error(error), IrType::Error) => {
                let (payload_type, value) = self.read_boxed(error.value);
                ConstValue::Error { payload_type, value }
            }

            (Expr::Atom(atom), IrType::Atom(_) | IrType::Enum(_)) => {
                ConstValue::Enum { variant: text(&atom.name), payload: None }
            }
            (Expr::Term(term), IrType::Term(_, payload)) => ConstValue::Enum {
                variant: text(&term.name),
                payload: Some(Box::new(self.read(term.payload, payload))),
            },
            (Expr::Term(term), IrType::Enum(variants)) => {
                let name = text(&term.name);
                let payload = variants.iter()
                    .find(|(variant, _)| *variant == name)
                    .and_then(|(_, payload)| payload.as_ref())
                    .expect("the typechecker checked the variant carries a payload");
                let payload = self.read(term.payload, payload);
                ConstValue::Enum { variant: name, payload: Some(Box::new(payload)) }
            }

            (_, ty) => unreachable!("data that checked as `{ty:?}` has a literal of another shape"),
        }
    }

    /// A `data` or `error` payload, which carries its own type: the one it
    /// synthesizes, as it was checked.
    fn read_boxed(&self, expr: ExprFull<'db>) -> (Box<IrType>, Box<ConstValue>) {
        let checked = datalit::tycheck::type_check(self.db, expr, self.resolved);
        let ty = checked.root_type(self.db).as_ref()
            .expect("the typechecker checked the payload");
        let ty = IrType::from_datalit(self.db, ty);
        let value = self.read(expr, &ty);
        (Box::new(ty), Box::new(value))
    }
}
