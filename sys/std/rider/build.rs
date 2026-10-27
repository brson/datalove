//! Generates the table of native functions this rider exports.
//!
//! The names come from `rider.dli` beside this file, the rider's interface: every `native fun`
//! declared there has a `dlr_std__`-prefixed function in `src/lib.rs`. A
//! datalove binary that links this crate reads the generated table to find
//! those functions by linker symbol, the same names the compiler emits calls
//! to. A declared function that is not implemented fails to compile here.
//!
//! Beside the table, each function's type as the interface declares it: a
//! constant of that function-pointer type, initialized with the function. The
//! compiler holds the two to agreeing, so a Rust signature that disagrees with
//! its declaration -- a parameter left out, a string where the declaration has
//! a list -- is a build error rather than a call reading the wrong memory.

use std::fmt::Write as _;
use std::path::Path;

const INTERFACE: &str = "rider.dli";

fn main() {
    println!("cargo:rerun-if-changed={INTERFACE}");

    let source = std::fs::read_to_string(INTERFACE)
        .unwrap_or_else(|e| panic!("unable to read {INTERFACE}: {e}"));

    let mut table = String::new();
    table.push_str(
        "/// Address of every function this rider exports, by linker symbol.\n\
         ///\n\
         /// Generated from `rider.dli` by `build.rs`.\n\
         pub fn symbols() -> Vec<(&'static str, *const ())> {\n\
         \x20   vec![\n",
    );

    let decls: Vec<Decl> = source.lines().filter_map(Decl::parse).collect();
    for decl in &decls {
        writeln!(
            table,
            "        (\"dlr_std__{name}\", dlr_std__{name} as *const ()),",
            name = decl.name,
        )
        .expect("writing to a string");
    }

    table.push_str("    ]\n}\n");

    table.push_str("\n// Each function's type as `rider.dli` declares it; see `build.rs`.\n");
    for decl in &decls {
        writeln!(
            table,
            "const _: unsafe extern \"C-unwind\" fn({}) -> u8 = dlr_std__{};",
            decl.rust_params().join(", "),
            decl.name,
        )
        .expect("writing to a string");
    }

    let out = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("symbols.rs");
    std::fs::write(&out, table)
        .unwrap_or_else(|e| panic!("unable to write {}: {e}", out.display()));
}

/// A `native fun` declaration: `native fun name<T, ...>(params): ret where {...}`.
struct Decl<'a> {
    name: &'a str,
    type_params: Vec<&'a str>,
    /// Each parameter's mode (`ref`, `mut`, `out` or `in`) and type.
    params: Vec<(&'a str, &'a str)>,
    /// `None` for a function returning nothing.
    ret: Option<&'a str>,
}

impl<'a> Decl<'a> {
    fn parse(line: &'a str) -> Option<Decl<'a>> {
        let rest = line.trim().strip_prefix("native fun ")?;
        let open = rest
            .find('(')
            .unwrap_or_else(|| panic!("native fun declaration has no parameters: {rest}"));
        let (name, type_params) = match rest[..open].split_once('<') {
            Some((name, tps)) => (name.trim(), split_top(tps.trim_end().trim_end_matches('>'))),
            None => (rest[..open].trim(), Vec::new()),
        };
        let close = matching_paren(rest, open);
        let params = split_top(&rest[open + 1..close])
            .into_iter()
            .map(|param| {
                let (head, ty) = param.split_once(':').unwrap_or_else(|| panic!("parameter without a type: {param}"));
                let mode = match head.split_whitespace().collect::<Vec<_>>()[..] {
                    [mode, _] => mode,
                    [_] => "in",
                    _ => panic!("unexpected parameter: {param}"),
                };
                (mode, ty.trim())
            })
            .collect();
        let after = rest[close + 1..].trim();
        let ret = after.strip_prefix(':').map(|r| r.split(" where ").next().expect("split yields one").trim());
        Some(Decl { name, type_params, params, ret })
    }

    /// The Rust parameter types of the function implementing this.
    ///
    /// The runtime handle; each argument and its descriptor; the result and
    /// its descriptor; then a descriptor for each type parameter no argument
    /// determines, which appears in the result alone. See
    /// `botdocs/native-abi.md`.
    ///
    /// An argument Rust can name the type of is a reference: shared for `ref`
    /// and for a scalar, exclusive for `mut` and for a consumed aggregate,
    /// which is handed over to be taken. The ownership checker makes the
    /// exclusive ones sound, rejecting a call that passes one value twice
    /// where either is `mut` (D010). A result is a `&mut MaybeUninit`, being
    /// written rather than read. A value at a type parameter, an option, a
    /// tuple and nothing at all have no Rust type, so they are raw pointers.
    fn rust_params(&self) -> Vec<String> {
        const TD: &str = "&rtdt::TyDesc";
        let mut out = vec!["*mut u8".to_string()];
        for &(mode, ty) in &self.params {
            let exclusive = mode == "mut" || mode == "out" || (mode == "in" && !is_scalar(ty));
            out.push(match rust_type(ty) {
                Some(t) if exclusive => format!("&mut {t}"),
                Some(t) => format!("&{t}"),
                None if exclusive => "*mut u8".to_string(),
                None => "*const u8".to_string(),
            });
            out.push(TD.to_string());
        }
        out.push(match self.ret.and_then(rust_type) {
            Some(t) => format!("&mut std::mem::MaybeUninit<{t}>"),
            None => "*mut u8".to_string(),
        });
        out.push(TD.to_string());
        for tp in &self.type_params {
            let in_params = self.params.iter().any(|(_, ty)| mentions(ty, tp));
            if !in_params {
                out.push(TD.to_string());
            }
        }
        out
    }
}

/// The Rust type of a value of datalove type `ty`, if Rust can name one.
///
/// A type parameter, an option, a tuple: nothing Rust can name, read
/// through the descriptor instead.
fn rust_type(ty: &str) -> Option<&'static str> {
    Some(match ty {
        "string" => "rtdt::String",
        "index" => "rtdt::Index",
        "offset" => "rtdt::Offset",
        "int" => "rtdt::Int",
        "f64" => "f64",
        "f32" => "f32",
        "i8" => "i8",
        "i16" => "i16",
        "i32" => "i32",
        "i64" => "i64",
        "u16" => "u16",
        "u32" => "u32",
        "u64" => "u64",
        "u8" | "bool" => "u8",
        _ if ty.starts_with("[|") => "rtdt::Tensor",
        _ if ty.starts_with('[') => "rtdt::List",
        _ if ty.starts_with("#{") => "rtdt::Set",
        _ if ty.starts_with("%{") => "rtdt::Map",
        _ => return None,
    })
}

fn is_scalar(ty: &str) -> bool {
    matches!(ty, "index" | "offset" | "f64" | "f32" | "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "bool")
}

/// Whether type `ty` mentions the type parameter `tp`.
fn mentions(ty: &str, tp: &str) -> bool {
    ty.split(|c: char| !c.is_alphanumeric() && c != '_').any(|word| word == tp)
}

/// Split at the commas not inside brackets.
fn split_top(s: &str) -> Vec<&str> {
    let (mut parts, mut depth, mut start) = (Vec::new(), 0, 0);
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(s[start..].trim());
    parts.retain(|p| !p.is_empty());
    parts
}

/// The index of the parenthesis closing the one at `open`.
fn matching_paren(s: &str, open: usize) -> usize {
    let mut depth = 0;
    for (i, c) in s[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return open + i;
                }
            }
            _ => {}
        }
    }
    panic!("unclosed parenthesis: {s}")
}
