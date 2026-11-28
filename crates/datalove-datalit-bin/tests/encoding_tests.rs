//! The binary encoding held to the runtime: values read from datalit text,
//! encoded, decoded, and compared with the runtime's own total order.

use rmx::prelude::*;
use bct::random::SeededRng;

use datalove_datalit::ast::ExprFull;
use datalove_datalit::ast_gen::{self, AstGenConfig, TypeWeights};
use datalove_datalit::tydesc_table::TyDescTable;
use datalove_datalit::{Database, instantiate2};
use datalove_datalit_bin as bin;
use datalove_diagnostic::{ParseDiagnostic, TypeDiagnostic};
use datalove_rt::c::{LocalRtHandle, RtOrdering};
use datalove_rt::rust::Runtime;
use datalove_rtdt as rtdt;
use datalove_rtdt::{TyDescRef, TyTag};

/// A value the test owns on the runtime heap.
struct Val {
    ptr: *mut u8,
    td: *const rtdt::TyDesc,
}

struct Fixture<'db> {
    db: &'db Database,
    table: TyDescTable<'db>,
    arena: bin::TyArena,
    // Last, so that the values are gone before it shuts down.
    rt: Runtime,
}

impl<'db> Fixture<'db> {
    fn new(db: &'db Database) -> Fixture<'db> {
        Fixture { db, table: TyDescTable::new(db), arena: bin::TyArena::new(), rt: Runtime::new() }
    }

    fn h(&self) -> LocalRtHandle {
        self.rt.handle()
    }

    /// The value `text` writes, if it is a well-typed datalit document the
    /// runtime can make.
    fn load(&mut self, text: &str) -> Option<Val> {
        let db = self.db;
        let source = bct::input::Source::new(db, text.S());
        if !datalove_datalit::parser::parse_integration_test::accumulated::<ParseDiagnostic>(db, source).is_empty() {
            return None;
        }
        let parsed = datalove_datalit::parser::parse_integration_test(db, source);
        let resolved = datalove_datalit::resolve::resolve_names(db, source, parsed.C());
        let checked = datalove_datalit::tycheck::type_check(db, parsed.C(), resolved);
        if !datalove_datalit::tycheck::type_check::accumulated::<TypeDiagnostic>(db, parsed, resolved).is_empty()
            || checked.root_type(db).is_none()
        {
            return None;
        }
        let inst = instantiate2::instantiate_value(db, self.rt.handle(), &mut self.table, checked).ok()?;
        Some(Val { ptr: inst.ptr as *mut u8, td: inst.tydesc.as_ptr() })
    }

    fn must_load(&mut self, text: &str) -> Val {
        self.load(text).unwrap_or_else(|| panic!("`{text}` loads"))
    }

    fn encode(&self, v: &Val) -> Vec<u8> {
        let mut out = Vec::new();
        unsafe { bin::encode(self.h(), td(v.td), v.ptr, &mut out) };
        out
    }

    fn decode(&mut self, td_ptr: *const rtdt::TyDesc, bytes: &[u8]) -> Result<Val, bin::DecodeError> {
        let rt = self.rt.handle();
        unsafe {
            let ptr = datalove_rt::c::dtlv_rti_mem_alloc_local(rt, &*td_ptr, 1);
            match bin::decode(rt, &mut self.arena, bytes, td(td_ptr), rtdt::AnyOut::new(ptr)) {
                Ok(()) => Ok(Val { ptr, td: td_ptr }),
                Err(e) => {
                    datalove_rt::c::dtlv_rti_mem_free_local(rt, &*td_ptr, 1, ptr);
                    Err(e)
                }
            }
        }
    }

    fn cmp(&self, a: &Val, b: &Val) -> RtOrdering {
        unsafe {
            datalove_rt::c::dtlv_rti_cmp_total_local(
                self.h(), rtdt::AnyRef::new(a.ptr), &*a.td, rtdt::AnyRef::new(b.ptr), &*b.td)
        }
    }

    fn drop_val(&self, v: Val) {
        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(self.h(), rtdt::AnyOwned::new(v.ptr), &*v.td);
            datalove_rt::c::dtlv_rti_mem_free_local(self.h(), &*v.td, 1, v.ptr);
        }
    }

    fn data_td(&mut self) -> *const rtdt::TyDesc {
        self.arena.scalar(TyTag::Data)
    }

    /// Check that a value survives the trip both ways: decoded, it is the
    /// value, and encoded again, the same bytes. Likewise as a `data`, and its
    /// type's encoding read back to a descriptor of the same type.
    fn round_trip(&mut self, text: &str, v: &Val) -> Vec<u8> {
        let bytes = self.encode(v);
        let back = self.decode(v.td, &bytes).unwrap_or_else(|e| panic!("`{text}` decodes: {e}"));
        assert_eq!(self.cmp(v, &back), RtOrdering::Equal, "`{text}` decodes to itself");
        assert_eq!(self.encode(&back), bytes, "`{text}` encodes again alike");
        self.drop_val(back);

        let mut data_bytes = Vec::new();
        unsafe { bin::encode_data(self.h(), td(v.td), v.ptr, &mut data_bytes) };
        let data_td = self.data_td();
        let back = self.decode(data_td, &data_bytes).unwrap_or_else(|e| panic!("`{text}` as data decodes: {e}"));
        assert_eq!(self.encode(&back), data_bytes, "`{text}` as data encodes again alike");
        self.drop_val(back);

        let type_bytes = bin::type_bytes(td(v.td));
        let read = self.arena.type_from_bytes(&type_bytes).unwrap_or_else(|e| panic!("`{text}`'s type decodes: {e}"));
        assert!(rtdt::tyenc::same_type(td(v.td), td(read)), "`{text}`'s type reads back as itself");
        if let Err(e) = bin::layouts_agree(td(v.td), td(read)) {
            panic!("`{text}`: datalit's descriptor and the arena's lay it out apart: {e}");
        }
        bytes
    }
}

fn td<'a>(p: *const rtdt::TyDesc) -> TyDescRef<'a> {
    unsafe { TyDescRef::from_ptr(p) }
}

fn rt_ord(o: std::cmp::Ordering) -> RtOrdering {
    match o {
        std::cmp::Ordering::Less => RtOrdering::Less,
        std::cmp::Ordering::Equal => RtOrdering::Equal,
        std::cmp::Ordering::Greater => RtOrdering::Greater,
    }
}

/// A value of every type, and the corners of each.
const VALUES: &[&str] = &[
    "true",
    "false",
    ": u8 / 255",
    ": i8 / -128",
    ": u16 / 65535",
    ": i16 / -300",
    ": u32 / 4000000000",
    ": i32 / -5",
    ": u64 / 18446744073709551615",
    ": i64 / -9223372036854775808",
    ": index / 7",
    ": offset / -7",
    ": f32 / 1.5",
    "-0.0",
    ": f64 / 0x7FF8000000000001",
    ": f64 / 0xFFF0000000000000",
    "0",
    "-1",
    "123456789012345678901234567890",
    "-123456789012345678901234567890",
    r#""""#,
    r#""a\0b""#,
    r#""héllo""#,
    "()",
    "(1,)",
    r#"(1, "a", true)"#,
    "{}",
    r#"{ x = 1, y = "b" }"#,
    "[1, 2, 3]",
    ": [u8] / [0, 1, 255, 0]",
    ": [u8] / []",
    ": [()] / [(), ()]",
    "#{3, 1, 2}",
    r#"%{ "b" -> 1, "a" -> 2 }"#,
    ": %{u32 -> [string]} / %{}",
    r#"{| x, y; 1, "a"; 2, "b" |}"#,
    ": {| x: int |} / {| x |}",
    "{| |}",
    "[| 1 2 3, 4 5 6 |]",
    ": [|u8, 2|] / [| 0 3 | |]",
    "[| 1 3 | 1.5 2.5 3.5 |]",
    "some 1",
    ": ?u8 / none",
    ": ??u8 / some none",
    "ok 1",
    r#": !u32 / er error "x""#,
    "atom Red",
    "term Foo 42",
    r#": enum { atom A, term B string } / term B "x""#,
    ": enum { atom A, term B string } / atom A",
    "data 1",
    "data [1, 2]",
    "data : u8 / 3",
    "data data 1",
    r#"error "oops""#,
    ": rec T [?T] / [some [], none, some [none]]",
    ": rec T (int, [T]) / (1, [(2, []), (3, [(4, [])])])",
    ": rec T %{string -> T} / %{ \"a\" -> %{}, \"b\" -> %{ \"c\" -> %{} } }",
    ": rec T [T] / [[], [[]]]",
    "data : rec T [?T] / [some []]",
];

#[test]
fn every_kind_of_value_round_trips() {
    let db = Database::default();
    let mut fx = Fixture::new(&db);
    for text in VALUES {
        let v = fx.must_load(text);
        fx.round_trip(text, &v);
        fx.drop_val(v);
    }
}

/// Runs of values of one type, each above the last.
const ASCENDING: &[(&str, &[&str])] = &[
    ("int", &["-123456789012345678901234567890", "-256", "-255", "-1", "0", "1", "255", "256", "123456789012345678901234567890"]),
    ("i32", &["-2147483648", "-1", "0", "1", "2147483647"]),
    ("f64", &["0xFFF8000000000000", "0xFFF0000000000000", "-1.0", "-0.0", "0.0", "1.0e-300", "1.0", "0x7FF0000000000000", "0x7FF8000000000000"]),
    ("f32", &["-1.5", "-0.0", "0.0", "0.25"]),
    ("string", &[r#""""#, r#""\0""#, r#""\0\0""#, r#""\0a""#, r#""a""#, r#""a\0""#, r#""ab""#, r#""b""#, r#""é""#]),
    ("[u8]", &["[]", "[0]", "[0, 0]", "[0, 1]", "[1]", "[255]"]),
    ("[u32]", &["[]", "[0]", "[0, 0]", "[0, 1]", "[1]"]),
    ("[string]", &["[]", r#"[""]"#, r#"["", ""]"#, r#"["a"]"#]),
    ("#{int}", &["#{}", "#{-1}", "#{-1, 0}", "#{0}", "#{0, 5}", "#{1}"]),
    ("%{int -> string}", &["%{}", r#"%{ 0 -> "" }"#, r#"%{ 0 -> "a" }"#, r#"%{ 0 -> "a", 1 -> "" }"#, r#"%{ 1 -> "" }"#]),
    ("(string, u64)", &[r#"("a", 7)"#, r#"("a", 8)"#, r#"("ab", 0)"#, r#"("b", 0)"#]),
    ("?int", &["none", "some -1", "some 0"]),
    ("!int", &["er error 1", r#"er error "a""#, "ok -5", "ok 5"]),
    ("enum { atom A, term B int, atom C }", &["atom A", "term B -1", "term B 1", "atom C"]),
    ("[|u8, 1|]", &["[| |]", "[| 9 |]", "[| 1 2 |]", "[| 1 3 |]"]),
    ("[|u8, 2|]", &["[| 1 2 | 1 2 |]", "[| 2 1 | 1 2 |]", "[| 2 1 | 1 3 |]"]),
    ("{| x: int, y: string |}", &[r#"{| x, y |}"#, r#"{| x, y; 1, "a" |}"#, r#"{| x, y; 1, "a"; 0, "" |}"#, r#"{| x, y; 1, "b" |}"#]),
];

#[test]
fn ascending_values_encode_ascending() {
    let db = Database::default();
    let mut fx = Fixture::new(&db);
    for (ty, values) in ASCENDING {
        let loaded: Vec<(String, Val)> = values.iter()
            .map(|v| {
                let text = fmt!(": {ty} / {v}");
                let val = fx.must_load(&text);
                (text, val)
            })
            .collect();
        let encoded: Vec<Vec<u8>> = loaded.iter().map(|(_, v)| fx.encode(v)).collect();
        for i in 1..loaded.len() {
            let (a, b) = (&loaded[i - 1], &loaded[i]);
            assert_eq!(fx.cmp(&a.1, &b.1), RtOrdering::Less, "the runtime orders `{}` before `{}`", a.0, b.0);
            assert!(encoded[i - 1] < encoded[i], "`{}` encodes before `{}`", a.0, b.0);
        }
        for (_, v) in loaded {
            fx.drop_val(v);
        }
    }
}

/// Data values of different types order by their types' encodings, in the
/// runtime as in bytes.
#[test]
fn data_of_different_types_order_alike() {
    let db = Database::default();
    let mut fx = Fixture::new(&db);
    let texts = [
        "data true", "data : u8 / 1", "data : u64 / 1", "data 1", "data 1.0", "data ()", "data (1, 2)",
        "data { a = 1 }", "data [1]", r#"data "a""#, "data %{ 1 -> 1 }", "data #{1}", "data some 1",
        "data atom A", "data : rec T [?T] / [none]", "data [[1]]", "data [some 1]",
    ];
    let mut vals: Vec<(&str, Val, Vec<u8>)> = texts.iter().map(|t| {
        let v = fx.must_load(t);
        let b = fx.encode(&v);
        (*t, v, b)
    }).collect();
    vals.sort_by(|a, b| a.2.cmp(&b.2));
    for i in 1..vals.len() {
        let (a, b) = (&vals[i - 1], &vals[i]);
        assert_eq!(fx.cmp(&a.1, &b.1), RtOrdering::Less, "`{}` before `{}`", a.0, b.0);
    }
    for (_, v, _) in vals {
        fx.drop_val(v);
    }
}

/// A recursive type and its unfoldings are one type to the runtime, so their
/// encodings are one encoding.
#[test]
fn recursive_types_fold_at_any_depth() {
    let db = Database::default();
    let mut fx = Fixture::new(&db);
    let forms = [
        ": rec T [?T] / []",
        ": [?rec T [?T]] / []",
        ": [?[?rec T [?T]]] / []",
        ": rec T [?[?T]] / []",
    ];
    let vals: Vec<Val> = forms.iter().map(|f| fx.must_load(f)).collect();
    let first = bin::type_bytes(td(vals[0].td));
    for (form, v) in forms.iter().zip(&vals) {
        assert_eq!(bin::type_bytes(td(v.td)), first, "`{form}` encodes as the folded type");
        assert!(rtdt::tyenc::same_type(td(vals[0].td), td(v.td)), "`{form}` is the same type to the runtime");
    }
    // Binder, list, option, reference to the binder.
    assert_eq!(first, [0x90, 0x50, 0x60, 0x91, 0x00]);
    for v in vals {
        fx.drop_val(v);
    }
}

#[test]
fn malformed_bytes_are_refused() {
    let db = Database::default();
    let mut fx = Fixture::new(&db);
    let cases: &[(&str, &[u8], bin::DecodeErrorKind)] = &[
        ("bool", &[2], bin::DecodeErrorKind::BadByte(2)),
        ("u32", &[0, 0, 1], bin::DecodeErrorKind::Truncated),
        ("u8", &[1, 2], bin::DecodeErrorKind::Trailing),
        ("int", &[0x82, 0, 1], bin::DecodeErrorKind::NotMinimal),
        ("string", &[0xFF, 0, 1], bin::DecodeErrorKind::NotUtf8),
        ("string", &[b'a', 0, 2], bin::DecodeErrorKind::BadByte(2)),
        ("#{u8}", &[1, 2, 1, 1, 0], bin::DecodeErrorKind::NotAscending),
        ("#{u8}", &[1, 2, 1, 2, 0], bin::DecodeErrorKind::NotAscending),
        ("enum { atom A, atom B }", &[b'C', 0, 1], bin::DecodeErrorKind::NoSuchVariant),
        ("?u8", &[2], bin::DecodeErrorKind::BadByte(2)),
        // A list of a recursive type cannot hold itself without a collection.
        ("data", &[rtdt::tyenc::tag::REC, rtdt::tyenc::tag::OPTION, rtdt::tyenc::tag::VAR, 0], bin::DecodeErrorKind::Unguarded),
        // A binder nothing refers to is not the canonical form.
        ("data", &[rtdt::tyenc::tag::REC, rtdt::tyenc::tag::BOOL, 1], bin::DecodeErrorKind::NotCanonical),
        // Nor are an enum's variants out of order.
        ("data", &[rtdt::tyenc::tag::ENUM, 2, b'B', 0, 1, 0, b'A', 0, 1, 0, b'A', 0, 1], bin::DecodeErrorKind::NotCanonical),
    ];
    for (ty, bytes, kind) in cases {
        let hint = match *ty {
            "data" => "data 1".S(),
            "enum { atom A, atom B }" => fmt!(": {ty} / atom A"),
            "?u8" => ": ?u8 / none".S(),
            _ => fmt!(": {ty} / {}", match *ty {
                "bool" => "true", "string" => "\"\"", "#{u8}" => "#{}", _ => "0",
            }),
        };
        let v = fx.must_load(&hint);
        match fx.decode(v.td, bytes) {
            Ok(back) => {
                fx.drop_val(back);
                panic!("{ty} {bytes:?} decodes");
            }
            Err(e) => assert_eq!(&e.kind, kind, "{ty} {bytes:?}"),
        }
        fx.drop_val(v);
    }
}

fn gen_config() -> AstGenConfig {
    AstGenConfig {
        type_weights: TypeWeights {
            rec_type: 3,
            named_tuple_type: 0,
            named_struct_type: 0,
            named_enum_type: 0,
            ..TypeWeights::default()
        },
        ..AstGenConfig::default()
    }
}

/// How many generated cases to run, from `DATALIT_BIN_COUNT`.
fn gen_count() -> u64 {
    std::env::var("DATALIT_BIN_COUNT").ok().and_then(|c| c.parse().ok()).unwrap_or(400)
}

/// Pairs of generated values of a generated type: the bytes order as the
/// runtime does, both round trip, and a byte changed anywhere either is
/// refused or reads as a value that encodes to the changed bytes.
#[test]
fn generated_values_order_and_round_trip() {
    let db = Database::default();
    let mut fx = Fixture::new(&db);
    let config = gen_config();
    let mut checked = 0;
    for seed in 0..gen_count() {
        let mut rng = SeededRng::derive(seed, "datalit-bin");
        let ty = ast_gen::gen_type_hint(&db, &mut rng, &config, 0);
        let ty_text = ty.syntax(&db).S();
        let texts: Vec<String> = (0..2).map(|_| {
            let e = ast_gen::gen_expr_matching_type(&db, &mut rng, ty.C(), &config, 0);
            fmt!(": {ty_text} / {}", datalove_datalit::pretty::pretty_print(&db, &ExprFull::new(None, e)))
        }).collect();
        let (Some(a), Some(b)) = (fx.load(&texts[0]), fx.load(&texts[1])) else {
            continue;
        };
        let ea = fx.round_trip(&texts[0], &a);
        let eb = fx.round_trip(&texts[1], &b);
        assert_eq!(
            fx.cmp(&a, &b), rt_ord(ea.cmp(&eb)),
            "seed {seed}: bytes order as the runtime does\n  {}\n  {}", texts[0], texts[1],
        );

        for _ in 0..4 {
            if ea.is_empty() {
                break;
            }
            let mut mutated = ea.C();
            let at = rng.random_range(0..mutated.len());
            mutated[at] = rng.random_range(0..=255u8);
            if let Ok(back) = fx.decode(a.td, &mutated) {
                assert_eq!(fx.encode(&back), mutated, "seed {seed}: what decodes, encodes back alike");
                fx.drop_val(back);
            }
        }
        fx.drop_val(a);
        fx.drop_val(b);
        checked += 1;
    }
    eprintln!("{checked} of {} generated cases loaded", gen_count());
    assert!(checked as u64 > gen_count() / 2, "most generated cases load: {checked}");
}
