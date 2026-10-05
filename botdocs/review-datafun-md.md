# Review: mandocs/datafun.md

A review of the datafun primer for content and flow, refreshed 2026-10-05
against the draft as it then stood: sections written through Numerics, with
Collections and indexing and everything after Numerics stubbed. The intended
audience is experienced programmers, likely knowing Rust, who have already
read `datalit.md`.

An earlier version of this review (2026-10-02) asked for a first-script
section, bindings before functions, an Ownership section, a smaller first
function, and option/result handling taught once rather than split across
two sections. The draft has done all of these, and the order of topics now
matches what that review suggested. Those items are gone from here, as are
the mechanical fixes it applied. What remains is either still open or new.

Every `datalove` code block was run through `datalove script`. All run, with
two exceptions: the D001 example, which is meant to fail, and the
`case default` example, which continues the block before it and fails alone
with F001 (`s` not found). Saying so in the text, or repeating the binding,
would help readers who paste examples.

## Summary

The structure is now sound, and the first script, Variables, Functions and
Ownership read well in sequence. The largest remaining gap is that Ownership
never says which types are copied and which are moved, and `int` -- the
default integer type -- is moved. Option and result handling understates
what a result's `else` needs.

The draft's rule that destructuring `if`s take no part in `else if` chains was
not enforced when this review was first refreshed. The parser now enforces it
(P073), so the text is right as written.

## Errors

These say something the implementation does not do.

- **The result `if` needs an error binding, not just an `else`.** "In the
  result case the `else` branch is required" understates it: `else` without
  `|e|` is F046, "Result destructuring requires an else binding". Say that
  the `else` must bind the error.
- **The D001 transcript does not match the output.** The real header is
  `╭─[ test.dfs:7:9 ]`, with spaces inside the brackets. The help shows the
  fixed line without a line-number gutter (`let b = a@` under `Help:`), not as
  ` 6 │    let b = a@`. Regenerate the transcript before publishing, or say it is
  abridged.

## Missing topics

- **Which types copy and which move.** Ownership says types "that contain heap
  allocations" move. The reader cannot tell from that which types are which,
  and the answer surprises Rust readers: `int` moves (`let y = x;
  debuglog (x, y)` is D001), while `f64` and the fixed-width integers copy.
  List them: copied are `bool`, the fixed-width integers, the floats,
  `index` and `offset`, and aggregates of those. Moved are `int`, `string`,
  collections, `data` and `error`. Also say that arithmetic and comparison
  read their operands rather than consuming them. That is why the
  control-flow examples can write `counter - 1` and `counter == 0` freely,
  but need `counter@` to pass `counter` to `rem_checked`.
- **What `out` means.** The parameter-modes example shows `out` but never says
  that the callee must write it before returning, that the caller's binding
  may be an unassigned `var`, and that it is initialised afterwards.
- **Collections in use** (the stubbed section): fallible indexing (`a[i]?`,
  `m[key]!`), field access (`.0`, `.name` -- the `match` example uses
  `dims.0` unexplained), building and iterating. Indexing that cannot panic
  is a strong selling point for this audience.
- **Totality.** The intro says functions "have no exceptional control-flow".
  The stronger and more distinctive claim deserves a paragraph: no panics,
  and every partial operation returns an option or result.
- **`index` and `offset`** are in the primitives table but absent from
  Numerics, though they are what lists are indexed by and their width is a
  build setting.
- **`error` and `data`**: "As well as the dynamic types, `data` and `error`"
  is the whole treatment, and `er error "failed to load"` uses the `error`
  constructor unexplained. A sentence on making errors and what boxes into
  `data`.
- **Riders**: a paragraph under Modules or std, since an embeddable scripting
  language is the intro's pitch.
- **Strings**: the first script shows `to_uppercase` and `len`, and Ownership
  shows `push_str`. A short tour of everyday string operations would still
  help, since it is the most common moved type.

## Clarity, by section

**Intro.** "Reactive script units that may be chained together, and that
incrementally recompile and reevaluate as dependent units and modules are
updated" is heavy for a bullet. Plainer wording, or a forward reference to
Scripts. "For detail see additional documentation" should name or link it.

**Data types.**
- "Structural and linear" sits awkwardly with Ownership's copy types.
  Something like "structural, and linear unless trivially copyable" would
  avoid a contradiction three sections later.
- "As well as the dynamic types, `data` and `error`" is a sentence fragment.
- "They can be used to name the type but not construct it" is still unclear.
  If the point is that there are no nominal constructors and values are built
  structurally (`atom Circle`, not `Shape.Circle`), say that.
- The example prints `enum { atom Circle }`, not `atom Circle`: a value
  typed as an enum prints with its enum wrapper. That will surprise readers,
  so say it, or print something less surprising.

**Functions.**
- "(`( .. )`, `{ .. }`, `< .. >` and others)": nothing so far uses `< .. >`,
  so either drop it or say what uses it.
- The `min_value` example rebinds `min` from a `let` to a `var`. That is
  legal shadowing, but it reads like a mistake in an example about something
  else; use two names.

**Ownership.**
- "All values are uniquely owned" followed by copy types reads as a
  contradiction; "affine" or "owned, with cheap types copied" is closer.
- The parameter modes map onto Rust directly, and saying so would orient
  this audience in one line: by value moves, `ref` is `&`, `mut` is
  `&mut`, `@` is `.clone()` or a lossless `.into()`.
- "In some cases Datalove can optionally compile in an 'auto-adapt' mode":
  say how it is enabled, or forward-reference where.
- "Beyond cloning it also performs widening numeric conversions and more":
  forward-reference Numerics, which shows the widening, and name the "more".

**Control flow.** `continue` is named but never shown, and early `ret` from
inside a loop is not mentioned. The two examples are still close. The second
could become a `loop while` that uses `continue`.

**Data types and destructuring.**
- Point out that struct types use `:` and struct values use `=` (`{a: bool}`
  versus `{a = true}`). The table shows it, but it is the reverse of what a
  Rust reader expects and deserves a sentence.
- `let term Foo x = term Foo "bar"` is unexplained. It works only because the
  type has one variant, and a reader will wonder what happens otherwise.
- The first `match` example computes `area` and never logs it.
- "Match must be exhaustive; Use" has a capital after a semicolon.

**Option and result handling.**
- For Rust readers: `?` is for options and `!` for results, unlike Rust's
  single `?`. Tie this to the checked operators in Numerics, which
  early-return by the same mechanism.
- "The postfix `?` and `!` operators propagate option and result return
  types" is vague. They return `none` or the `er` from the enclosing
  function, and otherwise yield the payload.
- `add_twice(self: u32, ...)`: naming a parameter `self` suggests method
  syntax, which does not exist (`x.add_checked(2)` is F078). Use another name.
- Mention the helpers (`ok_or`, `unwrap_or`, `is_some`) as a family; `ok_or`
  appears with no introduction.
- `debuglog(add_twice(...))` and, in Numerics, `debuglog(v)` are spaced
  differently from `debuglog (a, b)` everywhere else.

**Numerics.**
- "Integer literals have the bigint `int` type by default" undersells it:
  literals take their type from context. `u8.add_wrapping(255, 1)` and
  `u8.from_u64(100)` need no hint. The draft shows the `let` annotation case
  but not the argument case. The `: T /` hint is for when there is no
  context.
- The hint's precedence: `: u32 / 3 + 4` hints only `3`.
- "Thus none of the bare math binops work on fixed-sized integers" needs a
  scope. Comparisons work, as `min_value` shows. `+` is F026, and so is unary
  `-` on `i32`, though the paragraph before lists unary negation among the
  supported operations. Say whether there is a checked negation.
- That checked operators return early from the *enclosing function* is the
  most surprising semantics in the document. Emphasise it, and contrast it
  with Rust's `checked_add`.
- Say what `/!` produces. Division by zero gives `er error "arithmetic
  overflow"`, which is an odd message for that case. It may be worth fixing
  in the compiler rather than documenting.
- Conversions: describe the naming conventions rather than one function:
  `from_X` (none when out of range), `from_X_wrapping`, `from_int` on every
  fixed type, `f32`/`f64.from_<fixed>`, `int.from_f64`, conversions between
  `index`/`offset` and the integers, and that `@` only widens. Likewise the
  `_checked`/`_wrapping`/`_saturating` arithmetic naming.
