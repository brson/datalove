# Review: mandocs/datafun.md

A review of the datafun primer for content and flow, written 2026-10-02
against the draft as it then stood (sections through Numerics written, the
rest stubbed). The intended audience is experienced programmers, likely
knowing Rust, who have already read `datalit.md`.

The mechanical fixes listed under "Applied" were made in the same change as
this file. Everything else is a suggestion for the author.

## Summary

The prose is clear and the drafted sections are in a reasonable order. The
largest structural gap is that ownership -- that `int` and `string` are
linear, that passing or binding moves them, and that `@` clones -- is never
introduced, while the examples already depend on it. Several examples also
use constructs before the document explains them.

## Applied

Every code block was run through `datalove script`. These were fixed:

- The multi-line function example wrote a struct literal with `:`
  (`count: count`); struct values use `=`.
- Both control-flow examples failed with D001 use-after-move, because
  `rem_checked(counter, 2)` consumes the linear `int` counter. Changed to
  `counter@`, the fix the compiler suggests. This now forward-references `@`
  until an ownership section exists.
- `require sys/std/u8` was missing `module`.
- `ret some (a +? b) /? c` and its siblings are a P065 parse error: `/?`
  cannot follow a constructor's payload. Parenthesised the payload.
- The `add_saturating` example defined the function but never called it.
- `:u32 / 1` spacing made consistent with `: u32 /` elsewhere.
- Typos: structuaral, paramaters, "must by", "are be performed", "types
  operations".
- Type aliases were said to be `SnakeCase`; `Shape` is `PascalCase`.
- TOC: the modules link lacked `user-content-`, and the std entry's text and
  anchor did not match its heading.

Two blocks still do not run alone, by design: the multi-line function example
elides its body, and the `case default` example continues the one before it.
Saying so in the text, or repeating the binding, would help readers who paste
examples.

## Order of topics

1. **Add a short "first script" section before Data types.** Every example is
   a script, and the reader meets these with no introduction: top-level
   statements run in order; `debuglog` is the only output; `require module`
   and `import` (the second example needs four imports); `//` comments.
   A sentence each, with forward references. State that there are no
   qualified calls -- `u8.from_u64(x)` does not parse, the function must be
   imported -- because it is a real gotcha: one script cannot use both
   `u8.from_int` and `i8.from_int`.

2. **Bindings before functions.** `let`, `var`, `set` and shadowing are
   introduced at the end of Functions, after the large example uses them.

3. **An Ownership section right after Functions, absorbing the planned `@`
   section.** This is the main missing topic:
   - Copy types (`bool`, fixed ints, floats, `index`, `offset`) versus linear
     ones (`int`, `string`, collections, `data`, `error`).
   - Moves and use-after-move. Even `let x = 3; let y = x; debuglog (x, y)`
     is an error, which surprises a Rust reader, for whom integers are `Copy`.
   - Parameter modes as borrowing, mapped to Rust: by value is a move, `ref`
     is `&`, `mut` is `&mut`, `@` is `.clone()` or a lossless `.into()`.
   - The compiler suggests `@` in its errors.

   `@` is already used in Numerics, so its planned slot after Comparison is
   too late. With this section in place the control-flow examples read
   naturally.

4. **A smaller first function.** `count_substrings` uses `ref`, `var`/`set`,
   `loop`, option-destructuring `if`, postfix `?` and four imports before any
   is explained. Open with a three-line function; move `count_substrings` to
   the end of Option/result as a "putting it together" example.

5. **No duplication between destructuring and option/result.** The `if |x|`
   forms are taught in "Data types and destructuring" and then referred back
   to ("As described previously"). Keep products, `let` patterns and `match`
   in the destructuring section; move all option/result handling to its own.

6. **Suggested overall order:** intro, first script, data types recap,
   bindings, functions, ownership and `@`, control flow, destructuring and
   `match`, option and result, collections and indexing, numerics, comparison
   and equality, modules, constants and compile-time evaluation, generics,
   the standard library, scripts and interactive units, workspaces.

## Missing topics

- **Ownership**, above.
- **Collections in use**: fallible indexing (`a[i]?`, `m[key]!`), field
  access (`.0`, `.name` -- the match example uses `dims.0` unexplained),
  building and iterating. Already noted in the draft's Todo. Indexing that
  cannot panic is a strong selling point for this audience.
- **Generics**: generic functions and bounds (`with { T is fixedint, }`) are
  absent from the planned outline, though std uses them throughout and Rust
  readers will look for them.
- **What "(nearly) total" means**: no panics, no exceptions, every partial
  operation returns an option or result; the remaining exception is
  non-termination. One of the most distinctive claims, currently four words.
- **Statements, not expressions**: Rust readers will expect `let x = if ...`
  or a `match` that yields a value. Say once that `if`, `match` and `loop`
  are statements, which is why the match example assigns to a `var`.
- **`index` and `offset`**: listed among the primitives but absent from
  Numerics, though they are what lists are indexed by and their width is a
  build setting.
- **`error` and `data`**: `er error "failed to load"` appears unexplained.
  A sentence on making errors and on what boxes into `data`.
- **Riders**: a paragraph under Modules or std, since an embeddable
  scripting language is the intro's pitch.
- **Strings**: the most common linear type; a short example of everyday
  operations.

## Clarity, by section

**Intro.** "Reactive script units that may be chained together, and that
incrementally recompile and reevaluate as dependent units and modules are
updated" is heavy for the fourth bullet. Plainer wording, or a forward
reference to Scripts.

**Data types.** "They can be used to name the type but not construct it" is
unclear -- is the point that there are no nominal constructors and values are
built structurally? The escaped pipes in `{\| col1 ... \|}` will show their
backslashes in a code span outside a table under GitHub-flavoured markdown;
check the rendering.

**Functions.** Explain `out` parameters: is `var c: int` with no initialiser
how they are declared, and must the callee assign them? Also: `ret` is
required, how a unit-returning function is written, no implicit return.

**Control flow.** The two examples are near duplicates. Keep the `loop while`
one and add a short `loop` with `break` and `continue`. Mention early `ret`.

**Destructuring.** Point out that struct types use `:` and struct values
use `=` (`{a: bool}` versus `{a = true}`) -- the reverse of what a Rust
reader expects, and the draft's own example got it wrong. Say why `else if`
chains are disallowed after an option/result `if`, and whether the result
form's `else` must bind `|e|`.

**Option and result.** For Rust readers: `?` is for options and `!` for
results, unlike Rust's single `?`. Tie this to the checked operators and
fallible indexing, which early-return by the same mechanism. Mention the
helpers (`unwrap_or`, `ok_or`, `is_some`) as a family.

**Numerics.**
- "Integer literals have the bigint `int` type by default" undersells it:
  literals take their type from context (`let zero: u32 = 0`,
  `from_u64(100)` and `add_wrapping(255, 1)` need no hint), defaulting to
  `int`. The `: T /` hint is for when there is no context.
- The hint's precedence: `: u32 / 3 + 4` hints only `3` and is a type error.
- "None of the bare math binops work on fixed-sized integers" needs a scope:
  comparisons, bitwise operations, unary minus?
- That checked operators return early from the *enclosing function* is the
  most surprising semantics in the document. Emphasise it, contrast it with
  Rust's `checked_add`, and say what `/!` produces
  (`er error "arithmetic overflow"`).
- Conversions: describe the naming convention rather than one function:
  `from_X` (none when out of range), `from_X_wrapping`, `from_int` on every
  fixed type, `f32`/`f64.from_<fixed>`, `int.from_f64`, conversions between
  `index`/`offset` and the integers, and that `@` only widens. Likewise the
  `_checked`/`_wrapping`/`_saturating` arithmetic naming.
