//! The standard library's functions and the intrinsics, as the generator
//! can call them.
//!
//! Read from the modules' own sources, so that what is generated follows the
//! library as it changes: a function the generator can call is one of the
//! modules' top-level `fun`s, not generic, whose parameters and answer are
//! all of the types read here -- the scalars, a string, and an option, a
//! result or a list of one. Each is called qualified by its module, as
//! `u8.add_checked(a, b)`, so that the many functions of one name across the
//! modules never meet.

use rmx::prelude::*;
use datalove_datalit::ast::{TypeHint, TypeHintList, TypeHintOption, TypeHintResult};

use datalove_datafun_intrinsics::{INTRINSICS, IntrinsicType};

use crate::context::{FunctionSig, ModuleInfo, Param, ParamMode};

/// The modules read, by name, and their sources.
///
/// Not `list`, `map`, `set`, `option`, `result`, `tensor` and `mem`, whose
/// functions are generic, nor `fixedint`, `float` and `ord`, which declare
/// the bounds.
const SOURCES: &[(&str, &str)] = &[
    ("bool", include_str!("../../../sys/std/bool.dfm")),
    ("u8", include_str!("../../../sys/std/u8.dfm")),
    ("u16", include_str!("../../../sys/std/u16.dfm")),
    ("u32", include_str!("../../../sys/std/u32.dfm")),
    ("u64", include_str!("../../../sys/std/u64.dfm")),
    ("i8", include_str!("../../../sys/std/i8.dfm")),
    ("i16", include_str!("../../../sys/std/i16.dfm")),
    ("i32", include_str!("../../../sys/std/i32.dfm")),
    ("i64", include_str!("../../../sys/std/i64.dfm")),
    ("index", include_str!("../../../sys/std/index.dfm")),
    ("offset", include_str!("../../../sys/std/offset.dfm")),
    ("int", include_str!("../../../sys/std/int.dfm")),
    ("f32", include_str!("../../../sys/std/f32.dfm")),
    ("f64", include_str!("../../../sys/std/f64.dfm")),
    ("string", include_str!("../../../sys/std/string.dfm")),
];

/// Functions not called, by module and name: each makes something as large
/// as an argument says, or takes as long, and an argument may be in the
/// billions. `int.factorial` of one did not finish in two minutes.
const NOT_CALLED: &[(&str, &str)] = &[("int", "factorial"), ("int", "pow"), ("string", "repeat")];

/// Every module read, with the functions of it the generator can call.
pub fn std_modules<'db>() -> Vec<ModuleInfo<'db>> {
    SOURCES.iter()
        .map(|(name, source)| ModuleInfo {
            library: "sys".S(),
            package: "std".S(),
            module: name.S(),
            functions: source.lines()
                .filter_map(signature)
                .filter(|f| !NOT_CALLED.contains(&(*name, f.name.as_str())))
                .collect(),
            type_aliases: Vec::new(),
            enums: Vec::new(),
            consts: Vec::new(),
            generics: Vec::new(),
        })
        .collect()
}

/// Every intrinsic, as a function named `icall name`, which a call writes as
/// `icall name(args)`.
///
/// But the remainders: a remainder by zero is not defined for an intrinsic,
/// which `sys/std` checks for before it calls one.
pub fn intrinsic_functions<'db>() -> Vec<FunctionSig<'db>> {
    INTRINSICS.iter()
        .filter(|def| !def.name.starts_with("rem_") && !def.name.starts_with("srem_"))
        .map(|def| FunctionSig {
            name: format!("icall {}", def.name),
            params: def.params.iter().enumerate()
                .map(|(i, ty)| Param { name: format!("p{i}"), type_hint: intrinsic_type(*ty), mode: ParamMode::In })
                .collect(),
            return_type: Some(intrinsic_type(def.ret)),
        })
        .collect()
}

fn intrinsic_type<'db>(ty: IntrinsicType) -> TypeHint<'db> {
    match ty {
        IntrinsicType::U8 => TypeHint::U8,
        IntrinsicType::I8 => TypeHint::I8,
        IntrinsicType::U16 => TypeHint::U16,
        IntrinsicType::I16 => TypeHint::I16,
        IntrinsicType::U32 => TypeHint::U32,
        IntrinsicType::I32 => TypeHint::I32,
        IntrinsicType::U64 => TypeHint::U64,
        IntrinsicType::I64 => TypeHint::I64,
        IntrinsicType::Index => TypeHint::Index,
        IntrinsicType::Offset => TypeHint::Offset,
        IntrinsicType::F32 => TypeHint::F32,
        IntrinsicType::F64 => TypeHint::F64,
        IntrinsicType::Bool => TypeHint::Bool,
    }
}

/// The signature a line declares, if it begins a function the generator can
/// call.
fn signature<'db>(line: &str) -> Option<FunctionSig<'db>> {
    let rest = line.strip_prefix("fun ")?;
    let (name, rest) = rest.split_once('(')?;
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let (params, rest) = rest.split_once(')')?;
    let return_type = match rest.trim() {
        "" => None,
        ret => Some(type_hint(ret.strip_prefix(':')?.trim())?),
    };
    let params = match params.trim() {
        "" => Vec::new(),
        params => params.split(',').map(param).collect::<Option<Vec<_>>>()?,
    };
    Some(FunctionSig { name: name.S(), params, return_type })
}

/// A parameter, `mode name: type`.
fn param<'db>(text: &str) -> Option<Param<'db>> {
    let (name, ty) = text.split_once(':')?;
    let (mode, name) = match name.trim().split_once(' ') {
        Some(("ref", name)) => (ParamMode::Ref, name),
        Some(("mut", name)) => (ParamMode::Mut, name),
        Some(("out", name)) => (ParamMode::Out, name),
        Some(_) => return None,
        None => (ParamMode::In, name.trim()),
    };
    Some(Param { name: name.S(), type_hint: type_hint(ty.trim())?, mode })
}

/// The type `text` names, of those read here.
fn type_hint<'db>(text: &str) -> Option<TypeHint<'db>> {
    if let Some(inner) = text.strip_prefix('?') {
        return Some(TypeHint::Option(TypeHintOption { inner_type: Box::new(type_hint(inner)?) }));
    }
    if let Some(inner) = text.strip_prefix('!') {
        return Some(TypeHint::Result(TypeHintResult { inner_type: Box::new(type_hint(inner)?) }));
    }
    if let Some(inner) = text.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        return Some(TypeHint::List(TypeHintList { element_type: Box::new(type_hint(inner)?) }));
    }
    Some(match text {
        "bool" => TypeHint::Bool,
        "u8" => TypeHint::U8,
        "u16" => TypeHint::U16,
        "u32" => TypeHint::U32,
        "u64" => TypeHint::U64,
        "i8" => TypeHint::I8,
        "i16" => TypeHint::I16,
        "i32" => TypeHint::I32,
        "i64" => TypeHint::I64,
        "index" => TypeHint::Index,
        "offset" => TypeHint::Offset,
        "int" => TypeHint::Int,
        "f32" => TypeHint::F32,
        "f64" => TypeHint::F64,
        "string" => TypeHint::String,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Most of the library is read, and what is read is what a line says.
    #[test]
    fn reads_the_library() {
        let modules = std_modules();
        let count: usize = modules.iter().map(|m| m.functions.len()).sum();
        assert!(count > 500, "only {count} functions read");
        let string = modules.iter().find(|m| m.module == "string").X();
        let slice = string.functions.iter().find(|f| f.name == "slice").X();
        assert_eq!(slice.params.len(), 3);
        assert_eq!(slice.params[0].mode, ParamMode::Ref);
        assert!(matches!(slice.return_type, Some(TypeHint::Option(_))));
        assert!(string.functions.iter().all(|f| f.name != "repeat"));
    }
}
