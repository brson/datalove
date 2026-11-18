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
//! are compared with the model's. Each generated script also runs with its
//! bindings' hints dropped, and where both check a script cleanly, the types
//! they give its unhinted bindings are compared as well. `runs_agree` runs
//! scripts that check, with a driver calling their functions, in the model
//! and with the CLI, and compares what they log.
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
use datalove_datafun_ast::ast::{ExprArena, ExprFun, Statement};
use datalove_datafun_tycheck::Type as DfType;

const MAX_MUTATIONS: usize = 3;

/// Scripts written by hand, each a case worth keeping.
const CORPUS: &[&str] = &[
    // What inference decides by what a type parameter stands for is F110: a
    // literal meeting `T` in a collection, beside it as an operand and as a
    // generic call's argument; a part read off a `T`; a value adapted from
    // `T` meeting a literal or a type; `T` meeting a type with parts.
    "fun g<U>(a: U, b: U): U\n  ret a\nend fun\nfun f<T>(x: T): T where { T is fixedint, }\n  let a = [x, 1]\n  let b = x == 1\n  let c = g(x, 1)\n  let d = [x.0]\n  let e = [x[0]?]\n  let h = [x@, 1]\n  let i = [x@, : u8 / 1]\n  let j = [x, [1]]\n  ret x\nend fun\n",
    // A hint with a type parameter inside a recursive type would be no error
    // under an instance: in a list it is F110, where inference reads it, and
    // alone T066, where checking does.
    "fun f<T>(x: T): T\n  let a = [: rec L (T, [L]) / (x, [])]\n  let b = : rec L (T, [L]) / (x, [])\n  ret x\nend fun\n",
    // A type parameter meeting itself decides nothing by what it stands for.
    "fun g<U>(a: U, b: U): U\n  ret a\nend fun\nfun f<T>(x: T, y: [T]): ?[T] where { T is fixedint, }\n  let a: [T] = [x, x]\n  let b = x == x\n  let c = g(x, x)\n  let d = [x@]\n  let e = [y@, [x]]\n  let h = [y[0]?@]\n  ret some a\nend fun\n",
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
    "let s: #{u8} = #{1}\nfor i, x in ref s\n  call nope()\nend for\nlet m: %{u8 -> bool} = %{1 -> true}\nfor k in ref m\n  call nope()\nend for\n",
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
    "let a = 1 + \"s\"\ntype E = enum { atom A, term B u8 }\ntype P = (u8, Zz)\nfun f(e: E, q: Qq): u8\n  ret 0\nend fun\ntype E = enum { atom C }\nlet b: E = enum { atom A }\nlet c: Nope = 1\n",
    "fun f(q: Qq): u8\n  ret 0\nend fun\ntype P = (u8, Zz)\nlet b: E = enum { atom A }\ntype E = enum { atom A }\nlet c: E = enum { atom A }\n",
    "fun f(e: E): u8\n  let x: E = 1\n  ret 0\nend fun\ntype E = u8\ntype P = (u8, Zz)\nlet a: P = (1, 2)\nfun g(p: P): u8\n  ret 0\nend fun\n",
    "type E = Zz\ntype E = u8\nlet x: E = 300\ntype E = u8\ntype L = [L]\nlet a: L = []\ntype A = u8\ntype B = (A, A)\nlet y: B = (1, 300)\n",
    // An alias's `rec` is datalit's, a `rec` may not take an alias's name, and
    // a recursive alias is no map's key or set's element.
    "type J = rec J u8\nlet x: J = 1\ntype A = u8\ntype K = rec A [A]\ntype R = rec R [R]\nlet m: %{R -> u8} = %{}\nlet s: #{(u8, R)} = #{}\nlet l: [R] = []\ntype S = rec S (R, [S])\n",
    "fun f(): u8\n  ret 0\nend fun\ntype f = u8\nfun f(): u8\n  ret 1\nend fun\nlet x: Zz = 1\ntype E = enum { atom A }\nfun g(e: E, z: Zz): u8\n  let y: Qq = 1\n  ret 0\nend fun\nlet h: E = : E / enum { atom A }\nlet k = : Qq / 1\n",
    // Patterns: a tuple's and a struct's parts, an atom's and a term's; one
    // that does not fit is F071 and binds nothing.
    "let (a, b) = (1, \"s\")\nlet c: string = b\nlet (d, e, f) = (1, 2)\nlet g = d\nlet (h,) = (true,)\nlet () = ()\nlet (i, j): (u8, u8) = (1, 300)\nlet k: u8 = i\n",
    "let {x, y} = {x = 1, y = true}\nlet z: bool = y\nlet {x = p, y = q} = {y = 1, x = 2}\nlet {x} = {x = 1, y = 2}\nlet {x, w} = {x = 1}\nlet r = p\n",
    "let atom A = atom A\nlet atom B = atom A\nlet term T v = term T 5\nlet w: int = v\nlet term U u = term T 5\nlet s = u\n",
    "type E = enum { atom A, term B u8 }\nfun f(e: E, l: [u8], o: ?u8): u8\n  let atom A = e\n  let (a, b) = l\n  let {x} = o\n  let (c, d) = 1\n  ret 0\nend fun\n",
    "type L = rec L (u8, [L])\nfun f(l: L): u8\n  let (n, rest) = l\n  let m: [L] = rest\n  ret n\nend fun\nvar (a, b) = (1, 2)\nset a = 3\nset b += 1\nvar (c, d): (u8, u8) = (1, 2)\nvar e: (u8, u8)\n",
    // `match`: an enum's variants each once, of the right kind, or a
    // default; a term case binds the payload.
    "type E = enum { atom A, term B u8, atom C }\nfun f(e: E): u8\n  match e\n  case atom A\n    ret 1\n  case term B x\n    ret x\n  case atom C\n    ret 3\n  end match\nend fun\nfun g(e: E): u8\n  match e\n  case atom A\n    ret 1\n  case default\n    ret 2\n  end match\nend fun\n",
    "type E = enum { atom A, term B u8, atom C }\nfun f(e: E): u8\n  match e\n  case atom A\n    ret true\n  case atom A\n    ret zz\n  case atom B\n    ret zz\n  case term C y\n    ret y\n  case term D z\n    ret z\n  end match\n  ret 0\nend fun\n",
    "type E = enum { atom A, term B u8 }\nfun f(e: E): u8\n  match e\n  case atom A\n    ret 1\n  end match\nend fun\nfun g(n: u8): u8\n  match n\n  case atom A\n    ret zz\n  end match\n  match zz\n  case atom A\n    ret zz\n  end match\n  ret 0\nend fun\n",
    // A `match` completes when an arm or its default does, and a `break` in
    // an arm is the loop's.
    "type E = enum { atom A, atom B }\nfun f(e: E): u8\n  match e\n  case atom A\n    ret 1\n  case atom B\n    debuglog 0\n  end match\nend fun\nfun g(e: E): u8\n  loop\n    match e\n    case atom A\n      break\n    case default\n      ret 1\n    end match\n  end loop\nend fun\nfun h(e: E): u8\n  match e\n  case atom A\n    ret 1\n  case default\n    ret 2\n  end match\nend fun\n",
    // Elements of what is not a place, refused unless borrowed.
    "fun mk(): [string]\n  ret [\"a\"]\nend fun\nfun f(): ?u8\n  let a = mk()[0]?\n  let b = mk()[0]?@\n  debuglog mk()[0]?\n  let c = mk()[0]? == \"a\"\n  ret none\nend fun\n",
    // Indexes: with `?` and `!` and neither, into what cannot be indexed,
    // with a key of the wrong type, returning early where the function does
    // not, and a non-copy element moved out.
    "fun f(l: [u8], m: %{string -> u8}, n: u8, ls: [string]): ?u8\n  let a = l[0]?\n  let b = l[0]!\n  let c = l[0]\n  let d = n[0]?\n  let e = l[true]?\n  let g = m[\"k\"]?\n  let h = m[1]?\n  let i = ls[0]?\n  let j = ls[0]?@\n  debuglog ls[0]?\n  ret l[0]?\nend fun\n",
    // A tensor of rank 2 indexes to a row, a view, which no `mut` argument
    // may be.
    "fun g(mut r: [|u8, 1|])\nend fun\nfun h(ref r: [|u8, 1|])\nend fun\nfun f(mut t: [|u8, 2|]): ?()\n  let r = t[0]?\n  call g(mut t[0]?)\n  call h(ref t[0]?)\n  let x: u8 = t[0]?[1]?\n  ret some ()\nend fun\n",
    // Steps along a place: fields and indexes, each but the last borrowed.
    "fun f(s: {l: [string], t: (u8, [u8])}): ?u8\n  let a = s.l[0]?\n  let b = s.t.1[0]?\n  let c = s.t.1\n  let d = s.l\n  ret s.t.0\nend fun\n",
    // `set` through a place: fields, indexes, a map's insertion, views, and
    // the early returns its indexes make.
    "fun f(mut s: {a: u8, l: [u8]}, mut m: %{string -> u8}, mut t: [|u8, 2|], mut n: u8): ?()\n  set s.a = 1\n  set s.zz = 1\n  set s.l[0]? = 2\n  set s.l[0] = 2\n  set m[\"k\"] = 1\n  set m[\"k\"] += 1\n  set m[1] = true\n  set m[\"k\"]? = 2\n  set m[\"k\"]! = 2\n  set t[0]? = t[1]?\n  set t[0]?[1]? = 1\n  set n[0]? = 1\n  set s.l[0][0]? = 1\n  set s.a +!= 1\n  set s.a += 1\n  set s.a += true\n  ret some ()\nend fun\n",
    // A `for` over a field or an element.
    "fun f(s: {l: [u8], ls: [[u8]]}): ?()\n  for x in ref s.l\n    let a: u8 = x\n  end for\n  for x in ref s.ls[0]?\n    let a: string = x\n  end for\n  for x in ref s.zz\n    call nope()\n  end for\n  ret some ()\nend fun\n",
    // Signatures: a type parameter in a set's element or a map's key has to be
    // ordered (F061), at any depth but a map's value.
    "fun a<T>(s: #{T})\nend fun\nfun b<T>(x: T): %{T -> u8}\n  ret %{}\nend fun\nfun c<T>(m: %{u8 -> #{?T}}) where { T is ord, }\nend fun\nfun d<T>(l: [%{(T,) -> u8}])\nend fun\nfun e<T>(m: %{u8 -> T})\nend fun\ncall a(#{})\n",
    // Bindings in a body are held to F061 too, where their type is written.
    "fun f<T, U>(x: T, y: U): u8 where { T is ord, }\n  var s: #{T}\n  var t: #{U}\n  let u: %{U -> T} = %{}\n  let v: [#{U}] = []\n  let w: #{T} = #{x}\n  let z = #{y}\n  ret 0\nend fun\n",
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
    "type T = u8\nfun f<T>(x: T): rec L [(L, T)]\n  ret x\nend fun\nfun g<T>(x: rec T [T]): u8\n  ret 0\nend fun\nfun h<U>(x: T, y: U): U\n  let a: T = 300\n  let b: rec M [(M, U)] = []\n  ret y\nend fun\nfun k<T>(x: T): u8\n  let a: T = 300\n  ret 0\nend fun\nlet q: U = 1\nlet r: T = 300\n",
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
    // A recursive return type whose head is an option or a result is
    // returned into early as the option or result it unfolds to.
    "type L = rec L ?[L]\nfun f(x: ?u8, l: [u8]): L\n  let y = x?\n  let a = l[0]?\n  let b = a +? a\n  ret some [none]\nend fun\ntype R = rec R ![R]\nfun g(x: !u8, mut m: [u8]): R\n  let y = x!\n  set m[0]! = 1\n  let z = x?\n  ret ok []\nend fun\n",
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

const TYPES: &[&str] = &[
    "u8", "i8", "u32", "i64", "int", "f32", "f64", "bool", "string", "index", "?u8", "!u8", "[u8]", "()",
    "rec L ?[L]", "rec R ![R]",
];
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
    let original = line.C();
    let indent: String = line.chars().take_while(|c| *c == ' ').collect();
    let is_alias = line.trim_start().starts_with("type ");
    let pick = |rng: &mut SeededRng, from: &[&str]| from[rng.random_range(0..from.len())].S();
    let name = |rng: &mut SeededRng| pick(rng, &["p0", "p1", "p2", "v0", "v1", "v2", "1", "0x10", "1.5", "true"]);
    let step = |rng: &mut SeededRng| pick(rng, STEPS);
    // A statement where inference must decide something of a type
    // parameter's value, put at the top of a generic's body: some decisions
    // depend on what the parameter stands for (F110), and some do not.
    if rng.random_range(0..23) == 22 {
        let Some(j) = lines.iter().position(|l| l.starts_with("fun gen")) else { return text.S() };
        let a = pick(rng, &["x", "a", "b", "k", "v"]);
        let lit = pick(rng, &["1", "0x10", "1.5", "true", "\"s\"", "atom A", "term B 1", "none", "[]", "(1,)"]);
        let stmt = match rng.random_range(0..12) {
            0 => fmt!("let w = [{a}, {lit}]"),
            1 => fmt!("let w = ({a}, {lit})"),
            2 => fmt!("let w = {a} {} {lit}", pick(rng, &["==", ".<", "+?", "*"])),
            3 => fmt!("let w = [{a}{}]", pick(rng, &[".0", ".a", "[0]?", "?", "!", "[0]!"])),
            4 => fmt!("let w = [{a}@, {}]", pick(rng, &[lit.as_str(), a.as_str(), ": u8 / 1", ": [T] / []"])),
            5 => fmt!("let w = [{a}@]"),
            6 => fmt!("let w = [{a}, {}]", pick(rng, &[a.as_str(), "[1]", "some 1", "none", "(1, 2)", "%{1 -> 2}"])),
            7 => fmt!("let w = gen0_{}({a}, {lit})", rng.random_range(0..3)),
            8 => fmt!("let w = [: rec L ({}, [L]) / ({a}, [])]", pick(rng, &["T", "u8"])),
            9 => fmt!("let w: {} = {lit}", pick(rng, &["T", "?T", "[T]"])),
            10 => fmt!("let w = {a} == {a}"),
            _ => fmt!("debuglog [{a}, {lit}]"),
        };
        lines.insert(j + 1, fmt!("  {stmt}"));
        return lines.join("\n") + "\n";
    }
    let changed = match rng.random_range(0..22) {
        // A type named in the line changed.
        0 => {
            let ty = pick(rng, TYPES);
            match TYPES.iter().find(|t| line.contains(&fmt!(": {t}"))) {
                Some(t) => line.replacen(&fmt!(": {t}"), &fmt!(": {ty}"), 1),
                None => line,
            }
        }
        // What the line binds, sets or returns, replaced by a value. A type
        // alias's `=` is followed by a type, not a value.
        1 => match line.find(" = ").filter(|_| !is_alias).or_else(|| line.find("ret ").map(|p| p + 1)) {
            Some(p) => fmt!("{} {}", &line[..p + 2].trim_end(), pick(rng, VALUES)),
            None => line,
        },
        // An operator swapped, not a hint's `/`.
        2 => {
            let to = pick(rng, OPS);
            let found = [" + ", " - ", " * ", " / ", " +! ", " -! ", " *! ", " /! ", " +? ", " -? ", " *? ", " /? ", " == ", " .< "]
                .iter()
                .flat_map(|op| line.match_indices(*op))
                .filter(|(p, op)| *op != " / " || !ends_in_hint(&line[..*p]))
                .min_by_key(|(p, _)| *p);
            match found {
                Some((p, op)) => fmt!("{} {to} {}", &line[..p], &line[p + op.len()..]),
                None => line,
            }
        }
        // `?`, `!` or `@` added to the end of a value.
        3 => if (line.contains(" = ") && !is_alias) || line.trim_start().starts_with("ret ") {
            fmt!("{line}{}", pick(rng, &["?", "!", "@"]))
        } else {
            line
        },
        // A name made one nothing binds.
        4 => match rmx::regex::Regex::new(r"\b[vp][0-9]").X().find(&line) {
            Some(m) => fmt!("{}zz{}", &line[..m.start()], &line[m.start()..]),
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
        // A step into something in scope, read moved or borrowed. A literal
        // is put in parentheses, since `1.a` lexes as a float.
        15 => {
            let (a, st) = (fmt!("({})", name(rng)), step(rng));
            match rng.random_range(0..4) {
                0 => fmt!("{line}\n{indent}debuglog {a}{st}"),
                1 => fmt!("{line}\n{indent}let w = {a}{st}@"),
                2 => fmt!("{line}\n{indent}let w = {a}{st} == {a}{}", step(rng)),
                _ => fmt!("{line}\n{indent}let w = {a}{st}"),
            }
        }
        // A `set` through a place, plain or compound. A place's root is a
        // variable, so no literal is set.
        16 => {
            let (a, st) = (pick(rng, &["p0", "p1", "p2", "v0", "v1", "v2"]), step(rng));
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
            } else if let Some(tps) = line.strip_prefix("fun gen").and_then(|l| Some(&l[l.find('<')? + 1..l.find('>')?])) {
                // A bound on one of the generic's own type parameters: one on
                // another is a parse error.
                let tps: Vec<&str> = tps.split(", ").collect();
                fmt!("{line} where {{ {} is {bound}, }}", pick(rng, &tps))
            } else {
                let (a, b) = (pick(rng, &["x", "a", "b", "k", "v"]), pick(rng, &["x", "a", "b", "k", "v"]));
                let stmt = match rng.random_range(0..8) {
                    0 => fmt!("let w: #{{T}} = #{{}}"),
                    1 => fmt!("var w: %{{{} -> u8}}", pick(rng, &["T", "K", "V", "[T]", "?K"])),
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
    // What is added after a `match` would come between it and its first
    // case, which does not parse: it goes before the `match` instead.
    let changed = match changed.strip_prefix(&fmt!("{original}\n")) {
        Some(added) if original.trim_start().starts_with("match ") => fmt!("{added}\n{original}"),
        _ => changed,
    };
    lines[i] = changed;
    lines.join("\n") + "\n"
}

/// Whether `before` ends in a hint's type, so that a ` / ` after it is the
/// hint's: going back over a type's brackets and words, a `:` that does not
/// follow a name, as a binding's or a field's does.
fn ends_in_hint(before: &str) -> bool {
    let bytes = before.as_bytes();
    let mut depth = 0;
    for i in (0..bytes.len()).rev() {
        match bytes[i] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' if depth == 0 => return false,
            b'(' | b'[' | b'{' => depth -= 1,
            b':' if depth == 0 => {
                return i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
            }
            _ if depth > 0 => {}
            b' ' | b'?' | b'!' | b'#' | b'%' | b'_' => {}
            c if c.is_ascii_alphanumeric() => {}
            _ => return false,
        }
    }
    false
}

/// A script with the hint dropped from each `let` and `var` that has a value:
/// `let x: T = e` written `let x = e`. A hint ends at the first ` = ` outside
/// brackets, since a map type has one inside.
fn drop_binding_hints(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        let rest = trimmed.strip_prefix("let ").or_else(|| trimmed.strip_prefix("var "));
        let dropped = rest.and_then(|rest| {
            let colon = rest.find(':')?;
            let name = &rest[..colon];
            if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return None;
            }
            let hint = &rest[colon..];
            let mut depth = 0i32;
            let bytes = hint.as_bytes();
            for i in 0..bytes.len() {
                match bytes[i] {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => depth -= 1,
                    b'=' if depth == 0 && i > 0 && bytes[i - 1] == b' ' && bytes.get(i + 1) == Some(&b' ') => {
                        return Some(fmt!("{indent}{} {name} ={}", &trimmed[..3], &hint[i + 1..]));
                    }
                    _ => {}
                }
            }
            None
        });
        out += &dropped.unwrap_or_else(|| line.S());
        out += "\n";
    }
    out
}

/// The codes of a script's type diagnostics, in order, its serde AST, and the
/// types it gives its bindings, as `init_types` lists them.
///
/// Every script the tests make parses, the mutations included: one that does
/// not is a failure, of the test, rather than something to type check, as the
/// CLI does not, or to skip, which would hide a change of syntax the
/// generators have not followed.
fn rust_side(text: &str) -> (Vec<String>, Value, Vec<Value>) {
    let db = &datalove_datafun_compiler::Database::default();
    let source = bct::input::Source::new(db, text.S());
    let errors = datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(db, source);
    if let Some(e) = errors.first() {
        panic!("a script the tests made does not parse: {}\n{text}", e.to_diagnostic(db).message.as_str(db));
    }
    let parsed = datalove_datafun_parser::parse_for_diagnostics(db, source);
    let script = datalove_datafun_ast::ast_serde::Script::from_ast(db, parsed.C());
    let (unit, env) = datalove_datafun_tycheck::single_fragment_script(db, source);
    let _ = datalove_datafun_tycheck::type_check_script_units(db, unit, env);
    let diagnostics = datalove_datafun_tycheck::type_check_script_units::accumulated::<datalove_diagnostic::TypeDiagnostic>(db, unit, env);
    let codes = diagnostics.iter()
        .map(|d| d.to_diagnostic(db).code.map(|c| c.as_str(db).S()).unwrap_or_else(|| "none".S()))
        .collect();
    let types = datalove_datafun_tycheck::typecheck_script_unit(db, unit, env).result(db).expr_types(db);
    let mut inits = Vec::new();
    if let datalove_datafun_tycheck::ScriptUnitKind::Fragment(parsed) = &datalove_datafun_tycheck::unit_ast(db, unit).kind {
        let body = parsed.top.body(db);
        init_types(db, &|e| types.get(e).cloned(), &body.arena, &body.statements, &mut inits);
    }
    (codes, rmx::serde_json::to_value(&script).X(), inits)
}

/// The types the Rust gives each `let`, `var` and `const` with a value and no
/// hint, in source order, a function's body where the function is declared,
/// as the oracle's `inits` lists the model's (`Fun.traceScript`). Null where
/// it recorded none.
fn init_types<'db>(
    db: &'db datalove_datafun_compiler::Database,
    type_of: &dyn Fn(ExprFun<'db>) -> Option<DfType<'db>>,
    arena: &'db ExprArena<'db>,
    statements: &'db [Statement<'db>],
    out: &mut Vec<Value>,
) {
    let ty = |id| match type_of(arena.get(id)) {
        Some(DfType::Datalit(t)) => datalove_formal_tests::type_to_json(db, &t),
        Some(DfType::Function(_)) => json!("function"),
        None => Value::Null,
    };
    for s in statements {
        match s {
            Statement::Let(l) if l.type_hint.is_none() => out.push(ty(l.value)),
            Statement::Var(v) if v.type_hint.is_none() => {
                if let Some(value) = v.value {
                    out.push(ty(value));
                }
            }
            Statement::Const(c) if c.type_hint.is_none() => out.push(ty(c.value)),
            Statement::If(i) => {
                init_types(db, type_of, arena, &i.then_body, out);
                if let Some(e) = &i.else_body {
                    init_types(db, type_of, arena, e, out);
                }
            }
            Statement::Loop(l) => init_types(db, type_of, arena, &l.body, out),
            Statement::For(f) => init_types(db, type_of, arena, &f.body, out),
            Statement::Match(m) => {
                for c in &m.cases {
                    init_types(db, type_of, arena, &c.body, out);
                }
                if let Some(d) = &m.default_body {
                    init_types(db, type_of, arena, d, out);
                }
            }
            Statement::Fun(f) => init_types(db, type_of, &f.body.arena, &f.body.statements, out),
            _ => {}
        }
    }
}

/// The codes of each module's type diagnostics, in order, checked as the
/// modules of one world, as a library's are. A diagnostic belongs to the module
/// whose source its first label points into.
fn rust_world_side(modules: &[(String, String)]) -> Vec<Vec<String>> {
    let db = datalove_datafun::Database::default();
    let world: Vec<String> = modules.iter()
        .map(|(path, text)| fmt!("----------\nmodule {path}\n----------\n\n{text}"))
        .collect();
    let world = world.join("\n");
    let raw = datalove_datafun::package_load_worldfile::load_world_from_worldfile(world.as_bytes()).X();
    let package_world = datalove_datafun::package::import_from_loader(&db, raw);
    let resolution = datalove_datafun::package_resolve::resolve_package_world_with_imports(&db, package_world);
    let pkg_graph = resolution.result(&db).C().ok().X();
    let graph = datalove_datafun::to_module_graph(&db, package_world, pkg_graph);
    let parsed = datalove_datafun::module_graph::parse_module_graph(&db, graph.graph, graph.resolved_requires, Vec::new());
    let names = datalove_datafun_resolve::resolve_all_names(&db, parsed);
    let _ = datalove_datafun_tycheck::typecheck_module_graph(&db, parsed, names);
    let mut codes = vec![Vec::new(); modules.len()];
    for d in datalove_datafun_tycheck::typecheck_module_graph::accumulated::<datalove_diagnostic::TypeDiagnostic>(&db, parsed, names) {
        let d = d.to_diagnostic(&db);
        let source = d.labels.first().expect("a type diagnostic points somewhere").source.as_str(&db);
        let module = modules.iter().position(|(_, text)| text.trim() == source.trim())
            .expect("a diagnostic points into one of the world's modules");
        codes[module].push(d.code.map(|c| c.as_str(&db).S()).unwrap_or_else(|| "none".S()));
    }
    codes
}

/// Datafun's module typing held to the model: worldgen's worlds of up to three
/// modules, which require the ones before them, import their functions and
/// call them qualified, changed as the scripts are, one module at a time, and
/// checked as a library's modules are. A module's top level has no function
/// to return early from (F105). A world the Rust reports F089 in, a const that
/// depends on itself through a function, is skipped: the Rust finds those by
/// what its checker looked up, which the model does not follow.
#[test]
fn world_typing_agrees() {
    if std::env::var("FORMAL_ORACLE_TEST").is_err() {
        println!("Skipping the world typing tests (set FORMAL_ORACLE_TEST=1 to run)");
        return;
    }
    let seeds = bct::random::env_seeds("FORMAL_FUN", 0, 300);
    let mut config = config();
    config.module_count = (1, 3);
    config.qualified_calls = true;
    let mut cases: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for world in WORLD_CORPUS {
        cases.push(("corpus".S(), world.iter().map(|(p, t)| (p.S(), t.S())).collect()));
    }
    for seed in seeds.iter() {
        let session = datalove_worldgen::gen_session_seeded(seed, config.C());
        let mut modules = session.modules.C();
        if modules.is_empty() {
            continue;
        }
        let mut rng = SeededRng::derive(seed, "mutate-world");
        for _ in 0..rng.random_range(0..=MAX_MUTATIONS) {
            let m = rng.random_range(0..modules.len());
            modules[m].1 = mutate(&mut rng, &modules[m].1);
        }
        cases.push((fmt!("seed {seed}"), modules));
    }
    let mut queries = Vec::new();
    let mut rust = Vec::new();
    for (_, modules) in &cases {
        let world: Vec<_> = modules.iter()
            .map(|(path, text)| json!({ "path": path, "script": rust_side(text).1 }))
            .collect();
        queries.push(json!({ "world": world }));
        rust.push(rust_world_side(modules));
    }
    let answers = ask_oracle(&queries);
    let mut agreed = 0;
    let mut skipped = BTreeMap::<String, usize>::new();
    let mut mismatches = Vec::new();
    for (((what, modules), rust), oracle) in cases.iter().zip(&rust).zip(&answers) {
        if let Some(why) = oracle.get("unsupported") {
            *skipped.entry(why.as_str().X().S()).or_default() += 1;
            continue;
        }
        if rust.iter().flatten().any(|c| c == "F089") {
            *skipped.entry("F089".S()).or_default() += 1;
            continue;
        }
        let model: Vec<Vec<String>> = oracle["modules"].as_array().X().iter()
            .map(|m| m.as_array().X().iter().map(|c| c.as_str().X().S()).collect())
            .collect();
        if *rust == model {
            agreed += 1;
        } else {
            let text: String = modules.iter().map(|(p, t)| fmt!("-- {p}\n{t}\n")).collect();
            mismatches.push(fmt!("{what}:\n{text}    rust:  {rust:?}\n    model: {model:?}"));
        }
    }
    println!(
        "{} worlds over seeds {}..{}: {agreed} agree, {} differ; skipped {skipped:?}",
        cases.len(), seeds.start, seeds.start.wrapping_add(seeds.count), mismatches.len(),
    );
    for m in mismatches.iter().take(10) {
        println!("{m}");
    }
    assert!(mismatches.is_empty(), "the Rust and the typing rules disagree on {} worlds", mismatches.len());
}

/// Worlds written by hand, each module a path and its text.
const WORLD_CORPUS: &[&[(&str, &str)]] = &[
    // A module const has no function to return early from (F105).
    &[("local/pkg/m", "const a: ?u8 = none\nconst b: u8 = a?\nconst c: !u8 = ok 1\nconst d: u8 = c!\nconst e: u8 = 1 +! 2\nconst f: u8 = 1 +? 2\nconst g: i8 = -!(: i8 / 1)\nconst h: [u8] = [1]\nconst i: u8 = h[0]?\nfun k(): u8\n  ret d\nend fun\n")],
    // A function sees the consts above it, not below (F001), and a const sees
    // every function.
    &[("local/pkg/m", "const a: u8 = 1\nfun f(): u8\n  ret a + b\nend fun\nconst b: u8 = 2\nconst c: u8 = g()\nfun g(): u8\n  ret a\nend fun\n")],
    // Imports: through an alias nothing is required under (F078), of a
    // function the module lacks (F002), of a name defined here (F106), the
    // same name from another module (F059) and from the same one (nothing).
    &[
        ("local/pkg/a", "fun f(x: u8): u8\n  ret x\nend fun\nfun g(): bool\n  ret true\nend fun\nfun h<T>(x: T): [T]\n  ret [x]\nend fun\nconst c: u8 = 1\n"),
        ("local/pkg/b", "fun f(x: bool): bool\n  ret x\nend fun\n"),
        ("local/pkg/c", "require module local/pkg/a\nrequire module local/pkg/b\nimport z.f\nimport a.nope\nimport a.c\nimport a.g\nimport a.f\nimport b.f\nimport a.f\nimport a.h\nfun g(): u8\n  ret f(1)\nend fun\nfun k(): bool\n  let x: [u8] = h(1)\n  ret f(true)\nend fun\n"),
    ],
    // Qualified calls: of a function the module has, of one it lacks (F002),
    // through an alias nothing is required under (F078), of a generic, with
    // the wrong arguments, and of a function nothing imported.
    &[
        ("local/pkg/a", "fun f(x: u8): u8\n  ret x\nend fun\nfun h<T>(x: T): [T]\n  ret [x]\nend fun\n"),
        ("local/pkg/b", "require module local/pkg/a\nimport a.h\nconst k: u8 = a.f(2)\nfun g(): u8\n  let y: [bool] = a.h(true)\n  let z: [u8] = h(1)\n  let w = a.nope()\n  let v = q.f(1)\n  let u = a.f(true)\n  let t = f(1)\n  ret a.f(1)\nend fun\n"),
    ],
];

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
        // The script as generated with its bindings' hints dropped, so that
        // inference decides their types, which are compared where it checks.
        let generated = session.modules.first().map(|m| m.1.C()).unwrap_or_default();
        cases.push((fmt!("seed {seed}, unhinted"), drop_binding_hints(&generated)));
    }

    let mut queries = Vec::new();
    let mut rust = Vec::new();
    for (_, text) in &cases {
        let (codes, script, inits) = rust_side(text);
        queries.push(json!({ "fun": script }));
        rust.push((codes, inits));
    }
    let answers = ask_oracle(&queries);

    let mut skipped = BTreeMap::<String, usize>::new();
    let mut agreed = 0;
    let mut typed = 0;
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
        let (rust, rust_inits) = rust;
        let model: Vec<String> = oracle["codes"].as_array().X().iter().map(|c| c.as_str().X().S()).collect();
        // Where both check the script cleanly, the types they give its
        // bindings, which no diagnostic shows.
        if rust.is_empty() && model.is_empty() {
            let model_inits = oracle["inits"].as_array().X();
            if rust_inits != model_inits {
                mismatches.push(fmt!("{what}:\n{text}    rust types:  {}\n    model types: {}",
                    Value::from(rust_inits.C()), Value::from(model_inits.C())));
                continue;
            }
            typed += rust_inits.len();
        }
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
        "{} scripts over seeds {}..{}: {agreed} agree, {} differ, {typed} binding types compared; skipped {skipped:?}; codes {outcomes:?}",
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
    "{a: u8, b: string}", "%{string -> u8}", "#{i32}", "[|f64, 2|]", "enum { atom A, term B u8 }",
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
            aliases += &fmt!("type {p} = {}\n", pool[rng.random_range(0..pool.len())]);
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
            // Clean: no type diagnostic.
            let (codes, _, _) = rust_side(&text);
            if !codes.is_empty() {
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
        let (codes, script, _) = rust_side(text);
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
    let (codes, script, _) = rust_side(&std::fs::read_to_string(path).X());
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

/// Scripts written by hand for `runs_agree`, each run as it is: checked and
/// optional arithmetic at its bounds, division, indexes in and out of range,
/// `for`, `loop` with `break`, recursion, `match`, a binding `if`, `!` in a
/// function and at the top level, `set` through an index, a tensor's slice
/// among them, and `mut` and `out` arguments written back, an early
/// return's included, and calls
/// that write back inside an expression, read after in it, or returned
/// early from after; a `set` whose value or key changes its target, which it
/// reaches only after them; and copy arguments and operands read where
/// written.
const RUN_CORPUS: &[&str] = &[
    // Elements of a tensor of rank above one written through its slices, by
    // `set`, a compound `set` and a `mut` argument, and an index out of range.
    "fun set9(mut x: u8): u8\n  set x = : u8 / 9\n  ret : u8 / 0\nend fun\nfun row(ref t: [|u8, 2|], i: index): !(u8, u8, u8)\n  ret ok (t[i]![(: index / 0)]!, t[i]![(: index / 1)]!, t[i]![(: index / 2)]!)\nend fun\nvar t: [|u8, 2|] = [|1 2 3, 4 5 6|]\nset t[(: index / 1)]![(: index / 2)]! = : u8 / 7\nset t[(: index / 0)]![(: index / 1)]! +!= : u8 / 250\ndebuglog set9(mut t[(: index / 1)]![(: index / 0)]!)\ndebuglog row(ref t, : index / 0)\ndebuglog row(ref t, : index / 1)\nvar u: [|u8, 3|] = [|2 2 2 | 1 2 3 4 5 6 7 8|]\nset u[(: index / 1)]![(: index / 0)]![(: index / 1)]! = : u8 / 0\ndebuglog set9(mut u[(: index / 0)]![(: index / 1)]![(: index / 0)]!)\ndebuglog (u[(: index / 0)]![(: index / 1)]![(: index / 0)]!, u[(: index / 1)]![(: index / 0)]![(: index / 1)]!, u[(: index / 1)]![(: index / 1)]![(: index / 1)]!)\nset t[(: index / 2)]![(: index / 0)]! = : u8 / 1\n",
    // Calls of functions declared below: from the top level, from a body,
    // and from a const, which reads only consts above it; a function reads
    // the consts above it, evaluated before anything runs.
    "debuglog twice(3)\nconst K: int = 5\nconst L: int = sq(K@)\nfun twice(x: int): int\n  ret add(x@, x) + K@\nend fun\nfun add(a: int, b: int): int\n  ret a + b\nend fun\nfun sq(x: int): int\n  ret x * x\nend fun\ndebuglog L\nfun late(): int\n  ret L@ + twice(1)\nend fun\ndebuglog late()\n",
    // Strings by their text: escapes read, printed escaped again, compared,
    // and looked up as a map's keys.
    "fun same(a: string, b: string): bool\n  ret a == b\nend fun\nfun look(ref m: %{string -> u8}, k: string): ?u8\n  ret some m[k]?\nend fun\nlet s = \"q\\\"b\\\\s\\nn\\rr\\tt\\0z\\u{e9}\\u{1F600}\"\ndebuglog s\ndebuglog s == \"q\\\"b\\\\s\\nn\\rr\\tt\\0z\\u{E9}\\u{1f600}\"\ndebuglog same(s, \"q\")\ndebuglog (\"\", \"\\u{41}\" == \"A\", \"a\" != \"a\")\nlet m: %{string -> u8} = %{\"a\" -> 1, \"b\\n\" -> 2}\ndebuglog look(ref m, \"b\\n\")\ndebuglog look(ref m, \"c\")\nvar t: {s: string, o: ?string} = {s = \"x\", o = some \"y\"}\nset t.s = \"z\"\ndebuglog t\ndebuglog t.o == some \"y\"\n",
    // A hint on a `mut` or `out` argument says only its type: what the
    // parameter came to is written back to the place under it.
    // A field or element of a hinted place is the place's: written back
    // there, its keys evaluated with the arguments, a missing one returning
    // early.
    "fun set9(mut x: u8): u8\n  set x = : u8 / 9\n  ret : u8 / 0\nend fun\nfun put(out x: u8)\n  set x = : u8 / 7\nend fun\nfun bump(mut i: index): index\n  set i = : index / 1\n  ret : index / 0\nend fun\ntype R = {a: u8, b: (u8, u8), l: [u8]}\nvar r: R = {a = : u8 / 1, b = (: u8 / 2, : u8 / 3), l = [: u8 / 4, : u8 / 5]}\ndebuglog set9(mut (: R / r).a)\ndebuglog r\ncall put(out (: (u8, u8) / ((: R / r).b)).1)\ndebuglog r\nvar i: index = : index / 0\ndebuglog set9(mut (: [u8] / ((: R / r).l))[bump(mut i)]!)\ndebuglog r\ndebuglog i\ndebuglog set9(mut (: [u8] / ((: R / r).l))[(: index / 5)]!)\n",
    "fun nine(mut a: u8): u8\n  set a = : u8 / 9\n  ret a@\nend fun\nfun put(out a: u8)\n  set a = : u8 / 7\nend fun\nvar x: u8 = : u8 / 1\ndebuglog nine(mut : u8 / x)\ndebuglog x\nvar r: {f: [u8], g: (u8, u8)} = {f = [: u8 / 1, : u8 / 2], g = (: u8 / 3, : u8 / 4)}\ndebuglog nine(mut : u8 / r.f[(: index / 1)]!)\ndebuglog nine(mut : u8 / r.g.0)\ndebuglog r\nvar y: u8\ncall put(out : u8 / y)\ndebuglog y\ncall put(out : u8 / r.g.1)\ndebuglog r\n",
    "fun grow(mut xs: [int]): int\n  set xs = [7, 8, 9]\n  ret 10\nend fun\nfun key(mut xs: [int]): index\n  set xs = [1, 2, 3]\n  ret : index / 2\nend fun\nfun set9(mut x: u8): u8\n  set x = : u8 / 9\n  ret : u8 / 0\nend fun\nfun first(x: u8, y: u8): u8\n  ret x\nend fun\nvar xs: [int] = [1, 2]\nset xs[(: index / 0)]! = grow(mut xs)\ndebuglog xs\nvar ys: [int] = [1, 2]\nset ys[key(mut ys)]! = 5\ndebuglog ys\nvar zs: [int] = [1, 2]\nset zs[(: index / 1)]! += grow(mut zs)\ndebuglog zs\nvar a: u8 = : u8 / 1\ndebuglog first(a, set9(mut a))\ndebuglog a +! set9(mut a)\ndebuglog a\nret ok ()\n",
    "fun set7(mut x: u8): u8\n  set x = : u8 / 7\n  ret x@\nend fun\nfun g(mut x: u8): !u8\n  let y = set7(mut x) +! (: u8 / 250)\n  ret ok y\nend fun\nvar a: u8 = : u8 / 1\nlet r = g(mut a)\ndebuglog r\ndebuglog a\n",
    "fun bump(mut x: int): int\n  set x = x + 1\n  ret x@\nend fun\nvar a: int = 1\nlet y = bump(mut a) + bump(mut a)\ndebuglog y\ndebuglog a\nlet z: int = bump(mut a)\ndebuglog z\nvar hit: int = 0\nif bump(mut a) == 5\n  set hit = 1\nend if\ndebuglog hit\ndebuglog [bump(mut a), a@]\nlet w = bump(mut a) + a\ndebuglog w\n",
    "fun bump(mut x: u8, by: u8): !u8\n  set x = x +! by\n  ret ok x\nend fun\nfun split(n: int, out q: int, out r: int)\n  set q = n - 1\n  set r = n * 2\nend fun\nfun swap(mut a: i32, mut b: i32)\n  let t = a\n  set a = b\n  set b = t\nend fun\nfun early(mut x: u8): !()\n  set x = : u8 / 7\n  let y: u8 = (: u8 / 250) +! x\n  set x = : u8 / 9\n  ret ok ()\nend fun\nvar a: u8 = : u8 / 10\nlet r1 = bump(mut a, : u8 / 5)\ndebuglog r1\ndebuglog a\nlet r2 = bump(mut a, : u8 / 250)\ndebuglog r2\ndebuglog a\nvar q: int\nvar r: int\ncall split(1234, out q, out r)\ndebuglog q\ndebuglog r\nvar x: i32 = : i32 / 1\nvar y: i32 = : i32 / 2\ncall swap(mut x, mut y)\ndebuglog (x, y)\nvar e: u8 = : u8 / 0\nlet r3 = early(mut e)\ndebuglog r3\ndebuglog e\n",
    "fun add(a: u8, b: u8): !u8\n  ret ok (a +! b)\nend fun\nfun addq(a: i8, b: i8): ?i8\n  ret some (a +? b)\nend fun\nfun div(a: i32, b: i32): !i32\n  ret ok (a /! b)\nend fun\ndebuglog add(: u8 / 100, : u8 / 155)\ndebuglog add(: u8 / 100, : u8 / 156)\ndebuglog addq(: i8 / -100, : i8 / -28)\ndebuglog addq(: i8 / -100, : i8 / -29)\ndebuglog div(: i32 / -7, : i32 / 2)\ndebuglog div(: i32 / 7, : i32 / -2)\ndebuglog div(: i32 / 7, : i32 / 0)\ndebuglog div(: i32 / -2147483648, : i32 / -1)\n",
    "fun at(l: [u8], i: index): ?u8\n  ret some l[i]?\nend fun\nfun sum(l: [u32]): int\n  var s: int = 0\n  for x in ref l\n    set s += x@\n  end for\n  ret s\nend fun\nfun fact(n: int): int\n  if n .< 2\n    ret 1\n  end if\n  ret n * fact(n - 1)\nend fun\nfun count(n: int): int\n  var i: int = 0\n  loop while i .< n\n    set i += 1\n    if i == 7\n      break\n    end if\n  end loop\n  ret i\nend fun\ndebuglog at([1, 2, 3], : index / 1)\ndebuglog at([1, 2, 3], : index / 3)\ndebuglog sum([1, 2, 4294967295])\ndebuglog fact(20)\ndebuglog count(3)\ndebuglog count(100)\n",
    "type E = enum { atom A, term B u8, term C (u8, bool) }\nfun f(e: E): u8\n  match e\n  case atom A\n    ret : u8 / 1\n  case term B x\n    ret x\n  case term C p\n    if p.1\n      ret p.0\n    end if\n    ret : u8 / 0\n  end match\nend fun\nfun g(o: ?u8): u8\n  if o |v|\n    ret v\n  else\n    ret : u8 / 9\n  end if\nend fun\nfun h(r: !u8): !u8\n  let v = r!\n  ret ok (v *! (: u8 / 2))\nend fun\nlet t: [(u8, bool)] = [(1, true), (2, false)]\ndebuglog f(enum { atom A })\ndebuglog f(enum { term B 5 })\ndebuglog f(enum { term C (7, true) })\ndebuglog f(enum { term C (7, false) })\ndebuglog g(some 3)\ndebuglog g(none)\ndebuglog h(ok 100)\ndebuglog h(ok 200)\ndebuglog t\ndebuglog t[1]!\nvar m: [i16] = [1, 2, 3]\nset m[0]! = : i16 / -5\nset m[2]! +!= : i16 / 10\ndebuglog m\ndebuglog 1\nlet z = h(ok 200)!\ndebuglog 2\n",
    "fun seti(mut i: index): index\n  set i = : index / 1\n  ret : index / 0\nend fun\nvar xss: [[u32]] = [[: u32 / 1, : u32 / 2], [: u32 / 3, : u32 / 4]]\nvar i: index = : index / 0\nset xss[i]![seti(mut i)]! = : u32 / 99\ndebuglog xss\nvar j: index = : index / 0\nset xss[j]![seti(mut j)]! +!= : u32 / 50\ndebuglog xss\nvar k: index = : index / 0\ndebuglog xss[k]![seti(mut k)]!\nvar xs: [int] = [1000000000000000000000000000001, 2]\ndebuglog (: [int] / xs)[(: index / 1)]!\ndebuglog xs\nvar s: string = \"a-long-heap-string-0123456789-abcdefghij\"\nlet t = (: string / s)@\ndebuglog t\ndebuglog s\n",
    "fun f(mut x: u8, y: u8): u8\n  set x = : u8 / 9\n  ret y\nend fun\nfun g(y: u8, mut x: u8): u8\n  set x = : u8 / 9\n  ret y\nend fun\nfun h(mut xs: [u8], y: u8): u8\n  set xs = [y, y]\n  ret y\nend fun\nvar a: u8 = : u8 / 1\ndebuglog f(mut a, a)\ndebuglog a\nset a = : u8 / 1\ndebuglog g(a, mut a)\ndebuglog a\nset a = : u8 / 1\ndebuglog f(mut a, : u8 / a) +! f(mut a, a)\ndebuglog a\nvar xs: [u8] = [: u8 / 3, : u8 / 4]\ndebuglog h(mut xs, xs[(: index / 1)]!)\ndebuglog xs\n",
    "fun set9(mut x: u8): u8\n  set x = : u8 / 9\n  ret : u8 / 0\nend fun\nfun put(out x: u8)\n  set x = : u8 / 7\nend fun\nfun bump(mut i: index): u8\n  set i = : index / 1\n  ret : u8 / 0\nend fun\nfun pair(mut a: u8, mut b: u8)\n  set a = : u8 / 1\n  set b = : u8 / 2\nend fun\nvar xs: [u8] = [: u8 / 3, : u8 / 4, : u8 / 5]\ndebuglog set9(mut xs[(: index / 1)]!)\ndebuglog xs\ncall put(out xs[(: index / 2)]!)\ndebuglog xs\nvar p: (u8, (u8, bool)) = (: u8 / 1, (: u8 / 2, true))\ndebuglog set9(mut p.1.0)\ndebuglog p\ncall pair(mut p.0, mut xs[(: index / 0)]!)\ndebuglog (p, xs)\nvar xss: [[u8]] = [[: u8 / 1, : u8 / 2], [: u8 / 3, : u8 / 4]]\nvar i: index = : index / 0\ndebuglog set9(mut xss[i]![(: index / 1)]!) +! bump(mut i)\ndebuglog (xss, i)\nvar ys: [u8] = [: u8 / 1]\ndebuglog set9(mut ys[(: index / 5)]!)\n",
];

/// Literals of the types a driver can pass, each written with its type.
const ARG_LITERALS: &[(&str, &[&str])] = &[
    ("u8", &["0", "1", "200", "255"]),
    ("i8", &["-128", "-1", "0", "127"]),
    ("u16", &["0", "300", "65535"]),
    ("i16", &["-32768", "5", "32767"]),
    ("u32", &["0", "70000", "4294967295"]),
    ("i32", &["-2147483648", "0", "2147483647"]),
    ("u64", &["0", "5", "18446744073709551615"]),
    ("i64", &["-9223372036854775808", "-5", "9223372036854775807"]),
    ("int", &["-1000", "0", "12345678901234567890"]),
    ("index", &["0", "3"]),
    ("offset", &["-2", "0", "5"]),
    ("bool", &["true", "false"]),
];

/// A script's driver: for each function that is not generic and whose
/// parameters all take a literal, a top-level call with literals of their
/// types, a few times over, logged. A `mut` argument is a variable set to a
/// literal, an `out` one a variable with none; the call is then a binding's
/// value, and the variables are logged after it.
///
/// A call whose one argument written back is `mut` is also run where the
/// variable is read before and after it in the same expression, by a tuple
/// and by `==`, hinted too, twice in one, and beside itself given to an `in`
/// parameter; and where the variable is an
/// `index`, as (or as giving) the inner key of a `set` that reads the variable
/// as its outer key, plain and compound. What each sees of the variable is
/// the evaluation order.
fn driver(rng: &mut SeededRng, text: &str) -> String {
    let mut out = String::new();
    let mut fresh = 0;
    if !text.trim().is_empty() {
        out += &shaped_calls(rng);
    }
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("fun ") else { continue };
        let Some(open) = rest.find('(') else { continue };
        let name = &rest[..open];
        if name.contains('<') {
            continue;
        }
        let Some(close) = rest.rfind(')') else { continue };
        let params = &rest[open + 1..close];
        let ret = rest[close + 1..].strip_prefix(": ").map(str::trim);
        let mut parsed = Vec::new();
        let mut ok = true;
        if !params.trim().is_empty() {
            for p in params.split(", ") {
                let Some((head, ty)) = p.split_once(": ") else { ok = false; break };
                let mode = match head.split_once(' ') {
                    None => "",
                    Some(("mut", _)) => "mut",
                    Some(("out", _)) => "out",
                    Some(_) => { ok = false; break }
                };
                match ARG_LITERALS.iter().find(|(t, _)| *t == ty.trim()) {
                    Some(entry) => parsed.push((mode, entry)),
                    None => { ok = false; break }
                }
            }
        }
        if !ok {
            continue;
        }
        let writes = parsed.iter().any(|(m, _)| !m.is_empty());
        for _ in 0..3 {
            let mut decls = String::new();
            let mut written = Vec::new();
            let args: Vec<String> = parsed.iter().map(|(mode, (t, lits))| {
                let lit = lits[rng.random_range(0..lits.len())];
                match *mode {
                    "" => fmt!(": {t} / {lit}"),
                    m => {
                        fresh += 1;
                        let v = fmt!("w{fresh}");
                        written.push(v.C());
                        // A hint on what is written back says only its type,
                        // on the place or on a place along the way to it.
                        match rng.random_range(0..10) {
                            0..=6 => {
                                if m == "mut" {
                                    decls += &fmt!("var {v}: {t} = : {t} / {lit}\n");
                                } else {
                                    decls += &fmt!("var {v}: {t}\n");
                                }
                                if rng.random_bool(0.4) { fmt!("{m} : {t} / {v}") } else { fmt!("{m} {v}") }
                            }
                            7 => {
                                decls += &fmt!("var {v}: {{a: {t}, b: u8}} = {{a = : {t} / {lit}, b = : u8 / 1}}\n");
                                fmt!("{m} (: {{a: {t}, b: u8}} / {v}).a")
                            }
                            8 => {
                                decls += &fmt!("var {v}: ({t}, u8) = (: {t} / {lit}, : u8 / 1)\n");
                                fmt!("{m} (: ({t}, u8) / {v}).0")
                            }
                            _ => {
                                decls += &fmt!("var {v}: [{t}] = [: {t} / {lit}]\n");
                                fmt!("{m} (: [{t}] / {v})[(: index / 0)]!")
                            }
                        }
                    }
                }
            }).collect();
            if writes {
                fresh += 1;
                out += &decls;
                out += &fmt!("let w{fresh} = {name}({})\ndebuglog w{fresh}\n", args.join(", "));
                for v in written {
                    out += &fmt!("debuglog {v}\n");
                }
            } else {
                out += &fmt!("debuglog {name}({})\n", args.join(", "));
            }
        }
        let Some(m) = parsed.iter().position(|(mode, _)| *mode == "mut") else { continue };
        if parsed.iter().filter(|(mode, _)| !mode.is_empty()).count() != 1 {
            continue;
        }
        let t = parsed[m].1.0;
        let lit_of = |rng: &mut SeededRng, lits: &[&str]| lits[rng.random_range(0..lits.len())].S();
        let call_with = |rng: &mut SeededRng, v: &str| {
            let args: Vec<String> = parsed.iter().enumerate().map(|(i, (mode, (t, lits)))| match *mode {
                _ if i == m => fmt!("mut {v}"),
                "" => fmt!(": {t} / {}", lit_of(rng, lits)),
                _ => unreachable!(),
            }).collect();
            fmt!("{name}({})", args.join(", "))
        };
        fresh += 1;
        let v = fmt!("w{fresh}");
        out += &fmt!("var {v}: {t} = : {t} / {}\n", lit_of(rng, parsed[m].1.1));
        let (c1, c2) = (call_with(rng, &v), call_with(rng, &v));
        out += &fmt!("debuglog ({v}@, (: {t} / {v})@, {c1}, {v}@)\ndebuglog {v}\n");
        out += &fmt!("debuglog ({c1}, {c2})\ndebuglog {v}\n");
        // The variable also given to each `in` parameter of its type, which
        // reads it as it was before the call where it is a copy; an `int`
        // is not, and is O010.
        if t != "int" && parsed.iter().enumerate().any(|(i, (mode, (pt, _)))| i != m && mode.is_empty() && *pt == t) {
            let args: Vec<String> = parsed.iter().enumerate().map(|(i, (mode, (pt, lits)))| match *mode {
                _ if i == m => fmt!("mut {v}"),
                _ if *pt == t => v.C(),
                _ => fmt!(": {pt} / {}", lit_of(rng, lits)),
            }).collect();
            out += &fmt!("debuglog {name}({})\ndebuglog {v}\n", args.join(", "));
        }
        if ret == Some(t) {
            let c3 = call_with(rng, &v);
            out += &fmt!("debuglog {v} == {c1}\ndebuglog (: {t} / {v}) == {c2}\ndebuglog {c3} == {v}\ndebuglog {v}\n");
        }
        if let (Some(ret), "index") = (ret, t) {
            // Keys a `set` can fail at return early, so in a function of its
            // own, which writes back what it did.
            let row = |r: u32| (0..4).map(|c| fmt!(": u8 / {}", r * 4 + c)).collect::<Vec<_>>().join(", ");
            let grid = (0..4).map(|r| fmt!("[{}]", row(r))).collect::<Vec<_>>().join(", ");
            for op in ["=", "+!="] {
                fresh += 1;
                let f = fmt!("w{fresh}");
                let c = call_with(rng, "k");
                // The call's value, whatever its type, as an index.
                let key = if ret == "index" {
                    c
                } else {
                    out += &fmt!("fun {f}k(x: {ret}): index\n  ret : index / 2\nend fun\n");
                    fmt!("{f}k({c})")
                };
                out += &fmt!("fun {f}(mut ls: [[u8]], mut k: index): !()\n  set ls[k]![{key}]! {op} : u8 / 50\n  ret ok ()\nend fun\n");
                let k0 = lit_of(rng, &["0", "1", "3"]);
                out += &fmt!("var {f}a: [[u8]] = [{grid}]\nvar {f}b: index = : index / {k0}\n");
                out += &fmt!("let {f}c = {f}(mut {f}a, mut {f}b)\ndebuglog {f}c\ndebuglog {f}a\ndebuglog {f}b\n");
            }
        }
    }
    out
}

/// Callees that write their `mut` parameter and then read another, called
/// with one variable, or a part of one, given to both, and with parts of one
/// variable given to each, sibling fields written together among them, at a copy type picked by `rng`: what the callee
/// reads is whether an `in` argument was read before the call, as the model
/// reads it, or through a pointer to what it writes.
fn shaped_calls(rng: &mut SeededRng) -> String {
    let copies: Vec<&(&str, &[&str])> = ARG_LITERALS.iter().filter(|(t, _)| *t != "int").collect();
    let (t, lits) = copies[rng.random_range(0..copies.len())];
    let mut lit = || fmt!(": {t} / {}", lits[rng.random_range(0..lits.len())]);
    let (a, b, c) = (lit(), lit(), lit());
    let mut out = String::new();
    out += &fmt!("fun wsa(mut x: {t}, y: {t}): {t}\n  set x = {a}\n  ret y\nend fun\n");
    out += &fmt!("fun wsb(y: {t}, mut x: {t}): {t}\n  set x = {a}\n  ret y\nend fun\n");
    out += &fmt!("fun wsc(mut x: {t}, mut y: {t}): {t}\n  set x = {a}\n  set y = x\n  ret x\nend fun\n");
    out += &fmt!("var wsv: {t} = {b}\ndebuglog wsa(mut wsv, wsv)\ndebuglog wsv\n");
    out += &fmt!("set wsv = {b}\ndebuglog wsb(wsv, mut wsv)\ndebuglog wsv\n");
    out += &fmt!("set wsv = {c}\ndebuglog (wsv, wsa(mut wsv, wsv), wsv)\n");
    out += &fmt!("var wsl: [{t}] = [{b}, {c}]\n");
    out += &fmt!("debuglog wsa(mut wsl[(: index / 0)]!, wsl[(: index / 1)]!)\ndebuglog wsl\n");
    out += &fmt!("debuglog wsb(wsl[(: index / 1)]!, mut wsl[(: index / 1)]!)\ndebuglog wsl\n");
    out += &fmt!("var wsp: ({t}, {t}) = ({c}, {b})\n");
    out += &fmt!("debuglog wsa(mut wsp.1, wsp.1)\ndebuglog wsp\n");
    out += &fmt!("var wsw: {t} = {c}\ndebuglog wsc(mut wsp.0, mut wsw)\ndebuglog (wsp, wsw)\n");
    // Written back through a hint, to a variable and to parts.
    out += &fmt!("debuglog wsc(mut : {t} / wsl[(: index / 0)]!, mut : {t} / wsw)\ndebuglog (wsl, wsw)\n");
    out += &fmt!("debuglog wsa(mut : {t} / wsp.1, wsp.0)\ndebuglog wsp\n");
    // Sibling fields are distinct places: one written beside another, and
    // one held by an earlier argument while a later one writes the other.
    out += &fmt!("set wsp = ({b}, {c})\ndebuglog wsc(mut wsp.0, mut wsp.1)\ndebuglog wsp\n");
    out += &fmt!("set wsp = ({c}, {b})\ndebuglog wsa(mut wsp.1, wsb(wsp.0, mut wsp.0))\ndebuglog wsp\n");
    out
}

/// An engine `runs_agree` runs a script on.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Engine {
    /// The interpreter's register bytecode, what `script` runs by default.
    Bytecode,
    /// The IR walker every other engine is tested against.
    Ir,
    /// The JIT, compiling each function at its first call and entering a
    /// loop's compiled code after its first pass.
    Jit,
    /// Cranelift's AOT backend, linked into an executable.
    CraneliftAot,
    /// The C backend, compiled by the system's C compiler.
    CAot,
}

impl Engine {
    const ALL: [Engine; 5] = [Engine::Bytecode, Engine::Ir, Engine::Jit, Engine::CraneliftAot, Engine::CAot];

    fn is_aot(self) -> bool {
        matches!(self, Engine::CraneliftAot | Engine::CAot)
    }
}

/// What running a script with the CLI came to.
enum RustRun {
    /// What its `debuglog`s printed, and whether it exited successfully.
    Ran(Vec<String>, bool),
    /// A phase after type checking refused it, the ownership analysis say.
    Refused(String),
    /// It ran past the time it was given.
    TimedOut,
    /// An AOT backend failed to build it, with no diagnostic: what it printed.
    BuildFailed(String),
}

/// A command run for at most `limit`: how it exited, none if it ran out of
/// time and was killed, and what it wrote to stderr, uncolored.
fn run_bounded(mut cmd: std::process::Command, limit: std::time::Duration)
    -> (Option<std::process::ExitStatus>, Vec<String>)
{
    let mut child = cmd
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn().X();
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().X() {
            break Some(status);
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    let mut stderr = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().X(), &mut stderr).X();
    // Diagnostics are colored whatever the environment says.
    let ansi = rmx::regex::Regex::new("\x1b\\[[0-9;]*m").X();
    (status, stderr.lines().map(|l| ansi.replace_all(l, "").into_owned()).collect())
}

/// The first line, where it is a diagnostic: `[CODE] Error: ...`, which no
/// value prints as.
fn diagnostic(lines: &[String]) -> Option<&String> {
    lines.first().filter(|l| {
        let b = l.as_bytes();
        b.first() == Some(&b'[') && b.get(1).is_some_and(u8::is_ascii_uppercase) && b.contains(&b']')
            && l.contains("] Error:")
    })
}

/// A script run with the CLI on the bytecode interpreter.
fn rust_run(text: &str) -> RustRun {
    rust_run_on(text, Engine::Bytecode)
}

/// A script run with the CLI on `engine`. An AOT backend builds an
/// executable, which is then run, so that running out of time kills the
/// program and not only the CLI that started it. Every run checks for leaks,
/// whether or not the tests were started with `DATALOVE_LEAK_CHECK`: a
/// program that leaks prints the leak, and so does not agree.
fn rust_run_on(text: &str, engine: Engine) -> RustRun {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().X().parent().X();
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    let binary = workspace.join("target").join(profile).join("datalove");
    assert!(binary.exists(), "{} not built; `just test-formal` builds it", binary.display());
    let dir = rmx::tempfile::tempdir().X();
    let path = dir.path().join("script.dl");
    std::fs::write(&path, text).X();
    let run_limit = std::time::Duration::from_secs(5);
    let mut cmd = std::process::Command::new(&binary);
    cmd.env("DATALOVE_LEAK_CHECK", "panic");
    let (status, lines) = match engine {
        Engine::Bytecode => {
            cmd.arg("script").arg(&path);
            run_bounded(cmd, run_limit)
        }
        Engine::Ir => {
            cmd.env("DATALOVE_INTERP", "ir").arg("script").arg(&path);
            run_bounded(cmd, run_limit)
        }
        Engine::Jit => {
            cmd.args(["script", "--jit", "--jit-threshold", "1", "--jit-osr-threshold", "1"]).arg(&path);
            run_bounded(cmd, run_limit)
        }
        Engine::CraneliftAot | Engine::CAot => {
            let exe = dir.path().join("script");
            cmd.arg("aot-compile").arg("--link");
            if engine == Engine::CAot {
                cmd.arg("--c");
            }
            cmd.arg("-o").arg(&exe).arg(&path);
            let (status, lines) = run_bounded(cmd, std::time::Duration::from_secs(120));
            if let Some(line) = diagnostic(&lines) {
                return RustRun::Refused(line.C());
            }
            if !status.is_some_and(|s| s.success()) {
                return RustRun::BuildFailed(lines.join("\n"));
            }
            let mut run = std::process::Command::new(&exe);
            run.env("DATALOVE_LEAK_CHECK", "panic");
            run_bounded(run, run_limit)
        }
    };
    let Some(status) = status else { return RustRun::TimedOut };
    match diagnostic(&lines) {
        Some(line) => RustRun::Refused(line.C()),
        None => RustRun::Ran(lines, status.success()),
    }
}

/// Whether a line the Rust printed is the one the model printed, what the
/// model does not know (U+0001) matching anything.
fn line_matches(model: &str, rust: &str) -> bool {
    let parts: Vec<&str> = model.split('\u{1}').collect();
    if parts.len() == 1 {
        return model == rust;
    }
    let pattern = parts.iter().map(|p| regex_escape(p)).collect::<Vec<_>>().join(".*");
    rmx::regex::Regex::new(&fmt!("^{pattern}$")).X().is_match(rust)
}

fn regex_escape(s: &str) -> String {
    rmx::regex::escape(s)
}

/// Generated scripts that check, with a driver calling their functions,
/// run by the model and by the Rust on each engine: each top-level
/// `debuglog` prints the same, up to what the model does not know of a value,
/// and a script the model runs to its end, or to an early return, the Rust
/// does too, printing nothing more. Where the model stops at what it does not
/// run, what it printed before is what the Rust printed first.
#[test]
fn runs_agree() {
    if std::env::var("FORMAL_ORACLE_TEST").is_err() {
        println!("Skipping the run tests (set FORMAL_ORACLE_TEST=1 to run)");
        return;
    }
    let seeds = bct::random::env_seeds("FORMAL_FUN", 0, 300);
    let mut config = config();
    config.debuglog_probability = 0;
    config.param_mode_probability = 30;
    let mut cases = Vec::new();
    for text in RUN_CORPUS {
        let (codes, script, _) = rust_side(text);
        assert!(codes.is_empty(), "a run corpus script does not check: {codes:?}\n{text}");
        cases.push(("corpus".S(), text.S(), script));
    }
    for seed in seeds.iter() {
        let session = datalove_worldgen::gen_session_seeded(seed, config.C());
        let text = session.modules.first().map(|m| m.1.C()).unwrap_or_default();
        let mut rng = SeededRng::derive(seed, "drive");
        // As generated, and changed as `statement_typing_agrees` changes it,
        // where that still checks: the changes compute from the parameters.
        let mut mutated = text.C();
        let mut mrng = SeededRng::derive(seed, "mutate");
        for _ in 0..mrng.random_range(1..=MAX_MUTATIONS) {
            mutated = mutate(&mut mrng, &mutated);
        }
        for (kind, text) in [("", text), (", changed", mutated)] {
            // The model keeps only the top level's `debuglog`s.
            let text: String = text.lines()
                .filter(|l| !(l.starts_with(' ') && l.trim_start().starts_with("debuglog ")))
                .map(|l| fmt!("{l}\n"))
                .collect();
            let drive = driver(&mut rng, &text);
            if drive.is_empty() {
                continue;
            }
            let text = fmt!("{}\n{drive}", text.trim_end());
            let (codes, script, _) = rust_side(&text);
            if codes.is_empty() {
                cases.push((fmt!("seed {seed}{kind}"), text, script));
            }
        }
    }
    let queries: Vec<Value> = cases.iter().map(|(_, _, script)| json!({ "run": script })).collect();
    let answers = ask_oracle(&queries);

    // What the model ran each script to: what it logged, the value it
    // returned early printed last, as the CLI prints it, and how it ended.
    let mut ends = BTreeMap::<String, usize>::new();
    let mut models = Vec::new();
    for (i, ((what, text, _), oracle)) in cases.iter().zip(&answers).enumerate() {
        if let Some(why) = oracle.get("unsupported") {
            assert!(what != "corpus", "the model does not take a run corpus script ({why}):\n{text}");
            *ends.entry(fmt!("unsupported script: {why}")).or_default() += 1;
            continue;
        }
        let mut model: Vec<String> = oracle["log"].as_array().X().iter().map(|l| l.as_str().X().S()).collect();
        let end = oracle["end"].as_str().X().S();
        if let Some(returned) = oracle["returned"].as_str() {
            model.push(returned.S());
        }
        *ends.entry(end.C()).or_default() += 1;
        let finished = end == "ok" || end == "early";
        // A corpus script is written to be run to its end: one the model
        // stops in is a gap in the model, not a script to compare a prefix of.
        assert!(finished || what != "corpus", "the model stops in a run corpus script ({end}):\n{text}");
        models.push((i, model, end, finished));
    }

    // Every script runs on the interpreters and the JIT; the AOT backends,
    // which take a C or Cranelift build and a link a script, run the corpus
    // and one generated script in `FORMAL_FUN_AOT_EVERY`.
    let aot_every: usize = std::env::var("FORMAL_FUN_AOT_EVERY").map(|v| v.parse().X()).unwrap_or(8);
    let jobs: Vec<(usize, Engine)> = models.iter().enumerate()
        .flat_map(|(n, (i, ..))| {
            let aot = cases[*i].0 == "corpus" || (aot_every != 0 && n % aot_every == 0);
            Engine::ALL.into_iter().filter(move |e| aot || !e.is_aot()).map(move |e| (n, e))
        })
        .collect();
    let runs: Vec<RustRun> = {
        use rmx::rayon::prelude::*;
        jobs.par_iter().map(|&(n, e)| rust_run_on(&cases[models[n].0].1, e)).collect()
    };

    let mut lines = 0;
    let mut unknown = 0;
    let mut writing = 0;
    let mut per_engine = BTreeMap::<Engine, usize>::new();
    let mut mismatches = Vec::new();
    let mut refused_on = BTreeMap::<usize, String>::new();
    for (&(n, engine), rust) in jobs.iter().zip(&runs) {
        let (i, model, end, finished) = &models[n];
        let (what, text, _) = &cases[*i];
        let (agree, out) = match rust {
            RustRun::Ran(out, success) => {
                let prefix = model.len() <= out.len()
                    && model.iter().zip(out).all(|(m, r)| line_matches(m, r));
                (if *finished { prefix && model.len() == out.len() && *success } else { prefix }, fmt!("{out:?}"))
            }
            // A later phase refuses a script on every engine alike, before
            // any of them runs it.
            RustRun::Refused(line) => {
                assert!(what != "corpus", "the Rust refuses a run corpus script: {line}\n{text}");
                let code = line[..line.find(']').X() + 1].S();
                if engine == Engine::Bytecode {
                    *ends.entry(fmt!("refused: {code}")).or_default() += 1;
                }
                refused_on.insert(n, code);
                continue;
            }
            // Out of time: the model must not have finished.
            RustRun::TimedOut => (!finished, "timed out".S()),
            RustRun::BuildFailed(why) => (false, fmt!("build failed: {why}")),
        };
        *per_engine.entry(engine).or_default() += 1;
        if !agree {
            mismatches.push(fmt!("{what} on {engine:?}:\n{text}    model ({end}): {model:?}\n    rust: {out}"));
        } else if engine == Engine::Bytecode {
            if text.contains("mut w") || text.contains("out w") {
                writing += 1;
            }
            lines += model.len();
            unknown += model.iter().filter(|l| l.contains('\u{1}')).count();
        }
    }
    // A script one engine refuses and another runs.
    for (&(n, engine), rust) in jobs.iter().zip(&runs) {
        if refused_on.contains_key(&n) && !matches!(rust, RustRun::Refused(_)) {
            mismatches.push(fmt!("{} on {engine:?} runs what another engine refuses ({}):\n{}",
                cases[models[n].0].0, refused_on[&n], cases[models[n].0].1));
        }
    }
    println!(
        "{} scripts run over seeds {}..{}: {} differ; {lines} lines agree, {unknown} with what the model does not know; {writing} writing back through a call; runs by engine {per_engine:?}; ends {ends:?}",
        cases.len(), seeds.start, seeds.start.wrapping_add(seeds.count), mismatches.len(),
    );
    for m in mismatches.iter().take(10) {
        println!("{m}");
    }
    assert!(mismatches.is_empty(), "the Rust and the model run {} scripts differently", mismatches.len());
}


/// Print what the model's run of `FORMAL_FUN_RUN_OF`, a file, logs and how it
/// ends, with the codes the Rust gives it, to find where the model stops.
#[test]
fn run_of() {
    let Ok(path) = std::env::var("FORMAL_FUN_RUN_OF") else { return };
    let (codes, script, _) = rust_side(&std::fs::read_to_string(path).X());
    println!("rust:  {codes:?}");
    println!("model: {}", ask_oracle(&[json!({ "run": script })])[0]);
}

/// The places and values a call's arguments are drawn from in
/// `aliasing_agrees`, each with its type: a variable, its fields, fields of
/// fields, elements, the same through a hint, fields of a hinted place, and
/// values that are no place.
const ALIAS_ARGS: &[(&str, &str)] = &[
    ("n", "u8"), ("s", "string"), ("r", "R"), ("r.x", "u8"), ("r.s", "string"), ("r.l", "[u8]"),
    ("r.l[0]!", "u8"), ("r.l[1]!", "u8"), ("r.t", "(u8, string)"), ("r.t.0", "u8"), ("r.t.1", "string"),
    ("ls", "[string]"), ("ls[0]!", "string"), (": u8 / r.x", "u8"), (": string / r.s", "string"),
    ("(: R / r).x", "u8"), ("(: R / r).s", "string"), ("(: R / r).t.1", "string"),
    ("n +! 1", "u8"), ("s@", "string"), ("r.t.1@", "string"),
];

/// What an `out` parameter of each type in `ALIAS_ARGS` is set to.
fn alias_default(ty: &str) -> &'static str {
    match ty {
        "u8" => "0",
        "string" => "\"x\"",
        "R" => "{x = 0, s = \"x\", l = [], t = (0, \"x\")}",
        "[u8]" | "[string]" => "[]",
        "(u8, string)" => "(0, \"x\")",
        _ => unreachable!("a type of ALIAS_ARGS"),
    }
}

/// A script of one call whose arguments may name overlapping places, under
/// parameters of modes drawn at random.
fn alias_script(rng: &mut SeededRng) -> String {
    let count = rng.random_range(2..=3);
    let mut params = Vec::new();
    let mut sets = Vec::new();
    let mut args = Vec::new();
    for i in 0..count {
        let (arg, ty) = ALIAS_ARGS[rng.random_range(0..ALIAS_ARGS.len())];
        let mode = ["", "", "ref ", "mut ", "out "][rng.random_range(0..5)];
        let name = ["a", "b", "c"][i];
        params.push(fmt!("{mode}{name}: {ty}"));
        if mode == "out " {
            sets.push(fmt!("    set {name} = {}\n", alias_default(ty)));
        }
        args.push(fmt!("{mode}{arg}"));
    }
    fmt!(
        "type R = {{x: u8, s: string, l: [u8], t: (u8, string)}}\n\
         fun f({})\n{}end fun\n\
         var n: u8 = 1\n\
         var s: string = \"a-long-heap-string-0123456789\"\n\
         var r: R = {{x = 1, s = \"a-long-heap-string-0123456789\", l = [1, 2], t = (2, \"a-long-heap-string-0123456789\")}}\n\
         var ls: [string] = [\"a-long-heap-string-0123456789\", \"b\"]\n\
         call f({})\n",
        params.join(", "), sets.concat(), args.join(", "),
    )
}

/// O010 as the model decides it (`formal/Datalove/FunAlias.lean`) held to the
/// Rust's, on calls whose arguments name overlapping places: the model says
/// a call is O010 where, and only where, the CLI refuses it with O010. A
/// script the model's checker refuses, or the Rust refuses with another
/// code first, is counted and skipped.
#[test]
fn aliasing_agrees() {
    if std::env::var("FORMAL_ORACLE_TEST").is_err() {
        println!("Skipping the aliasing tests (set FORMAL_ORACLE_TEST=1 to run)");
        return;
    }
    let seeds = bct::random::env_seeds("FORMAL_FUN", 0, 300);
    let mut cases = Vec::new();
    for seed in seeds.iter() {
        let mut rng = SeededRng::derive(seed, "alias");
        let text = alias_script(&mut rng);
        let (_, script, _) = rust_side(&text);
        cases.push((fmt!("seed {seed}"), text, script));
    }
    let queries: Vec<Value> = cases.iter().map(|(_, _, script)| json!({ "alias": script })).collect();
    let answers = ask_oracle(&queries);

    let mut outcomes = BTreeMap::<String, usize>::new();
    let mut mismatches = Vec::new();
    for ((what, text, _), oracle) in cases.iter().zip(&answers) {
        if let Some(why) = oracle.get("unsupported") {
            *outcomes.entry(fmt!("unsupported: {why}")).or_default() += 1;
            continue;
        }
        let codes = oracle["codes"].as_array().X();
        if !codes.is_empty() {
            *outcomes.entry("model checker refuses".S()).or_default() += 1;
            continue;
        }
        let model = oracle["calls"][0].as_bool().X();
        let rust = match rust_run(text) {
            RustRun::Refused(line) if line.starts_with("[O010]") => true,
            RustRun::Refused(line) => {
                *outcomes.entry(fmt!("rust refuses {}", &line[..line.find(']').X() + 1])).or_default() += 1;
                continue;
            }
            RustRun::Ran(..) => false,
            RustRun::TimedOut => panic!("a call of an empty function timed out:\n{text}"),
            RustRun::BuildFailed(_) => unreachable!("only an AOT backend builds"),
        };
        *outcomes.entry(if rust { "O010" } else { "accepted" }.S()).or_default() += 1;
        if model != rust {
            mismatches.push(fmt!("{what}:\n{text}    rust O010: {rust}, model O010: {model}"));
        }
    }
    println!(
        "{} calls over seeds {}..{}: {} differ; {outcomes:?}",
        cases.len(), seeds.start, seeds.start.wrapping_add(seeds.count), mismatches.len(),
    );
    for m in mismatches.iter().take(10) {
        println!("{m}");
    }
    assert!(mismatches.is_empty(), "the Rust and the model decide O010 differently on {} calls", mismatches.len());
}

/// The declarations every `held_agrees` script starts with: functions that
/// write a `u8`, an `index`, a list or a string through `mut` or `out`, read
/// one by `ref`, or take one, and variables of those types and of a struct of
/// them.
const HELD_PRELUDE: &str = "\
type R = {x: u8, s: string, l: [u8], t: (u8, string)}
fun set9(mut x: u8): u8
  set x = : u8 / 9
  ret : u8 / 0
end fun
fun put(out x: u8): u8
  set x = : u8 / 7
  ret : u8 / 0
end fun
fun grow(mut xs: [u8]): index
  set xs = [: u8 / 1]
  ret : index / 0
end fun
fun ms(mut s: string): string
  set s = \"m\"
  ret \"r\"
end fun
fun eat(s: string): index
  ret : index / 0
end fun
fun look(ref s: string): index
  ret : index / 0
end fun
fun seti(mut i: index): index
  set i = : index / 0
  ret : index / 0
end fun
fun pick(ref a: string, b: u8): u8
  ret b
end fun
fun keep(ref a: string, b: string): string
  ret b
end fun
fun add9(mut a: u8, b: u8): u8
  set a = : u8 / 9
  ret b
end fun
fun mr(mut q: R): u8
  set q.x = : u8 / 5
  ret : u8 / 0
end fun
fun lr(ref q: R, b: u8): u8
  ret b
end fun
var n: u8 = : u8 / 1
var i: index = : index / 0
var s: string = \"a\"
var r: R = {x = : u8 / 1, s = \"b\", l = [: u8 / 2, : u8 / 3], t = (: u8 / 4, \"c\")}
var xs: [u8] = [: u8 / 1, : u8 / 2]
var ls: [string] = [\"a\", \"b\"]
";

/// An expression for `held_agrees` of one of the prelude's types, `depth`
/// calls or operators deep at most: places of the type, and calls that take
/// a place `mut`, `out` or `ref`, or take a value, inside operators,
/// indexes and other calls.
fn held_expr(rng: &mut SeededRng, ty: &str, depth: u32) -> String {
    let pick = |rng: &mut SeededRng, xs: &[&str]| xs[rng.random_range(0..xs.len())].S();
    let compound = depth > 0 && rng.random_range(0..3) != 0;
    let d = depth.saturating_sub(1);
    match (ty, compound) {
        ("u8", false) => match rng.random_range(0..4) {
            0 => fmt!("xs[{}]!", held_expr(rng, "index", d)),
            1 => fmt!("r.l[{}]!", held_expr(rng, "index", d)),
            _ => pick(rng, &["n", "n", "r.x", "r.t.0", "(: u8 / n)", "(: R / r).x"]),
        },
        ("u8", true) => match rng.random_range(0..4) {
            0 => fmt!("set9(mut {})", held_place(rng, "u8", d)),
            1 => fmt!("put(out {})", held_place(rng, "u8", d)),
            2 => fmt!("{} +! {}", held_expr(rng, "u8", d), held_expr(rng, "u8", d)),
            _ => fmt!("pick(ref {}, {})", held_place(rng, "string", d), held_expr(rng, "u8", d)),
        },
        ("index", false) => pick(rng, &["i", "i", "(: index / 0)", "(: index / 1)"]),
        ("index", true) => match rng.random_range(0..4) {
            0 => fmt!("grow(mut {})", held_place(rng, "list", d)),
            1 => fmt!("eat({})", held_expr(rng, "string", d)),
            2 => fmt!("look(ref {})", held_place(rng, "string", d)),
            _ => "seti(mut i)".S(),
        },
        ("string", false) => match rng.random_range(0..4) {
            0 => fmt!("ls[{}]!", held_expr(rng, "index", d)),
            _ => pick(rng, &["s", "s", "s", "r.s", "r.t.1", "(: R / r).s"]),
        },
        ("string", true) => match rng.random_range(0..3) {
            0 => fmt!("ms(mut {})", held_place(rng, "string", d)),
            1 => fmt!("keep(ref {}, {})", held_place(rng, "string", d), held_expr(rng, "string", d)),
            _ => fmt!("{}@", held_expr(rng, "string", d)),
        },
        ("bool", _) => match rng.random_range(0..2) {
            0 => fmt!("{} == {}", held_expr(rng, "string", depth), held_expr(rng, "string", depth)),
            _ => fmt!("{} == {}", held_expr(rng, "u8", depth), held_expr(rng, "u8", depth)),
        },
        ("list", _) => pick(rng, &["xs", "r.l"]),
        _ => unreachable!("no expression of type {ty}"),
    }
}

/// A place of one of the prelude's types, its keys `depth` deep at most.
fn held_place(rng: &mut SeededRng, ty: &str, depth: u32) -> String {
    if ty == "u8" && rng.random_range(0..5) == 0 {
        return "(: R / r).x".S();
    }
    held_target(rng, ty, depth)
}

/// A place a `set` may write, which starts from a variable: one of
/// `held_place`'s but a hinted one.
fn held_target(rng: &mut SeededRng, ty: &str, depth: u32) -> String {
    let pick = |rng: &mut SeededRng, xs: &[&str]| xs[rng.random_range(0..xs.len())].S();
    match ty {
        "u8" => match rng.random_range(0..4) {
            0 => fmt!("xs[{}]!", held_expr(rng, "index", depth)),
            1 => fmt!("r.l[{}]!", held_expr(rng, "index", depth)),
            _ => pick(rng, &["n", "n", "r.x", "r.t.0"]),
        },
        "string" => match rng.random_range(0..4) {
            0 => fmt!("ls[{}]!", held_expr(rng, "index", depth)),
            _ => pick(rng, &["s", "s", "s", "r.s", "r.t.1"]),
        },
        "list" => pick(rng, &["xs", "xs", "r.l"]),
        _ => unreachable!("no place of type {ty}"),
    }
}

/// A place of `r` or of a variable, of `ty` or of all of `R`, near the
/// places the others name.
fn held_near(rng: &mut SeededRng, ty: &str) -> String {
    let pick = |rng: &mut SeededRng, xs: &[&str]| xs[rng.random_range(0..xs.len())].S();
    match ty {
        "string" => pick(rng, &["s", "r.s", "r.t.1", "ls[(: index / 0)]!", "ls[i]!"]),
        "u8" => pick(rng, &["n", "r.x", "r.t.0", "r.l[(: index / 0)]!", "xs[i]!"]),
        "list" => pick(rng, &["xs", "r.l"]),
        _ => unreachable!("no place of type {ty}"),
    }
}

/// An expression that writes a place near `held_near`'s, or moves one.
fn held_write(rng: &mut SeededRng, ty: &str) -> String {
    match (ty, rng.random_range(0..3)) {
        ("u8", 0) => fmt!("set9(mut {})", held_near(rng, "u8")),
        ("u8", 1) => fmt!("put(out {})", held_near(rng, "u8")),
        ("u8", _) => "mr(mut r)".S(),
        ("string", 0) => "s".S(),
        ("string", _) => fmt!("ms(mut {})", held_near(rng, "string")),
        ("index", 0) => fmt!("grow(mut {})", held_near(rng, "list")),
        ("index", 1) => "eat(s)".S(),
        ("index", _) => "seti(mut i)".S(),
        _ => unreachable!("no write of type {ty}"),
    }
}

/// A statement that holds a place while it writes or moves one near it: an
/// operator's operand, a `ref` or `mut` argument, an index's base, a place's
/// keys, or a `set`'s keys and compound value, each held over a write.
fn held_pair(rng: &mut SeededRng) -> String {
    match rng.random_range(0..9) {
        0 => fmt!("debuglog {} == {}", held_near(rng, "string"), held_write(rng, "string")),
        1 => fmt!("debuglog keep(ref {}, {})", held_near(rng, "string"), held_write(rng, "string")),
        2 => fmt!("debuglog pick(ref {}, {})", held_near(rng, "string"), held_write(rng, "u8")),
        3 => fmt!("debuglog add9(mut {}, {})", held_near(rng, "u8"), held_write(rng, "u8")),
        4 => fmt!("debuglog lr(ref r, {})", held_write(rng, "u8")),
        5 => fmt!("debuglog {}[{}]!", held_near(rng, "list"), held_write(rng, "index")),
        6 => fmt!("debuglog ls[{}]! == {}", held_write(rng, "index"), held_write(rng, "string")),
        7 => fmt!("set {}[{}]! = {}", held_near(rng, "list"), held_write(rng, "index"), held_write(rng, "u8")),
        _ => fmt!("set ls[{}]! = {}", held_write(rng, "index"), held_write(rng, "string")),
    }
}

/// A script for `held_agrees`: the prelude and one statement, a `debuglog`
/// of an expression or a `set`, plain or compound, through keys, or one made
/// to hold a place while it writes one near it.
fn held_script(rng: &mut SeededRng) -> String {
    if rng.random_range(0..2) == 0 {
        return fmt!("{HELD_PRELUDE}{}\n", held_pair(rng));
    }
    let depth = rng.random_range(1..=3);
    let stmt = match rng.random_range(0..6) {
        0 | 1 => {
            let ty = ["u8", "string", "index", "bool"][rng.random_range(0..4)];
            fmt!("debuglog {}", held_expr(rng, ty, depth))
        }
        2 => fmt!("set {} = {}", held_target(rng, "u8", depth), held_expr(rng, "u8", depth)),
        3 => fmt!("set {} +!= {}", held_target(rng, "u8", depth), held_expr(rng, "u8", depth)),
        4 => fmt!("set {} = {}", held_target(rng, "string", depth), held_expr(rng, "string", depth)),
        _ => fmt!("set ls[{}]! = {}", held_expr(rng, "index", depth), held_expr(rng, "string", depth)),
    };
    fmt!("{HELD_PRELUDE}{stmt}\n")
}

/// The ownership codes the CLI reports of a script, in order, or none if it
/// ran.
fn ownership_codes(text: &str) -> Vec<String> {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().X().parent().X();
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    let binary = workspace.join("target").join(profile).join("datalove");
    let dir = rmx::tempfile::tempdir().X();
    let path = dir.path().join("script.dl");
    std::fs::write(&path, text).X();
    let mut cmd = std::process::Command::new(&binary);
    cmd.arg("script").arg(&path);
    let (status, lines) = run_bounded(cmd, std::time::Duration::from_secs(5));
    assert!(status.is_some(), "a script of empty functions timed out:\n{text}");
    lines.iter()
        .filter(|l| l.starts_with("[O") && l.contains("] Error:"))
        .map(|l| l[1..l.find(']').X()].S())
        .collect()
}

/// O017 as the model decides it (`formal/Datalove/FunHeld.lean`) held to the
/// Rust's, on statements whose operands and arguments read places that later
/// ones take `mut`, `out` or move: the model says a statement is O017 where,
/// and only where, the CLI reports O017 among its errors. A script the
/// model's checker refuses, or the Rust refuses with other codes alone, is
/// counted and skipped.
#[test]
fn held_agrees() {
    if std::env::var("FORMAL_ORACLE_TEST").is_err() {
        println!("Skipping the held operand tests (set FORMAL_ORACLE_TEST=1 to run)");
        return;
    }
    let seeds = bct::random::env_seeds("FORMAL_FUN", 0, 300);
    let mut cases = Vec::new();
    for seed in seeds.iter() {
        // Three statements a seed, since each is a single line.
        for k in 0..3 {
            let mut rng = SeededRng::derive(seed, &fmt!("held {k}"));
            let text = held_script(&mut rng);
            let (_, script, _) = rust_side(&text);
            cases.push((fmt!("seed {seed}.{k}"), text, script));
        }
    }
    let queries: Vec<Value> = cases.iter().map(|(_, _, script)| json!({ "held": script })).collect();
    let answers = ask_oracle(&queries);

    let checked: Vec<usize> = (0..cases.len()).filter(|&n| {
        answers[n].get("unsupported").is_none() && answers[n]["codes"].as_array().X().is_empty()
    }).collect();
    let rust: Vec<Vec<String>> = {
        use rmx::rayon::prelude::*;
        checked.par_iter().map(|&n| ownership_codes(&cases[n].1)).collect()
    };

    let mut outcomes = BTreeMap::<String, usize>::new();
    for oracle in &answers {
        if let Some(why) = oracle.get("unsupported") {
            *outcomes.entry(fmt!("unsupported: {why}")).or_default() += 1;
        } else if !oracle["codes"].as_array().X().is_empty() {
            *outcomes.entry("model checker refuses".S()).or_default() += 1;
        }
    }
    let mut mismatches = Vec::new();
    for (&n, codes) in checked.iter().zip(&rust) {
        let (what, text, _) = &cases[n];
        let model = answers[n]["held"].as_array().X().last().X().as_bool().X();
        let rust_held = codes.iter().any(|c| c == "O017");
        if !rust_held && !codes.is_empty() {
            *outcomes.entry(fmt!("rust refuses {}", codes[0])).or_default() += 1;
            continue;
        }
        *outcomes.entry(if rust_held { "O017" } else { "accepted" }.S()).or_default() += 1;
        if model != rust_held {
            let stmt = text.lines().last().X();
            mismatches.push(fmt!("{what}: {stmt}\n    rust {codes:?}, model O017: {model}"));
        }
    }
    println!(
        "{} statements over seeds {}..{}: {} differ; {outcomes:?}",
        cases.len(), seeds.start, seeds.start.wrapping_add(seeds.count), mismatches.len(),
    );
    for m in mismatches.iter().take(15) {
        println!("{m}");
    }
    assert!(mismatches.is_empty(), "the Rust and the model decide O017 differently on {} statements", mismatches.len());
}

/// What `init_script` declares before the function it generates: an enum to
/// match on, and functions that write an `out` parameter, one returning a
/// value too, one taking a `mut` one, and one giving an option to unwrap,
/// which `?` leaves through where it is none.
const INIT_PRELUDE: &str = "type E = enum { atom A, atom B }\n\
    fun put(out q: int)\n  set q = 1\nend fun\n\
    fun get(out q: int): int\n  set q = 2\n  ret 3\nend fun\n\
    fun bump(mut q: int)\n  set q = q + 1\nend fun\n\
    fun maybe(c: bool): ?int\n  if c\n    ret some 1\n  end if\n  ret none\nend fun\n";

/// A function's body of statements that give, read and pass on variables
/// declared with no value and an `out` parameter, across branches, loops,
/// `match` arms and early returns: what O005, O006 and O009 decide.
fn init_block(rng: &mut SeededRng, depth: usize, in_loop: bool, out: &mut String) {
    let indent = "  ".repeat(depth + 1);
    let count = rng.random_range(1..=4);
    for _ in 0..count {
        let t = ["x", "y", "p", "x", "p"][rng.random_range(0..5)];
        let nested = depth < 3;
        let pick = rng.random_range(0..if nested { 24 } else { 15 });
        let line = match pick {
            0 | 1 => fmt!("set {t} = 1"),
            2 => fmt!("debuglog {t}"),
            3 => fmt!("let r = {t} + 1"),
            4 => fmt!("call put(out {t})"),
            5 => fmt!("call bump(mut {t})"),
            6 => fmt!("let r = get(out {t}) + 1"),
            7 => fmt!("let r = maybe(c)? + get(out {t})"),
            8 => fmt!("set {t} += 1"),
            9 => fmt!("let r = (get(out {t}), 1)"),
            10 => ["set s.a = 1", "debuglog s.a", "set s = {a = 1}", "call bump(mut s.a)"][rng.random_range(0..4)].S(),
            // A way out ends the block: what follows it would be dead.
            11 => {
                let line = if rng.random_bool(0.5) { "ret none".S() } else { fmt!("ret some {t}") };
                out.push_str(&fmt!("{indent}{line}\n"));
                return;
            }
            12 => "let r = maybe(c)?".S(),
            13 if in_loop => {
                out.push_str(&fmt!("{indent}{}\n", ["break", "continue"][rng.random_range(0..2)]));
                return;
            }
            14 => ["var x: int", "let x = 5", "var y: int"][rng.random_range(0..3)].S(),
            13 => fmt!("set {t} = 2"),
            15..=17 => {
                out.push_str(&fmt!("{indent}if c\n"));
                init_block(rng, depth + 1, in_loop, out);
                if rng.random_bool(0.6) {
                    out.push_str(&fmt!("{indent}else\n"));
                    init_block(rng, depth + 1, in_loop, out);
                }
                out.push_str(&fmt!("{indent}end if\n"));
                continue;
            }
            18 => {
                out.push_str(&fmt!("{indent}if maybe(c) |v|\n"));
                init_block(rng, depth + 1, in_loop, out);
                out.push_str(&fmt!("{indent}else\n"));
                init_block(rng, depth + 1, in_loop, out);
                out.push_str(&fmt!("{indent}end if\n"));
                continue;
            }
            19 => {
                out.push_str(&fmt!("{indent}loop while c\n"));
                init_block(rng, depth + 1, true, out);
                out.push_str(&fmt!("{indent}end loop\n"));
                continue;
            }
            20 => {
                out.push_str(&fmt!("{indent}loop\n"));
                init_block(rng, depth + 1, true, out);
                out.push_str(&fmt!("{indent}  if c\n{indent}    break\n{indent}  end if\n{indent}end loop\n"));
                continue;
            }
            21 => {
                out.push_str(&fmt!("{indent}for w in ref l\n"));
                init_block(rng, depth + 1, true, out);
                out.push_str(&fmt!("{indent}end for\n"));
                continue;
            }
            _ => {
                out.push_str(&fmt!("{indent}match e\n{indent}case atom A\n"));
                init_block(rng, depth + 1, in_loop, out);
                if rng.random_bool(0.5) {
                    out.push_str(&fmt!("{indent}case atom B\n"));
                } else {
                    out.push_str(&fmt!("{indent}case default\n"));
                }
                init_block(rng, depth + 1, in_loop, out);
                out.push_str(&fmt!("{indent}end match\n"));
                continue;
            }
        };
        out.push_str(&fmt!("{indent}{line}\n"));
    }
}

/// A script for `init_agrees`: `INIT_PRELUDE`, then a function with an
/// `out` parameter and variables declared with no value, which nothing
/// calls, so that only the analysis runs.
fn init_script(rng: &mut SeededRng) -> String {
    let mut body = String::new();
    body.push_str("  var x: int\n");
    body.push_str(if rng.random_bool(0.5) { "  var y: int\n" } else { "  var y: int = 0\n" });
    body.push_str(if rng.random_bool(0.5) { "  var s: {a: int}\n" } else { "  var s: {a: int} = {a = 0}\n" });
    init_block(rng, 0, false, &mut body);
    body.push_str("  ret some 0\n");
    fmt!("{INIT_PRELUDE}fun f(out p: int, c: bool, o: ?int, e: E, l: [int]): ?int\n{body}end fun\n")
}

/// The ownership codes the CLI reports for a script, in order.
fn rust_ownership_codes(text: &str) -> Vec<String> {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().X().parent().X();
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    let binary = workspace.join("target").join(profile).join("datalove");
    assert!(binary.exists(), "{} not built; `just test-formal` builds it", binary.display());
    let dir = rmx::tempfile::tempdir().X();
    let path = dir.path().join("script.dl");
    std::fs::write(&path, text).X();
    let mut cmd = std::process::Command::new(&binary);
    cmd.arg("script").arg(&path);
    let (_, lines) = run_bounded(cmd, std::time::Duration::from_secs(5));
    assert!(!lines.iter().any(|l| l.contains("panicked at")), "the Rust panics on:\n{text}\n{}", lines.join("\n"));
    let code = rmx::regex::Regex::new(r"^\[(O[0-9]+)\] Error:").X();
    lines.iter().filter_map(|l| code.captures(l).map(|c| c[1].S())).collect()
}

/// O005, O006 and O009 as the model decides them (`formal/Datalove/FunInit.lean`)
/// held to the Rust's, on functions that give, read and pass on variables
/// with no value and an `out` parameter: the model refuses a function where,
/// and only where, the CLI reports one of the three. A script the checker
/// refuses, or the Rust refuses with another code, is counted and skipped.
#[test]
fn init_agrees() {
    if std::env::var("FORMAL_ORACLE_TEST").is_err() {
        println!("Skipping the init tests (set FORMAL_ORACLE_TEST=1 to run)");
        return;
    }
    let seeds = bct::random::env_seeds("FORMAL_FUN", 0, 300);
    let mut cases = Vec::new();
    for seed in seeds.iter() {
        let mut rng = SeededRng::derive(seed, "init");
        let text = init_script(&mut rng);
        let (codes, script, _) = rust_side(&text);
        cases.push((fmt!("seed {seed}"), text, script, codes));
    }
    let queries: Vec<Value> = cases.iter().map(|(_, _, script, _)| json!({ "init": script })).collect();
    let answers = ask_oracle(&queries);
    const INIT_CODES: [&str; 3] = ["O005", "O006", "O009"];

    let mut outcomes = BTreeMap::<String, usize>::new();
    let mut mismatches = Vec::new();
    for ((what, text, _, rust_types), oracle) in cases.iter().zip(&answers) {
        if let Some(why) = oracle.get("unsupported") {
            *outcomes.entry(fmt!("unsupported: {why}")).or_default() += 1;
            continue;
        }
        let codes = oracle["codes"].as_array().X();
        if !codes.is_empty() || !rust_types.is_empty() {
            assert_eq!(codes.len(), rust_types.len(), "the checkers differ:\n{text}");
            *outcomes.entry(fmt!("checker refuses {}", codes[0].as_str().X())).or_default() += 1;
            continue;
        }
        let model = oracle["init"].as_str().map(|c| c.S());
        let rust = rust_ownership_codes(text);
        if let Some(other) = rust.iter().find(|c| !INIT_CODES.contains(&c.as_str())) {
            *outcomes.entry(fmt!("rust refuses {other}")).or_default() += 1;
            continue;
        }
        *outcomes.entry(match &model {
            Some(c) => c.C(),
            None => "accepted".S(),
        }).or_default() += 1;
        if model.is_some() != !rust.is_empty() {
            mismatches.push(fmt!("{what}:\n{text}    rust: {rust:?}, model: {model:?}"));
        }
    }
    println!(
        "{} functions over seeds {}..{}: {} differ; {outcomes:?}",
        cases.len(), seeds.start, seeds.start.wrapping_add(seeds.count), mismatches.len(),
    );
    for m in mismatches.iter().take(10) {
        println!("{m}");
    }
    assert!(mismatches.is_empty(), "the Rust and the model decide what holds a value differently on {} functions", mismatches.len());
}
