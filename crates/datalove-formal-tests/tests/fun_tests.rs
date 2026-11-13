//! Datafun's statement and expression typing held to the typing rules,
//! through the Lean model.
//!
//! The model in `formal/Datalove/Fun.lean` is written from
//! `botdocs/datafun-typing-rules.md` for a slice of datafun: scripts of
//! functions, generic or not, type aliases and statements, without modules
//! or consts. A script outside it is skipped and counted.
//!
//! Scripts come from worldgen, made to stay inside the slice, and are then
//! changed a line at a time at random, so that most are wrong somewhere: a
//! type or a literal changed, an operator swapped, `?`, `!` or `@` added, a
//! name made unbound, a `ret` dropped, a stray `break` or `set` added, a field
//! or index read or written through, borrowed or not, a `match`'s case
//! damaged, a generic's bound changed or its parameters used, a generic
//! called with what is at hand. Before
//! them runs a corpus of cases written by hand. Each script is parsed and
//! typechecked, and the codes of every diagnostic the Rust reports, in order,
//! are compared with the model's.
//!
//! Needs the oracle built, so it runs only when asked, as `oracle_tests` does:
//!
//! ```text
//! FORMAL_ORACLE_TEST=1 cargo test -p datalove-formal-tests --test fun_tests
//! ```
//!
//! Seeds come from `FORMAL_FUN_SEED` and `FORMAL_FUN_COUNT`.

use rmx::prelude::*;
use rmx::serde_json::{Value, json};
use bct::random::SeededRng;
use datalove_formal_tests::{ask_oracle, oracle_path};
use datalove_worldgen::WorldGenConfig;
use std::collections::BTreeMap;

const MAX_MUTATIONS: usize = 3;

/// Scripts written by hand, each a case worth keeping.
const CORPUS: &[&str] = &[
    "let a: i8 = -5\n",
    "let a: u8 = true\nlet b: u8 = none\nlet c: ?u8 = 1\n",
    "let a = not 1\nlet b = true + 1\nlet c = 1 and 2\n",
    "let top = 1 + \"s\"\nfun f(): u8\n  ret true\nend fun\n",
    "fun f(a: u8, b: u8): u8\n  ret a\nend fun\nlet x = f(ref 1, \"s\")\n",
    "fun f(): u8\n  let x = 1\nend fun\n",
    "fun f(a: u8): u8\n  let x: u16 = a\n  let y = x\n  ret y\nend fun\nlet z = 1 + \"s\"\nlet w = z\n",
    "var x: u32 = : u32 / 1\nloop while x .< 3\n  break\nend loop\n",
    "fun f(a: u32, b: u32): !u32\n  ret ok (a +! b)\nend fun\nfun g(a: u32): ?u32\n  let b = a +! a\n  ret some b\nend fun\n",
    "fun f(o: ?u8): u8\n  if o |v|\n    ret v\n  else |e|\n    ret 0\n  end if\nend fun\n",
    "fun f(r: !u8): u8\n  if r |v|\n    ret v\n  end if\n  ret 0\nend fun\n",
    "break\ncontinue\nfun f()\n  ret 1\nend fun\nfun g(): u8\n  ret\nend fun\n",
    "let x = 1\nset x = 2\nset y = 3\nvar z: u8 = 1\nset z += 1\nset z +!= 1\n",
    "fun f(a: u8): u16\n  ret a@\nend fun\nfun g(a: u16): u8\n  ret a@\nend fun\n",
    "fun f(): ()\n  ret ()\nend fun\nfun g(): u8\n  ret 1\nend fun\ncall f()\ncall g()\n",
    "fun f(): u8\n  ret 1\nend fun\nfun f(): u8\n  ret 2\nend fun\n",
    "var v0: u32 = []\nlet a: u8 = [1]\n",
    // A value with a hint at its head that does not check binds its name at
    // the hint's type, as a type on the binding does, so the uses after are
    // not unbound; one without a hint binds nothing.
    "let a = : u32 / \"s\"\nlet b: u32 = a\nlet c = 1 + \"s\"\nlet d = c\n",
    // A call stops at its first argument not of its parameter's type, and
    // fails, so what it was to bind is unbound; a wrong marker is reported
    // and checking goes on.
    "fun f(a: u8, b: u8): u8\n  ret a\nend fun\nlet w = f(true, true)\nlet z = w\nlet y = f(mut 1, true)\nlet x = y\n",
    // A binding `if` that binds nothing leaves its bodies unchecked: a
    // condition with no type, one of neither an option nor a result, and a
    // result with no error bound.
    "fun f(r: !u8, n: u8)\n  if r |v|\n    call nope()\n  end if\n  if n |v|\n    call nope()\n  end if\nend fun\n",
    // An option with an error bound: F094 after the first body, and the
    // error unbound in the second.
    "fun f(o: ?u8)\n  if o |v|\n    call nope()\n  else |e|\n    debuglog e\n  end if\nend fun\n",
    // A `for` over a list, a set and a map, with one name and two, and over
    // what cannot be walked, with something wrong in the body each time.
    "let l: [u8] = [1]\nfor x in ref l\n  call nope()\nend for\nfor i, x in ref l\n  let a: u8 = i\nend for\n",
    "let s: #{u8} = #{1}\nfor i, x in ref s\n  call nope()\nend for\nlet m: %{u8 = bool} = %{1 = true}\nfor k in ref m\n  call nope()\nend for\n",
    "let n: u8 = 1\nfor x in ref n\n  call nope()\nend for\nfor x in ref zz\n  call nope()\nend for\n",
    "let t: [|u8, 2|] = [| 1 2, 3 4 |]\nfor r in ref t\n  let a: u8 = r\nend for\nbreak\n",
    // A negated literal with a hint of its own has the hint's type, and is not
    // range-checked as a bare one is.
    "let a: i8 = -(: string / 8)\nlet b: i8 = -(: i64 / 8)\nlet c: u8 = -(: int / 8)\nlet d: i8 = -128\nlet e: u8 = -1\n",
    // A `set` of an immutable binding still checks its value.
    "let x: u8 = 1\nset x = true\n",
    "if zz |v|\n  call nope()\nelse\n  call nope()\nend if\nif zz\n  call nope()\nelse\n  call nope()\nend if\n",
    // Inference that cannot read a part leaves the part to be synthesized,
    // and a literal it cannot read is still a mismatch.
    "fun f(): u8\n  ret some x\nend fun\nfun g(): u8\n  let a = (x, none)\n  let b = (none, x)\n  let c = [zz, 1]\n  let d = [zz.a]\n  ret some (: Foo / 1)\nend fun\n",
    // Fields: by name and by position, missing, out of range, of what has
    // none, and not copied out of a place but where it is borrowed.
    "fun f(s: {a: u8, b: string}, t: (u8, string)): u8\n  let a: u8 = s.a\n  let b = s.zz\n  let c = t.2\n  let d = t.a\n  let e = s.0\n  let g = s.b\n  let h = t.1@\n  debuglog s.b\n  let i = s.b == \"x\"\n  ret t.0\nend fun\n",
    // Fields of what is not a place.
    "fun mk(): {a: u8, b: string}\n  ret {a = 1, b = \"s\"}\nend fun\nfun f(): u8\n  let a = mk().a\n  let b = mk().b\n  let c = mk().zz\n  let d = (1, 2).1\n  debuglog mk().b\n  ret mk().a\nend fun\n",
    // Aliases: resolved with the signatures, in order, before anything is
    // checked, each seeing only the aliases above it. A name taken is F066
    // and the first stays; a body that does not convert leaves the name free.
    "let a = 1 + \"s\"\ntype E: enum { atom A, term B u8 }\ntype P: (u8, Zz)\nfun f(e: E, q: Qq): u8\n  ret 0\nend fun\ntype E: enum { atom C }\nlet b: E = enum { atom A }\nlet c: Nope = 1\n",
    "fun f(q: Qq): u8\n  ret 0\nend fun\ntype P: (u8, Zz)\nlet b: E = enum { atom A }\ntype E: enum { atom A }\nlet c: E = enum { atom A }\n",
    "fun f(e: E): u8\n  let x: E = 1\n  ret 0\nend fun\ntype E: u8\ntype P: (u8, Zz)\nlet a: P = (1, 2)\nfun g(p: P): u8\n  ret 0\nend fun\n",
    "type E: Zz\ntype E: u8\nlet x: E = 300\ntype E: u8\ntype L: [L]\nlet a: L = []\ntype A: u8\ntype B: (A, A)\nlet y: B = (1, 300)\n",
    // An alias's `rec` is datalit's, a `rec` may not take an alias's name, and
    // a recursive alias is no map's key or set's element.
    "type J: rec J u8\nlet x: J = 1\ntype A: u8\ntype K: rec A [A]\ntype R: rec R [R]\nlet m: %{R = u8} = %{}\nlet s: #{(u8, R)} = #{}\nlet l: [R] = []\ntype S: rec S (R, [S])\n",
    "fun f(): u8\n  ret 0\nend fun\ntype f: u8\nfun f(): u8\n  ret 1\nend fun\nlet x: Zz = 1\ntype E: enum { atom A }\nfun g(e: E, z: Zz): u8\n  let y: Qq = 1\n  ret 0\nend fun\nlet h: E = : E / enum { atom A }\nlet k = : Qq / 1\n",
    // Patterns: a tuple's and a struct's parts, an atom's and a term's; one
    // that does not fit is F071 and binds nothing.
    "let (a, b) = (1, \"s\")\nlet c: string = b\nlet (d, e, f) = (1, 2)\nlet g = d\nlet (h,) = (true,)\nlet () = ()\nlet (i, j): (u8, u8) = (1, 300)\nlet k: u8 = i\n",
    "let {x, y} = {x = 1, y = true}\nlet z: bool = y\nlet {x = p, y = q} = {y = 1, x = 2}\nlet {x} = {x = 1, y = 2}\nlet {x, w} = {x = 1}\nlet r = p\n",
    "let atom A = atom A\nlet atom B = atom A\nlet term T v = term T 5\nlet w: int = v\nlet term U u = term T 5\nlet s = u\n",
    "type E: enum { atom A, term B u8 }\nfun f(e: E, l: [u8], o: ?u8): u8\n  let atom A = e\n  let (a, b) = l\n  let {x} = o\n  let (c, d) = 1\n  ret 0\nend fun\n",
    "type L: rec L (u8, [L])\nfun f(l: L): u8\n  let (n, rest) = l\n  let m: [L] = rest\n  ret n\nend fun\nvar (a, b) = (1, 2)\nset a = 3\nset b += 1\nvar (c, d): (u8, u8) = (1, 2)\nvar e: (u8, u8)\n",
    // `match`: an enum's variants each once, of the right kind, or a
    // default; a term case binds the payload.
    "type E: enum { atom A, term B u8, atom C }\nfun f(e: E): u8\n  match e\n  case atom A\n    ret 1\n  case term B x\n    ret x\n  case atom C\n    ret 3\n  end match\nend fun\nfun g(e: E): u8\n  match e\n  case atom A\n    ret 1\n  case default\n    ret 2\n  end match\nend fun\n",
    "type E: enum { atom A, term B u8, atom C }\nfun f(e: E): u8\n  match e\n  case atom A\n    ret true\n  case atom A\n    ret zz\n  case atom B\n    ret zz\n  case term C y\n    ret y\n  case term D z\n    ret z\n  end match\n  ret 0\nend fun\n",
    "type E: enum { atom A, term B u8 }\nfun f(e: E): u8\n  match e\n  case atom A\n    ret 1\n  end match\nend fun\nfun g(n: u8): u8\n  match n\n  case atom A\n    ret zz\n  end match\n  match zz\n  case atom A\n    ret zz\n  end match\n  ret 0\nend fun\n",
    // A `match` completes when an arm or its default does, and a `break` in
    // an arm is the loop's.
    "type E: enum { atom A, atom B }\nfun f(e: E): u8\n  match e\n  case atom A\n    ret 1\n  case atom B\n    debuglog 0\n  end match\nend fun\nfun g(e: E): u8\n  loop\n    match e\n    case atom A\n      break\n    case default\n      ret 1\n    end match\n  end loop\nend fun\nfun h(e: E): u8\n  match e\n  case atom A\n    ret 1\n  case default\n    ret 2\n  end match\nend fun\n",
    // Elements of what is not a place, refused unless borrowed.
    "fun mk(): [string]\n  ret [\"a\"]\nend fun\nfun f(): ?u8\n  let a = mk()[0]?\n  let b = mk()[0]?@\n  debuglog mk()[0]?\n  let c = mk()[0]? == \"a\"\n  ret none\nend fun\n",
    // Indexes: with `?` and `!` and neither, into what cannot be indexed,
    // with a key of the wrong type, returning early where the function does
    // not, and a non-copy element moved out.
    "fun f(l: [u8], m: %{string = u8}, n: u8, ls: [string]): ?u8\n  let a = l[0]?\n  let b = l[0]!\n  let c = l[0]\n  let d = n[0]?\n  let e = l[true]?\n  let g = m[\"k\"]?\n  let h = m[1]?\n  let i = ls[0]?\n  let j = ls[0]?@\n  debuglog ls[0]?\n  ret l[0]?\nend fun\n",
    // A tensor of rank 2 indexes to a row, a view, which no `mut` argument
    // may be.
    "fun g(mut r: [|u8, 1|])\nend fun\nfun h(ref r: [|u8, 1|])\nend fun\nfun f(mut t: [|u8, 2|]): ?()\n  let r = t[0]?\n  call g(mut t[0]?)\n  call h(ref t[0]?)\n  let x: u8 = t[0]?[1]?\n  ret some ()\nend fun\n",
    // Steps along a place: fields and indexes, each but the last borrowed.
    "fun f(s: {l: [string], t: (u8, [u8])}): ?u8\n  let a = s.l[0]?\n  let b = s.t.1[0]?\n  let c = s.t.1\n  let d = s.l\n  ret s.t.0\nend fun\n",
    // `set` through a place: fields, indexes, a map's insertion, views, and
    // the early returns its indexes make.
    "fun f(mut s: {a: u8, l: [u8]}, mut m: %{string = u8}, mut t: [|u8, 2|], mut n: u8): ?()\n  set s.a = 1\n  set s.zz = 1\n  set s.l[0]? = 2\n  set s.l[0] = 2\n  set m[\"k\"] = 1\n  set m[\"k\"] += 1\n  set m[1] = true\n  set m[\"k\"]? = 2\n  set m[\"k\"]! = 2\n  set t[0]? = t[1]?\n  set t[0]?[1]? = 1\n  set n[0]? = 1\n  set s.l[0][0]? = 1\n  set s.a +!= 1\n  set s.a += 1\n  set s.a += true\n  ret some ()\nend fun\n",
    // A `for` over a field or an element.
    "fun f(s: {l: [u8], ls: [[u8]]}): ?()\n  for x in ref s.l\n    let a: u8 = x\n  end for\n  for x in ref s.ls[0]?\n    let a: string = x\n  end for\n  for x in ref s.zz\n    call nope()\n  end for\n  ret some ()\nend fun\n",
    // Signatures: a type parameter in a set's element or a map's key has to be
    // ordered (F061), at any depth but a map's value.
    "fun a<T>(s: #{T})\nend fun\nfun b<T>(x: T): %{T = u8}\n  ret %{}\nend fun\nfun c<T>(m: %{u8 = #{?T}}) where { T is ord, }\nend fun\nfun d<T>(l: [%{(T,) = u8}])\nend fun\nfun e<T>(m: %{u8 = T})\nend fun\ncall a(#{})\n",
    // Bindings in a body are held to F061 too, where their type is written.
    "fun f<T, U>(x: T, y: U): u8 where { T is ord, }\n  var s: #{T}\n  var t: #{U}\n  let u: %{U = T} = %{}\n  let v: [#{U}] = []\n  let w: #{T} = #{x}\n  let z = #{y}\n  ret 0\nend fun\n",
    // A bounded type parameter has the operators every type of its bound has.
    "fun f<F, I, O, A>(a: F, b: F, i: I, j: I, o: O, p: O, x: A): ?bool where { F is float, I is fixedint, O is ord, }\n  let s = a + b\n  let d = a / b\n  let n = -a\n  let c = a .< b\n  let e = a == b\n  let k = i +? j\n  let m = i + j\n  let q = -?i\n  let r = -i\n  let t = i <= j\n  let u = o == p\n  let v = o .< p\n  let w = x == x\n  let y = (a, a) == (b, b)\n  let z = -!i\n  let g = not a\n  ret some true\nend fun\nfun h<F, I>(mut a: F, mut i: I): ?() where { F is float, I is fixedint, }\n  set a += a\n  set i +?= i\n  set i += i\n  set a +?= a\n  ret some ()\nend fun\n",
    // A type parameter is no literal's type, and is not copied.
    "fun f<T>(x: T, s: {v: T}, l: [T]): ?T\n  let a: T = 1\n  let b = s.v\n  let c = l[0]?\n  debuglog s.v\n  let d = x +? 1\n  let e: ?T = none\n  let g: T = s.v@\n  ret some x\nend fun\n",
    // Calls: type arguments inferred from the arguments, then from what is
    // expected; open (T016), refused by a bound (F092), or undecided (F101).
    "fun id<T>(x: T): T\n  ret x\nend fun\nfun z<T>(): ?T\n  ret none\nend fun\nfun two<T>(a: T, b: T): T\n  ret a\nend fun\nfun fl<T>(a: T): T where { T is float, }\n  ret a\nend fun\nfun fi<T>(a: T): T where { T is fixedint, }\n  ret a\nend fun\nlet a: u32 = id(5)\nlet b = id(5)\nlet c: u8 = b\ndebuglog id([])\ndebuglog z()\nlet d: ?u8 = z()\nlet e = two(1, : u8 / 2)\nlet f = two(1, 2.0)\nlet g = fl(1.5)\nlet h = fl(1)\nlet i: u8 = fi(1)\nlet j = fi(: f32 / 1.0)\nlet k: string = id(5)\nlet l = id(ref 5)\nlet m = id()\nlet n = two(none, some 1)\nlet o = id(zz)\nlet p = [id(1), : u8 / 2]\n",
    // A caller's own type parameter satisfies only the bound it has itself.
    "fun o<T>(x: T): T where { T is ord, }\n  ret x\nend fun\nfun fl<T>(x: T): T where { T is float, }\n  ret o(x)\nend fun\nfun an<T>(x: T): T\n  ret o(x)\nend fun\nfun or2<T>(x: T): T where { T is ord, }\n  ret o(x)\nend fun\nfun fl2<T>(x: T): T where { T is float, }\n  ret fl(x)\nend fun\nfun i2<T>(x: T): T where { T is fixedint, }\n  ret fl(x)\nend fun\n",
    // Type parameters among names: no `rec` around one (T066), none named as
    // one (T063), one shadowing an alias, and none outside its function.
    "type T: u8\nfun f<T>(x: T): rec L [(L, T)]\n  ret x\nend fun\nfun g<T>(x: rec T [T]): u8\n  ret 0\nend fun\nfun h<U>(x: T, y: U): U\n  let a: T = 300\n  let b: rec M [(M, U)] = []\n  ret y\nend fun\nfun k<T>(x: T): u8\n  let a: T = 300\n  ret 0\nend fun\nlet q: U = 1\nlet r: T = 300\n",
    // Generics calling generics, at the caller's type parameters.
    "fun id<T>(x: T): T\n  ret x\nend fun\nfun f<T>(x: T, l: [T]): [T]\n  let a: T = id(x)\n  let b = id(l)\n  let c: [T] = id([])\n  let d = id(x) == id(x)\n  let e: ?T = id(none)\n  ret b\nend fun\n",
    // `-` reads its operand borrowed, synthesized or checked against a float.
    "fun mk(): {r: !f32}\n  ret {r = ok 1.5}\nend fun\nlet a: f32 = -(mk().r!)\nlet b = -(mk().r!)\nfun g<T>(x: T): !T where { T is float, }\n  let y: T = -(id(x))\n  ret ok y\nend fun\nfun id<T>(x: T): T\n  ret x\nend fun\n",
    // A parameter bounded to the floats is checked under `-` as a float is.
    "fun f<T>(x: T, y: u8): !T where { T is float, }\n  let a: T = -x\n  let b: T = -(x * x)\n  let c: T = -(-x)\n  let d: T = -y\n  let e: T = -1.5\n  let g: T = -5\n  ret ok a\nend fun\nfun h<T>(x: T): T where { T is ord, }\n  let a: T = -x\n  ret x\nend fun\n",
    // Consts: an initializer names only consts (F058), a function sees the
    // consts above it and not those after, a const argument names a const
    // (F076), and in a generic neither a const parameter (F077) nor a const
    // (F088) has a type parameter in its type.
    "let a = 1\nconst b = a + 1\nconst c: u8 = 300\nconst d = 10 / 0\nfun f(const n: u8): u8\n  ret n\nend fun\nfun g(): u8\n  ret c\nend fun\nfun h<T>(const x: T): u8\n  const k: T = x\n  ret 0\nend fun\ndebuglog f(5)\ndebuglog f(c)\n",
    "fun two(): u8\n  ret 2\nend fun\nconst a: u8 = two()\nconst b: u8 = 1 +! 1\nconst c: ?u8 = none\nconst d: u8 = c?\nfun f(): u8\n  ret a\nend fun\nfun g(): u8\n  ret late\nend fun\nconst late: u8 = 3\ndebuglog f()\n",
    "fun f(p: u8, const q: u8): u8\n  const x = p\n  let y = 1\n  const z = y\n  const w = q\n  set w = 2\n  ret x\nend fun\nconst k = 1\nlet k = 2\nfun g(): int\n  ret k\nend fun\nlet m = 1\nconst m = zz\nfun h(const v: int): int\n  ret v\nend fun\ndebuglog h(m)\n",
    // Every marker is reported before any argument is checked.
    "fun g(ref a: u8, b: u8)\nend fun\ncall g(1, ref true)\ncall g(ref true, ref 1)\n",
];

fn config() -> WorldGenConfig {
    let mut config = WorldGenConfig::default();
    config.module_count = (1, 1);
    config.functions_per_module = (1, 4);
    config.generics_per_module = (0, 3);
    config.generic_call_probability = 35;
    config.type_aliases_per_module = (0, 0);
    config.type_alias_usage_probability = 0;
    config.enums_per_module = (0, 3);
    config.consts_per_module = (0, 2);
    config.const_probability = 30;
    config.match_probability = 20;
    config.for_probability = 15;
    config.param_mode_probability = 0;
    config.projection_probability = 40;
    config.function_call_probability = 50;
    config.arithmetic_probability = 50;
    config
}

const TYPES: &[&str] = &["u8", "i8", "u32", "i64", "int", "f32", "f64", "bool", "string", "index", "?u8", "!u8", "[u8]", "()"];
const VALUES: &[&str] = &["0", "1", "-1", "300", "true", "\"s\"", "1.5", "none", "some 1", "ok 1", "()", "[]"];
const STEPS: &[&str] = &[".0", ".1", ".a", ".zz", "[0]?", "[0]!", "[0]", "[true]?", ".0[0]?", "[0]?.0", "[0]?[0]?", "[0]"];
const OPS: &[&str] = &["+", "-", "*", "/", "+!", "-!", "*?", "/?", "==", ".<", "and"];

/// One random change to one line of a script.
fn mutate(rng: &mut SeededRng, text: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(|l| l.S()).collect();
    if lines.is_empty() {
        return text.S();
    }
    let i = rng.random_range(0..lines.len());
    let line = lines[i].C();
    let indent: String = line.chars().take_while(|c| *c == ' ').collect();
    let pick = |rng: &mut SeededRng, from: &[&str]| from[rng.random_range(0..from.len())].S();
    let name = |rng: &mut SeededRng| pick(rng, &["p0", "p1", "p2", "v0", "v1", "v2", "1", "0x10", "1.5", "true"]);
    let step = |rng: &mut SeededRng| pick(rng, STEPS);
    let changed = match rng.random_range(0..22) {
        // A type named in the line changed.
        0 => {
            let ty = pick(rng, TYPES);
            match TYPES.iter().find(|t| line.contains(&fmt!(": {t}"))) {
                Some(t) => line.replacen(&fmt!(": {t}"), &fmt!(": {ty}"), 1),
                None => line,
            }
        }
        // What the line binds, sets or returns, replaced by a value.
        1 => match line.find(" = ").or_else(|| line.find("ret ").map(|p| p + 1)) {
            Some(p) => fmt!("{} {}", &line[..p + 2].trim_end(), pick(rng, VALUES)),
            None => line,
        },
        // An operator swapped.
        2 => {
            let to = pick(rng, OPS);
            match [" + ", " - ", " * ", " / ", " +! ", " -! ", " *! ", " /! ", " +? ", " -? ", " *? ", " /? ", " == ", " .< "]
                .iter().find(|op| line.contains(*op)) {
                Some(op) => line.replacen(op, &fmt!(" {to} "), 1),
                None => line,
            }
        }
        // `?`, `!` or `@` added to the end of a value.
        3 => if line.contains(" = ") || line.trim_start().starts_with("ret ") {
            fmt!("{line}{}", pick(rng, &["?", "!", "@"]))
        } else {
            line
        },
        // A name made one nothing binds.
        4 => match line.find(" v").or_else(|| line.find(" p")) {
            Some(p) => fmt!("{} zz{}", &line[..p], &line[p + 1..]),
            None => line,
        },
        // A `ret` dropped.
        5 => if line.trim_start().starts_with("ret") { fmt!("{indent}debuglog 0") } else { line },
        // A stray `break` or `continue`.
        6 => fmt!("{line}\n{indent}{}", pick(rng, &["break", "continue"])),
        // A `set` of something.
        7 => fmt!("{line}\n{indent}set {} = {}", pick(rng, &["v0", "v1", "p0", "x"]), pick(rng, VALUES)),
        // A call statement of something.
        8 => fmt!("{line}\n{indent}call {}()", pick(rng, &["fn0", "fn1", "fn2", "nope"])),
        // A binding computed from what is in scope: an operator over two
        // names, or one adapted, unwrapped or negated, with or without a type.
        9 => {
            let (a, b, op) = (name(rng), name(rng), pick(rng, OPS));
            fmt!("{line}\n{indent}let w = {a} {op} {b}")
        }
        10 => {
            let (a, ty) = (name(rng), pick(rng, TYPES));
            let form = pick(rng, &["@", "?", "!", ""]);
            let neg = if rng.random_bool(0.3) { "-" } else { "" };
            fmt!("{line}\n{indent}let w: {ty} = {neg}{a}{form}")
        }
        // A binding `if` on something in scope, with an `else` that binds or
        // not, or none.
        11 => {
            let a = name(rng);
            let tail = pick(rng, &["", "else\n  debuglog 0\n", "else |e|\n  debuglog e\n"]);
            let tail = tail.replace('\n', &fmt!("\n{indent}")).replace(&fmt!("{indent}  "), &fmt!("{indent}  "));
            fmt!("{line}\n{indent}if {a} |w|\n{indent}  debuglog w\n{indent}{tail}end if")
        }
        // A `ret` of something in scope, or of nothing.
        12 => fmt!("{line}\n{indent}ret {}", pick(rng, &["", "p0", "p1", "v0", "()", "ok ()", "none"])),
        // A call with arguments from scope, some with a mode marker.
        13 => {
            let (a, b) = (name(rng), name(rng));
            let marker = pick(rng, &["", "", "ref ", "mut "]);
            fmt!("{line}\n{indent}let w = {}({marker}{a}, {b})", pick(rng, &["fn0", "fn1", "fn2"]))
        }
        // A `for` over something in scope, with one name or two, and a body
        // that uses them.
        14 => {
            let names = pick(rng, &["w", "i, w", "k, w"]);
            fmt!("{line}\n{indent}for {names} in ref {}\n{indent}  debuglog w\n{indent}  let z: u8 = w\n{indent}end for", name(rng))
        }
        // A step into something in scope, read moved or borrowed.
        15 => {
            let (a, st) = (name(rng), step(rng));
            match rng.random_range(0..4) {
                0 => fmt!("{line}\n{indent}debuglog {a}{st}"),
                1 => fmt!("{line}\n{indent}let w = {a}{st}@"),
                2 => fmt!("{line}\n{indent}let w = {a}{st} == {a}{}", step(rng)),
                _ => fmt!("{line}\n{indent}let w = {a}{st}"),
            }
        }
        // A `set` through a place, plain or compound.
        16 => {
            let (a, st) = (name(rng), step(rng));
            let op = pick(rng, &["", "", "+", "+!", "*?"]);
            fmt!("{line}\n{indent}set {a}{st} {op}= {}", pick(rng, VALUES))
        }
        // A `match`'s case damaged: of the other kind, of a variant the
        // enum has not, a default made a case, or the input not an enum.
        17 => {
            let trimmed = line.trim_start();
            let words: Vec<&str> = trimmed.split_whitespace().collect();
            match words.as_slice() {
                ["case", "atom", name] => match rng.random_range(0..2) {
                    0 => fmt!("{indent}case term {name} w"),
                    _ => fmt!("{indent}case atom Zz"),
                },
                ["case", "term", name, _] => match rng.random_range(0..2) {
                    0 => fmt!("{indent}case atom {name}"),
                    _ => fmt!("{indent}case term Zz w"),
                },
                ["case", "default"] => fmt!("{indent}case atom Zz"),
                ["match", _] => fmt!("{indent}match {}", name(rng)),
                _ => line,
            }
        }
        // A generic's bound changed, dropped or added, or a statement added
        // that uses its type parameters.
        18 => {
            let bound = pick(rng, &["ord", "float", "fixedint"]);
            if let Some(p) = line.find(" where {") {
                match rng.random_range(0..2) {
                    0 => line[..p].S(),
                    _ => ["ord", "float", "fixedint"].iter()
                        .find(|b| line.contains(&fmt!(" is {b},")))
                        .map(|b| line.replacen(&fmt!(" is {b},"), &fmt!(" is {bound},"), 1))
                        .unwrap_or(line),
                }
            } else if line.starts_with("fun gen") {
                fmt!("{line} where {{ {} is {bound}, }}", pick(rng, &["T", "K", "V"]))
            } else {
                let (a, b) = (pick(rng, &["x", "a", "b", "k", "v"]), pick(rng, &["x", "a", "b", "k", "v"]));
                let stmt = match rng.random_range(0..8) {
                    0 => fmt!("let w: #{{T}} = #{{}}"),
                    1 => fmt!("var w: %{{{} = u8}}", pick(rng, &["T", "K", "V", "[T]", "?K"])),
                    2 => fmt!("let w: T = {a}"),
                    3 => fmt!("let w = {a} {} {b}", pick(rng, OPS)),
                    4 => fmt!("let w = {}{a}", pick(rng, &["-", "-?", "-!", "not "])),
                    5 => fmt!("let w = {a} {} {b}", pick(rng, &[".<", "<=", "==", "!="])),
                    6 => fmt!("let w: {} = {a}", pick(rng, &["T", "?T", "[T]", "K", "V"])),
                    _ => fmt!("let w = gen0_{}({a})", rng.random_range(0..3)),
                };
                fmt!("{line}\n{indent}{stmt}")
            }
        }
        // A generic called with what is at hand, its answer bound with a type
        // or without.
        19 => {
            let args: Vec<String> = (0..rng.random_range(0..3))
                .map(|_| if rng.random_bool(0.5) { name(rng) } else { pick(rng, VALUES) })
                .collect();
            let hint = if rng.random_bool(0.5) { fmt!(": {}", pick(rng, TYPES)) } else { String::new() };
            fmt!("{line}\n{indent}let w{hint} = gen0_{}({})", rng.random_range(0..3), args.join(", "))
        }
        // A const bound from what is at hand, or a `let` made one.
        20 => match rng.random_range(0..3) {
            0 => fmt!("{line}\n{indent}const w = {}", name(rng)),
            1 => fmt!("{line}\n{indent}const w: {} = {}", pick(rng, TYPES), pick(rng, VALUES)),
            _ => match line.trim_start().strip_prefix("let ") {
                Some(rest) => fmt!("{indent}const {rest}"),
                None => line,
            },
        },
        // A literal at a range's edge, negated.
        _ => fmt!("{line}\n{indent}let w: {} = -{}", pick(rng, &["i8", "u8", "i16", "f32", "int"]), pick(rng, &["5", "128", "129", "300", "1.5"])),
    };
    lines[i] = changed;
    lines.join("\n") + "\n"
}

/// The codes of a script's type diagnostics, in order, and its serde AST.
fn rust_side(text: &str) -> (Vec<String>, Value) {
    let db = &datalove_datafun_compiler::Database::default();
    let source = bct::input::Source::new(db, text.S());
    let parsed = datalove_datafun_parser::parse_for_diagnostics(db, source);
    let script = datalove_datafun_ast::ast_serde::Script::from_ast(db, parsed.C());
    let (unit, env) = datalove_datafun_tycheck::single_fragment_script(db, source);
    let _ = datalove_datafun_tycheck::type_check_script_units(db, unit, env);
    let diagnostics = datalove_datafun_tycheck::type_check_script_units::accumulated::<datalove_diagnostic::TypeDiagnostic>(db, unit, env);
    let codes = diagnostics.iter()
        .map(|d| d.to_diagnostic(db).code.map(|c| c.as_str(db).S()).unwrap_or_else(|| "none".S()))
        .collect();
    (codes, rmx::serde_json::to_value(&script).X())
}

#[test]
fn statement_typing_agrees() {
    if std::env::var("FORMAL_ORACLE_TEST").is_err() {
        println!("Skipping the statement typing tests (set FORMAL_ORACLE_TEST=1 to run)");
        return;
    }
    let seeds = bct::random::env_seeds("FORMAL_FUN", 0, 300);
    let config = config();
    let mut cases: Vec<(String, String)> = CORPUS.iter().map(|t| ("corpus".S(), t.S())).collect();
    for seed in seeds.iter() {
        let session = datalove_worldgen::gen_session_seeded(seed, config.C());
        let mut text = session.modules.first().map(|m| m.1.C()).unwrap_or_default();
        let mut rng = SeededRng::derive(seed, "mutate");
        for _ in 0..rng.random_range(0..=MAX_MUTATIONS) {
            text = mutate(&mut rng, &text);
        }
        cases.push((fmt!("seed {seed}"), text));
    }

    let mut queries = Vec::new();
    let mut rust = Vec::new();
    for (_, text) in &cases {
        let (codes, script) = rust_side(text);
        queries.push(json!({ "fun": script }));
        rust.push(codes);
    }
    let answers = ask_oracle(&queries);

    let mut skipped = BTreeMap::<String, usize>::new();
    let mut agreed = 0;
    let mut outcomes = BTreeMap::<String, usize>::new();
    let mut mismatches = Vec::new();
    for (((what, text), rust), oracle) in cases.iter().zip(&rust).zip(&answers) {
        if let Some(why) = oracle.get("unsupported") {
            // What the Rust says of a script the model does not take, to see
            // what widening the slice will have to agree with.
            if what == "corpus" && std::env::var("FORMAL_FUN_SHOW_SKIPPED").is_ok() {
                println!("skipped ({why}):\n{text}    rust: {rust:?}");
            }
            *skipped.entry(why.as_str().X().S()).or_default() += 1;
            continue;
        }
        let model: Vec<String> = oracle["codes"].as_array().X().iter().map(|c| c.as_str().X().S()).collect();
        for c in rust {
            *outcomes.entry(c.C()).or_default() += 1;
        }
        if rust.is_empty() {
            *outcomes.entry("clean".S()).or_default() += 1;
        }
        if *rust == model {
            agreed += 1;
        } else {
            mismatches.push(fmt!("{what}:\n{text}    rust:  {rust:?}\n    model: {model:?}"));
        }
    }
    println!(
        "{} scripts over seeds {}..{}: {agreed} agree, {} differ; skipped {skipped:?}; codes {outcomes:?}",
        cases.len(), seeds.start, seeds.start.wrapping_add(seeds.count), mismatches.len(),
    );
    for m in mismatches.iter().take(15) {
        println!("{m}");
    }
    assert!(mismatches.is_empty(), "the Rust and the typing rules disagree on {} scripts", mismatches.len());
}

/// Type arguments for a parameter with no bound or `ord`, none of them
/// recursive: the check-once theorem leaves recursive arguments out.
const ANY_ARGS: &[&str] = &[
    "u8", "i64", "int", "f32", "f64", "string", "bool", "[u8]", "?u8", "!u8", "(u8, string)",
    "{a: u8, b: string}", "%{string = u8}", "#{i32}", "[|f64, 2|]", "enum { atom A, term B u8 }",
];
const FLOAT_ARGS: &[&str] = &["f32", "f64"];
const FIXED_ARGS: &[&str] = &["u8", "i8", "u16", "i16", "u32", "i32", "u64", "i64", "index", "offset"];

/// Each generic function of a script, instantiated: the script with its type
/// parameters declared as aliases of type arguments their bounds admit, and
/// the function copied after it without them, as `formal/Datalove/Proofs/InstanceStmt.lean`
/// states an instance.
fn instances(rng: &mut SeededRng, text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let header = line.strip_prefix("fun ")
            .and_then(|rest| Some((rest, rest.find('<')?, rest.find('>')?)));
        let Some((rest, lt, gt)) = header else {
            i += 1;
            continue;
        };
        let Some(end) = (i + 1..lines.len()).find(|&j| lines[j] == "end fun") else { break };
        let name = &rest[..lt];
        let after = &rest[gt + 1..];
        let (sig, bounds) = match after.find(" where {") {
            Some(w) => (&after[..w], &after[w..]),
            None => (after, ""),
        };
        let mut aliases = String::new();
        for p in rest[lt + 1..gt].split(',').map(str::trim) {
            let pool = if bounds.contains(&fmt!("{p} is float")) {
                FLOAT_ARGS
            } else if bounds.contains(&fmt!("{p} is fixedint")) {
                FIXED_ARGS
            } else {
                ANY_ARGS
            };
            aliases += &fmt!("type {p}: {}\n", pool[rng.random_range(0..pool.len())]);
        }
        let body = lines[i + 1..=end].join("\n");
        let text = text.trim_end();
        out.push(fmt!("{aliases}{text}\n\nfun {name}_inst{sig}\n{body}\n"));
        i = end + 1;
    }
    out
}

/// The check-once theorem held to the Rust: a script the Rust checks without
/// a diagnostic, with each of its generic functions instantiated and appended,
/// still checks without one, and the model agrees. The scripts are worldgen's,
/// and the same changed by the mutations that touch generics, wherever that
/// leaves them clean.
#[test]
fn instances_check() {
    if std::env::var("FORMAL_ORACLE_TEST").is_err() {
        println!("Skipping the instance tests (set FORMAL_ORACLE_TEST=1 to run)");
        return;
    }
    let seeds = bct::random::env_seeds("FORMAL_FUN", 0, 300);
    let config = config();
    let mut cases = Vec::new();
    for seed in seeds.iter() {
        let session = datalove_worldgen::gen_session_seeded(seed, config.C());
        let base = session.modules.first().map(|m| m.1.C()).unwrap_or_default();
        let mut rng = SeededRng::derive(seed, "instances");
        let mut mutated = base.C();
        for _ in 0..rng.random_range(1..=MAX_MUTATIONS) {
            mutated = mutate(&mut rng, &mutated);
        }
        for text in [base, mutated] {
            // Clean: no type diagnostic, and nothing the parser could not read.
            let (codes, script) = rust_side(&text);
            if !codes.is_empty() || script.to_string().contains("ParseError") {
                continue;
            }
            for inst in instances(&mut rng, &text) {
                cases.push((seed, inst));
            }
        }
    }

    let mut queries = Vec::new();
    let mut rust = Vec::new();
    for (_, text) in &cases {
        let (codes, script) = rust_side(text);
        queries.push(json!({ "fun": script }));
        rust.push(codes);
    }
    let answers = ask_oracle(&queries);
    let mut failures = Vec::new();
    for (((seed, text), rust), oracle) in cases.iter().zip(&rust).zip(&answers) {
        // Every instance is inside the slice, as the script it was made from
        // is, so one the model does not take, a parse error among them, is a
        // failure of the test, not something to skip.
        if let Some(why) = oracle.get("unsupported") {
            failures.push(fmt!("seed {seed}: unsupported ({why}):\n{text}"));
            continue;
        }
        let model: Vec<String> = oracle["codes"].as_array().X().iter().map(|c| c.as_str().X().S()).collect();
        if !rust.is_empty() || !model.is_empty() {
            failures.push(fmt!("seed {seed}:\n{text}    rust:  {rust:?}\n    model: {model:?}"));
        }
    }
    println!(
        "{} instances over seeds {}..{}: {} clean, {} not",
        cases.len(), seeds.start, seeds.start.wrapping_add(seeds.count),
        cases.len() - failures.len(), failures.len(),
    );
    for f in failures.iter().take(10) {
        println!("{f}");
    }
    assert!(failures.is_empty(), "{} instances of clean generics are not clean", failures.len());
}

/// Print the serde AST of `FORMAL_FUN_AST_OF`, a file, with the codes the Rust
/// and the model give it.
#[test]
fn ast_of() {
    let Ok(path) = std::env::var("FORMAL_FUN_AST_OF") else { return };
    let (codes, script) = rust_side(&std::fs::read_to_string(path).X());
    println!("{}", rmx::serde_json::to_string_pretty(&script).X());
    println!("rust:  {codes:?}");
    if oracle_path().exists() {
        println!("model: {}", ask_oracle(&[json!({ "fun": script })])[0]);
    }
}

/// Print the script a seed makes, to look at a failure:
/// `FORMAL_FUN_DUMP=<seed> cargo test -p datalove-formal-tests --test fun_tests dump -- --nocapture`.
#[test]
fn dump() {
    let Ok(seed) = std::env::var("FORMAL_FUN_DUMP") else { return };
    let seed: u64 = seed.parse().X();
    let session = datalove_worldgen::gen_session_seeded(seed, config());
    let mut text = session.modules.first().map(|m| m.1.C()).unwrap_or_default();
    let mut rng = SeededRng::derive(seed, "mutate");
    for _ in 0..rng.random_range(0..=MAX_MUTATIONS) {
        text = mutate(&mut rng, &text);
    }
    println!("{text}");
}
