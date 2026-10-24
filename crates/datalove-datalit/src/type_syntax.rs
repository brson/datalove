//! Types as datalove writes them.
//!
//! A type is held in several forms -- as written (`TypeHint`), as checked
//! (`Type`), half inferred (`PType`), and with datafun's functions beside them
//! -- and every one of them is shown to a reader in the same syntax. So each
//! maps itself onto a [`TypeSyntax`], and its `Display` here is the one place
//! that syntax is spelled out. A new kind of type is a new variant here, which
//! each form's mapping has to say how to reach before it compiles.

use rmx::prelude::*;
use std::borrow::Cow;
use std::fmt;

/// A type, shaped as its syntax is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeSyntax<'a> {
    /// A type written as a word: a primitive, an alias, a type parameter, or
    /// `_` for one nothing has decided yet.
    Word(Cow<'a, str>),
    /// `(A, B)`; `()` is the unit, and `(A,)` the tuple of one.
    Tuple(Vec<TypeSyntax<'a>>),
    /// `{ x: A, y: B }`.
    Struct(Vec<(Cow<'a, str>, TypeSyntax<'a>)>),
    /// `{| x: A, y: B |}`.
    Table(Vec<(Cow<'a, str>, TypeSyntax<'a>)>),
    /// `[A]`.
    List(Box<TypeSyntax<'a>>),
    /// `#{A}`.
    Set(Box<TypeSyntax<'a>>),
    /// `%{K = V}`.
    Map(Box<TypeSyntax<'a>>, Box<TypeSyntax<'a>>),
    /// `?A`.
    Option(Box<TypeSyntax<'a>>),
    /// `!A`.
    Result(Box<TypeSyntax<'a>>),
    /// `[|A, N|]`.
    Tensor(Box<TypeSyntax<'a>>, u32),
    /// `atom Name`.
    Atom(Cow<'a, str>),
    /// `term Name A`.
    Term(Cow<'a, str>, Box<TypeSyntax<'a>>),
    /// `enum { atom A, term B T }`: each variant, with its payload if it is
    /// a term.
    Enum(Vec<(Cow<'a, str>, Option<TypeSyntax<'a>>)>),
    /// A function, which has no type syntax of its own, written as a
    /// function's head is: `fun(A, B): R`.
    Function(Vec<TypeSyntax<'a>>, Box<TypeSyntax<'a>>),
}

impl<'a> TypeSyntax<'a> {
    pub fn word(word: impl Into<Cow<'a, str>>) -> Self {
        TypeSyntax::Word(word.into())
    }

    /// The unit, `()`.
    pub fn unit() -> Self {
        TypeSyntax::Tuple(Vec::new())
    }
}

impl fmt::Display for TypeSyntax<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeSyntax::Word(word) => f.write_str(word),
            TypeSyntax::Tuple(fields) => match fields.as_slice() {
                [only] => write!(f, "({only},)"),
                _ => write!(f, "({})", Joined(fields)),
            },
            TypeSyntax::Struct(fields) if fields.is_empty() => f.write_str("{}"),
            TypeSyntax::Struct(fields) => write!(f, "{{ {} }}", Fields(fields)),
            TypeSyntax::Table(columns) if columns.is_empty() => f.write_str("{||}"),
            TypeSyntax::Table(columns) => write!(f, "{{| {} |}}", Fields(columns)),
            TypeSyntax::List(elem) => write!(f, "[{elem}]"),
            TypeSyntax::Set(elem) => write!(f, "#{{{elem}}}"),
            TypeSyntax::Map(key, value) => write!(f, "%{{{key} = {value}}}"),
            TypeSyntax::Option(inner) => write!(f, "?{inner}"),
            TypeSyntax::Result(inner) => write!(f, "!{inner}"),
            TypeSyntax::Tensor(elem, rank) => write!(f, "[|{elem}, {rank}|]"),
            TypeSyntax::Atom(name) => write!(f, "atom {name}"),
            TypeSyntax::Term(name, payload) => write!(f, "term {name} {payload}"),
            TypeSyntax::Enum(variants) if variants.is_empty() => f.write_str("enum {}"),
            TypeSyntax::Enum(variants) => {
                f.write_str("enum { ")?;
                for (index, (name, payload)) in variants.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    match payload {
                        Some(payload) => write!(f, "term {name} {payload}")?,
                        None => write!(f, "atom {name}")?,
                    }
                }
                f.write_str(" }")
            }
            TypeSyntax::Function(params, ret) => write!(f, "fun({}): {ret}", Joined(params)),
        }
    }
}

/// Types, separated by commas.
struct Joined<'s, 'a>(&'s [TypeSyntax<'a>]);

impl fmt::Display for Joined<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, ty) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{ty}")?;
        }
        Ok(())
    }
}

/// Named fields, `name: type`, separated by commas.
struct Fields<'s, 'a>(&'s [(Cow<'a, str>, TypeSyntax<'a>)]);

impl fmt::Display for Fields<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, (name, ty)) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{name}: {ty}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(w: &'static str) -> TypeSyntax<'static> {
        TypeSyntax::word(w)
    }

    /// Each form is written as the spec writes it.
    #[test]
    fn each_form_is_written_as_the_spec_has_it() {
        let cases = [
            (TypeSyntax::unit(), "()"),
            (TypeSyntax::Tuple(vec![word("int")]), "(int,)"),
            (TypeSyntax::Tuple(vec![word("int"), word("string")]), "(int, string)"),
            (TypeSyntax::Struct(vec![("x".into(), word("f32")), ("y".into(), word("f32"))]), "{ x: f32, y: f32 }"),
            (TypeSyntax::Struct(vec![]), "{}"),
            (TypeSyntax::Table(vec![("a".into(), word("u32"))]), "{| a: u32 |}"),
            (TypeSyntax::List(Box::new(word("int"))), "[int]"),
            (TypeSyntax::Set(Box::new(word("int"))), "#{int}"),
            (TypeSyntax::Map(Box::new(word("string")), Box::new(word("int"))), "%{string = int}"),
            (TypeSyntax::Option(Box::new(TypeSyntax::List(Box::new(word("u8"))))), "?[u8]"),
            (TypeSyntax::Result(Box::new(TypeSyntax::unit())), "!()"),
            (TypeSyntax::Tensor(Box::new(word("i32")), 2), "[|i32, 2|]"),
            (TypeSyntax::Atom("Red".into()), "atom Red"),
            (TypeSyntax::Term("Foo".into(), Box::new(word("int"))), "term Foo int"),
            (TypeSyntax::Enum(vec![("Red".into(), None), ("Custom".into(), Some(word("string")))]),
                "enum { atom Red, term Custom string }"),
            (TypeSyntax::Function(vec![word("int"), word("string")], Box::new(word("bool"))), "fun(int, string): bool"),
        ];
        for (syntax, written) in cases {
            assert_eq!(syntax.to_string(), written);
        }
    }

    /// What is printed is datalove: each type, written as the printer writes
    /// it, parses with no complaint, and prints back the same. A form the
    /// parser does not read as the printer writes it fails here, so the two
    /// cannot drift apart.
    #[test]
    fn what_is_printed_parses_back_the_same() {
        let db = crate::Database::default();
        let types = [
            "()", "(int,)", "(int, string)", "((u8,), ())",
            "{ x: f32, y: f32 }", "{}", "{| a: u32, b: string |}",
            "[int]", "[[u8]]", "#{string}", "%{string = [int]}",
            "?int", "??int", "![u8]", "!()", "?!int", "[|i32, 2|]",
            "atom Red", "term Foo int", "term Foo ?(int, int)",
            "enum { atom Red, term Custom string }",
            "data", "error", "index", "offset", "bool",
            "Point", "[Point]",
        ];
        for written in types {
            let source = bct::input::Source::new(&db, fmt!(": {written} / 0"));
            let parsed = crate::parser::parse_for_test(&db, source);
            let errors = crate::parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(&db, source);
            assert!(errors.is_empty(), "{written} does not parse");
            let hint = parsed.type_hint().unwrap_or_else(|| panic!("{written} is not read as a hint"));
            assert_eq!(hint.syntax(&db).to_string(), written);
        }
    }
}
