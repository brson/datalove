# Review: mandocs/datafun.md

A review of the datafun primer for content and flow, as of 2026-10-06. The
intended audience is experienced programmers, likely knowing Rust, who have
already read `datalit.md`.

Every section is written except Scripts and interactive units, which is a
heading only. Every `datalove` code block was run through `datalove script`.
All run except the two meant to fail: the D001 example in Ownership and the
inference example in Generics (F016). Both are labelled as failing in the text.

## Summary

The structure is sound and the order of topics works. Collections and
indexing, Comparison and equality, Constants, and Generics are thorough and
accurate. What remains falls into three groups:

- Three gaps in the early sections that this audience will trip on. Ownership
  never says which types copy, and `int` moves. Numerics doesn't say literals
  take their type from context. Option and result handling understates the
  result `else`.
- Topics with no home yet: the Scripts section, strings, `error` and `data`,
  writing a module, and riders.
- Smaller clarity issues, most of them in the first half, which was written
  earliest.

## Errors

These say something the implementation does not do.

- **The D001 transcript does not match the output.** The real header is
  `╭─[ test.dfs:7:9 ]`, with spaces inside the brackets. The help shows the
  fixed line without a line-number gutter (`│           let b = a@`), not as
  ` 6 │    let b = a@`. Regenerate it, or say it is abridged.
- **The result `if` needs an error binding, not just an `else`.** "In the
  result case the destructuring `else` branch is required" understates it:
  `else` without `|e|` is F046, "Result destructuring requires an else
  binding".
- **Numerics says unary negation is a supported binop.** For fixed-width
  integers it is not: `-a` on an `i32` is F026. The checked forms `-?` and
  `-!` exist, and the example uses them, but the text should say so.

## Missing topics

- **Scripts and interactive units.** This is the only stub. The intro promises
  incremental recompilation and reevaluation, and Constants says a script
  returns a result. This section should cover what a script's top level is,
  the script's result, how `!` at top level stops it, and the interactive and
  reactive behaviour. Generics now points here for the REPL story.
- **Which types copy and which move.** Ownership says types "that contain heap
  allocations" move. The reader cannot tell from that which types are which,
  and the answer surprises Rust readers: `int` moves (`let y = x; debuglog
  (x, y)` is D001), while `f64` and the fixed-width integers copy. List them.
  Copied: `bool`, the fixed-width integers, the floats, `index` and `offset`,
  and aggregates of those. Moved: `int`, `string`, collections, `data` and
  `error`. Also say that arithmetic and comparison read their operands rather
  than consuming them. That explains why Control flow writes `counter - 1` and
  `counter == 0` freely but needs `counter@` for `rem_checked`. Generics and
  Constants both lean on this distinction.
- **What `out` means.** The parameter-modes example shows `out` but never says
  that the callee must write it before returning, that the caller's binding
  may be an unassigned `var`, and that it is initialised afterwards.
- **Totality.** The intro says functions "have no exceptional control-flow".
  Collections now makes the strongest case, "no indexing operation that
  panics", but only for indexing. Give it a paragraph near the start: no
  panics, and every partial operation returns an option or result, or returns
  early through `?` and `!`.
- **`error` and `data`.** "As well as the dynamic types, `data` and `error`" is
  the whole treatment. `er error "failed to load"` uses the `error`
  constructor unexplained, and Generics now says type parameters are carried
  as `data`. A short passage on making errors and what `data` holds.
- **Strings.** The most common moved type gets no section. The first script
  shows `to_uppercase` and `len`, and Ownership shows `push_str`. A short tour
  of everyday string operations would help.
- **Writing a module.** Modules describes the hierarchy and the workspace but
  never shows a module's contents. Say that a `.dfm` holds functions, types
  and consts. Say that every function is visible to requirers while consts are
  private, as Constants says, and show a `local/` module being required from
  a script.
- **Riders.** These are native modules, `require`d like any other. A paragraph
  under Modules, since an embeddable scripting language is the intro's pitch.
- **`index` and `offset` in Numerics.** They are in the primitives table, and
  Collections indexes with them, but Numerics opens with "`u8` .. `u64` and
  `i8` .. `i64`" and never mentions them or that their width is a build
  setting.

## Clarity, by section

**Intro.** "For detail see additional documentation" should name or link it.

**Data types.**
- "Structural and linear" sits awkwardly with Ownership's copy types.
  "Structural, and linear unless trivially copyable" avoids the contradiction.
- "As well as the dynamic types, `data` and `error`" is a sentence fragment.
- "They can be used to name the type but not construct it" is unclear. If the
  point is that there are no nominal constructors, so values are built
  structurally (`atom Circle`, not `Shape.Circle`), say that.
- The example prints `enum { atom Circle }`, not `atom Circle`. A value typed
  as an enum prints with its enum wrapper. Say so, or log something less
  surprising.

**Functions.**
- "`< .. >`" now has a use: forward-reference Generics.
- The `min_value` example rebinds `min` from a `let` to a `var`. That is legal
  shadowing, but it reads like a mistake in an example about something else.
  Use two names.

**Ownership.**
- "All values are uniquely owned" followed by copy types reads as a
  contradiction; "affine" or "owned, with cheap types copied" is closer.
- The parameter modes map onto Rust directly, and one line would orient this
  audience: by value moves, `ref` is `&`, `mut` is `&mut`, `@` is `.clone()`
  or a lossless `.into()`.
- "In some cases Datalove can optionally compile in an 'auto-adapt' mode": say
  how it is enabled, or cut it.
- "Beyond cloning it also performs widening numeric conversions and more":
  forward-reference Numerics, which shows the widening, and name the "more".

**Control flow.** `continue` is named but never shown, and an early `ret` from
inside a loop is not mentioned. The two examples are nearly the same program.
The second could become a `loop while` that uses `continue`.

**Data types and destructuring.**
- Point out that struct types use `:` and struct values use `=` (`{a: bool}`
  versus `{a = true}`). The table shows it, but it is the reverse of what a
  Rust reader expects.
- `let term Foo x = term Foo "bar"` is unexplained. It works only because the
  type has one variant, and a reader will wonder what happens otherwise.
- The first `match` example computes `area` and never logs it, so it prints
  nothing.

**Option and result handling.**
- For Rust readers: `?` is for options and `!` for results, unlike Rust's
  single `?`. Tie this to the checked operators and to indexing, which return
  early the same way. Collections already says "just like the checked
  arithmetic operators", but that comparison belongs here first.
- "The postfix `?` and `!` operators propagate option and result return types"
  is vague. They return `none` or the `er` from the enclosing function, and
  otherwise yield the payload.
- `add_twice(self: u32, ...)`: naming a parameter `self` suggests method
  syntax, which does not exist (`x.add_checked(2)` is F078). The same name is
  used in the Generics examples, so either explain the convention once or
  rename it.
- Introduce the helpers (`ok_or`, `unwrap_or`, `is_some`) as a family; `ok_or`
  appears with no introduction.
- `debuglog (add_twice(...))` wraps a single value in parentheses, unlike the
  rest of the document.

**Collections and indexing.**
- "bult-in" is a typo.
- The `loop while` walk uses `i +! 1` at script top level, where a failure
  would stop the script. That's fine, but it is the first time a checked
  operator appears outside a function, and Numerics has not yet been read.
  Either move Numerics earlier or say what `+!` does here.

**Numerics.**
- "Integer literals have the bigint `int` type by default" undersells it:
  literals take their type from context. `u8.add_wrapping(255, 1)` needs no
  hint, as the section's last example shows without comment, and Generics now
  relies on the same rule. The `: T /` hint is for when there is no context.
- The hint binds tightly: `: u32 / 3 + 4` hints only `3` and is a type error.
- "None of the bare math operations work on fixed-sized integers" needs a
  scope. Comparisons work, as `min_value` shows.
- That checked operators return early from the *enclosing function* is the
  most surprising semantics in the document. Emphasise it, and contrast it
  with Rust's `checked_add`, which returns an `Option` in place.
- Say what the `!` operators produce: `er error "arithmetic overflow"` for
  `+!`, `-!` and `*!`. For `/!` it is `er error "division by zero"`, or
  `"division by zero or overflow"` on signed types, where `MIN /! -1` also
  fails.
- Conversions: describe the naming conventions rather than one function.
  `from_X` returns none when out of range; there are also `from_X_wrapping`,
  `from_int` on every fixed type, `f32`/`f64.from_<fixed>`, `int.from_f64`,
  and conversions between `index`/`offset` and the integers. `@` only widens.
  Likewise the `_checked`/`_wrapping`/`_saturating` arithmetic naming.
- `debuglog (v)` has redundant parentheses.

**Comparison and equality.** Accurate and complete. The `largest` example
writes `with { T is ord, }` with a trailing comma, while Generics writes
`with { T is fixedint }` without one. Both parse; pick one.

**Modules, packages, libraries and the workspace.** See the missing-topic items
on writing a module and on riders. "This capability is not yet exposed through
any frontend" ends the section on something the reader cannot use; it could be
cut or moved to Scripts.

**Constants and compile-time evaluation.** Clear. One sentence in Const
parameters is hard to parse: "`const` arguments are passed as references
during compile-time evaluation so they are written without `@`". Saying that
a const argument is borrowed, like any named const, would be enough.

**Generics.** Accurate and complete. The `with` comma style is noted under
Comparison.
