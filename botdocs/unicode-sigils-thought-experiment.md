# Unicode Sigils Thought Experiment

Datalove is sigil-heavy by design:
its structural types, collection literals, checked arithmetic,
and error-handling all lean on ASCII punctuation combinations.
This works, but multi-character sigils like `%{`, `#{`, `{|`, `[|`, `.<`, `.>`, `+!`, `+?`
add visual noise and token complexity.

In a future where AI is the primary code author/reader,
we can trade ASCII familiarity for Unicode precision --
one glyph per concept, every bracket pair visually distinct.

This document proposes three coherent replacement schemes.


## Current Sigil Inventory

### Brackets

| Use     | Open/Close | Notes                           |
|---------|------------|---------------------------------|
| Tuple   | `( )`      | Also grouping, function params  |
| Struct  | `{ }`      | Also enum bodies                |
| List    | `[ ]`      | Ordered sequence                |
| Generic | `< >`      | Matched braces, type params     |
| Map     | `%{ }`     | Sigil-brace, closes with `}`   |
| Set     | `#{ }`     | Sigil-brace, closes with `}`   |
| Table   | `{| |}`    | Earmuff brace (2-char open/close) |
| Tensor  | `[| |]`    | Earmuff brace (2-char open/close) |
| Binding | `\|x\|`    | If/else destructuring           |

### Sigil-Logic (single-char, fixed meaning)

| Sigil | Meaning             |
|-------|---------------------|
| `:`   | Type annotation     |
| `?`   | Option              |
| `!`   | Result / error      |
| `@`   | Adapt (clone/widen) |
| `;`   | Statement break     |
| `.`   | Field projection    |
| `=`   | Value binding       |
| `/`   | Division + type hint separator |

### Multi-Character Operators

| Operator           | Meaning                    |
|--------------------|----------------------------|
| `.<` `.>`          | Less/greater than          |
| `<=` `>=`          | Less/greater or equal      |
| `==` `!=`          | Equal / not equal          |
| `+! -! *! /!`      | Checked-result arithmetic  |
| `+? -? *? /?`      | Checked-option arithmetic  |
| `-!` `-?`          | Unary negation (checked)   |

### Reserved

`$` and `~` are lexed but not yet assigned meaning.
`(| |)` and `<| |>` are reserved earmuff bracket pairs.


---


## Scheme A: Bracket Reform

**Philosophy**: Every collection type gets its own Unicode bracket pair.
Move generics from `< >` to `⟨ ⟩`, freeing `<` `>` for comparison.
Operators get targeted fixes where ASCII causes conflicts.

### Bracket Changes

| Current    | New    | Unicode Name              | Codepoints     |
|------------|--------|---------------------------|----------------|
| `< >`      | `⟨ ⟩`  | Math angle bracket        | U+27E8, U+27E9 |
| `%{ }`     | `⟪ ⟫`  | Math double angle bracket | U+27EA, U+27EB |
| `#{ }`     | `⦃ ⦄`  | White curly bracket       | U+2983, U+2984 |
| `{| |}`    | `⟬ ⟭`  | White tortoise shell      | U+27EC, U+27ED |
| `[| |]`    | `⟦ ⟧`  | Math double square bracket | U+27E6, U+27E7 |

The full bracket set becomes 9 visually distinct pairs:

```
( )   tuples, grouping, function params
{ }   structs, enum bodies
[ ]   lists
⟨ ⟩   generics / type parameters   (was < >)
⟪ ⟫   maps                         (was %{ })
⦃ ⦄   sets                         (was #{ })
⟬ ⟭   tables                       (was {| |})
⟦ ⟧   tensors                      (was [| |])
```

Design rationale:
- `⟨ ⟩` for generics: the lightest angle bracket, same role
- `⟪ ⟫` for maps: doubled angles suggest key-value pairing
- `⦃ ⦄` for sets: curly-like (mathematical set notation uses `{ }`)
- `⟬ ⟭` for tables: wide/rounded shape evokes tabular rows
- `⟦ ⟧` for tensors: doubled squares = "dense array", common in math

### Operator Changes

| Current | New | Why                                    |
|---------|-----|----------------------------------------|
| `.<`    | `<` | `< >` freed by moving generics to `⟨ ⟩` |
| `.>`    | `>` | Same                                   |
| `<=`    | `≤` | Standard mathematical (U+2264)         |
| `>=`    | `≥` | Standard mathematical (U+2265)         |
| `!=`    | `≠` | Frees `!` to be purely result-oriented (U+2260) |
| `==`    | `==` | Keep (already unambiguous)             |

Checked and option arithmetic stay as `+! -! *! /!` and `+? -? *? /?`.
Their two-char form directly encodes sigil-logic: the operator + handling mode.

### Sample Code

```
// -- Types --
type Point: { x: f32, y: f32 }
type Color: enum { atom Red, atom Blue, term Custom string }

// -- Collection types --
let xs: [i32] = [1, 2, 3]
let scores: ⟪string = int⟫ = ⟪"a" = 1, "b" = 2⟫
let ids: ⦃int⦄ = ⦃10, 20, 30⦄
let matrix: ⟦f64, 2⟧ = ⟦1.0 0.0, 0.0 1.0⟧
let data: ⟬x: int, y: int⟭ = ⟬
  x, y
  1, 2
  3, 4
⟭

// -- Generics --
fun unwrap_or⟨T⟩(self: ?T, default: T): T where {
  T is move,
}
  if self |value|
    ret value
  else
    ret default
  end if
end fun

// -- Comparison (natural < > freed) --
fun clamp(val: u32, lo: u32, hi: u32): u32
  if val < lo
    ret lo
  else if val > hi
    ret hi
  end if
  ret val
end fun

// -- Checked arithmetic + option/result --
fun safe_add(a: u32, b: u32): !u32
  ret ok (a +! b)
end fun

fun ratio(a: u32, b: u32): ?u32
  let sum = a +? b
  ret some (sum /? 2)
end fun

// -- Adapt, try, comparison --
fun process(vals: ?⟦u32, 1⟧): !int
  let t = vals?
  let n: int = 42
  if n ≤ 100
    ret ok n
  end if
  ret error "too large"
end fun

// -- Data literal showcase --
let record: {
  name: string,
  tags: [string],
  scores: ⟪string = int⟫,
  ids: ⦃int⦄,
  matrix: ⟦f64, 2⟧,
  metrics: ⟬name: string, value: f64⟭,
  status: !string,
  backup: ?string,
  kind: enum { atom Normal, term Custom string },
} = {
  name = "example",
  tags = ["fast", "typed"],
  scores = ⟪"x" = 1, "y" = 2⟫,
  ids = ⦃10, 20⦄,
  matrix = ⟦1.0 0.0, 0.0 1.0⟧,
  metrics = ⟬name, value; "latency", 0.5⟭,
  status = ok "healthy",
  backup = none,
  kind = (atom Normal)@,
}
```


---


## Scheme B: Operator Clarity

**Philosophy**: Keep brackets close to current ASCII style --
the earmuff and sigil-brace syntax is learnable by humans.
Instead, focus on replacing the operators that cause
visual confusion or semantic overlap.
Keep `< >` for brace matching; use symbols to fix comparison.

### Bracket Changes (minimal)

| Current  | New    | Why                                  |
|----------|--------|--------------------------------------|
| `{| |}` | `⟬ ⟭`  | Most visually cluttered earmuff      |
| `[| |]` | `⟦ ⟧`  | Second most cluttered                |

Keep `%{ }`, `#{ }`, `< >` as-is.

### Operator Changes

| Current      | New  | Unicode Name       | Codepoint |
|--------------|------|--------------------|-----------|
| `.<`         | `≺`  | Precedes           | U+227A    |
| `.>`         | `≻`  | Succeeds           | U+227B    |
| `<=`         | `≤`  | Less-or-equal      | U+2264    |
| `>=`         | `≥`  | Greater-or-equal   | U+2265    |
| `!=`         | `≠`  | Not equal          | U+2260    |
| `==`         | `≡`  | Identical to       | U+2261    |

Checked and option arithmetic stay as `+! -! *! /!` and `+? -? *? /?`.

### Updated Sigil-Logic Table

| When you see | It means        |
|--------------|-----------------|
| `:`          | type            |
| `?`          | option          |
| `!`          | result          |
| `@`          | adapt           |
| `≺` `≻`     | ordered compare |
| `≤` `≥`     | ordered compare |
| `≡` `≠`     | equality        |

No sigil serves double duty. `!` is purely result/error.
`≺`/`≻` are unambiguous -- no dot-prefix trick needed.

### Sample Code

```
type Color: enum { atom Red, atom Blue, term Custom string }

let scores: %{string = int} = %{"a" = 1, "b" = 2}
let ids: #{int} = #{10, 20, 30}
let matrix: ⟦f64, 2⟧ = ⟦1.0 0.0, 0.0 1.0⟧
let data: ⟬x: int, y: int⟭ = ⟬
  x, y
  1, 2
  3, 4
⟭

fun clamp(val: u32, lo: u32, hi: u32): u32
  if val ≺ lo
    ret lo
  else if val ≻ hi
    ret hi
  end if
  ret val
end fun

fun safe_divide(a: u32, b: u32): !u32
  if b ≡ 0
    ret error "divide by zero"
  end if
  ret ok (a /! b)
end fun

fun find_threshold(vals: ?⟦u32, 1⟧, limit: u32): ?u32
  let t = vals?
  let n: u32 = 42
  if n ≤ limit
    ret some n
  end if
  ret none
end fun
```


---


## Scheme C: Full Unicode

**Philosophy**: Combine Scheme A's bracket reform
with Scheme B's operator cleanup,
plus replace the type-hint separator and binding pipes.
Maximum single-glyph-per-concept density.

### All Changes

**Brackets** (same as Scheme A):

| Current    | New    | Use                  |
|------------|--------|----------------------|
| `< >`      | `⟨ ⟩`  | Generics             |
| `%{ }`     | `⟪ ⟫`  | Maps                 |
| `#{ }`     | `⦃ ⦄`  | Sets                 |
| `{| |}`    | `⟬ ⟭`  | Tables               |
| `[| |]`    | `⟦ ⟧`  | Tensors              |

**Comparison operators** (same as Scheme A,
since `< >` are freed):

| Current | New |
|---------|-----|
| `.<`    | `<` |
| `.>`    | `>` |
| `<=`    | `≤` |
| `>=`    | `≥` |
| `!=`    | `≠` |
| `==`    | `==` |

**Checked arithmetic** -- replace two-char combos
with circled and squared math operators:

| Current | New | Unicode Name    | Codepoint |
|---------|-----|-----------------|-----------|
| `+!`    | `⊕`  | Circled plus    | U+2295    |
| `-!`    | `⊖`  | Circled minus   | U+2296    |
| `*!`    | `⊗`  | Circled times   | U+2297    |
| `/!`    | `⊘`  | Circled divide  | U+2298    |
| `+?`    | `⊞`  | Squared plus    | U+229E    |
| `-?`    | `⊟`  | Squared minus   | U+229F    |
| `*?`    | `⊠`  | Squared times   | U+22A0    |
| `/?`    | `⧄`  | Squared divide  | U+29C4    |

Circled = result/error channel. Squared = option channel.

**Type-hint separator**:

| Current       | New            | Notes                   |
|---------------|----------------|-------------------------|
| `: T / expr`  | `: T -> expr`  | Arrow, reads as "gives" |

The `/` overloads division; `->` is dedicated to type hints.
(Alternative: `=>` or Unicode `|>`, but `->` is most readable.)

**Binding pipes**:

| Current    | New      | Unicode Name        | Codepoint       |
|------------|----------|---------------------|-----------------|
| `\|x\|`   | `[x]`    | Reuse brackets      | n/a             |

Or for maximum disambiguation:

| Current    | New      | Unicode Name        | Codepoint       |
|------------|----------|---------------------|-----------------|
| `\|x\|`   | `<x>`    | Angle brackets      | n/a (freed)     |

Since `< >` are freed from brace matching, they could serve as
binding delimiters in `if`/`else`. This reads naturally:
`if opt <value>` -- "if opt, binding value".

### Updated Sigil-Logic Table

| When you see | It means              |
|--------------|-----------------------|
| `:`          | type annotation       |
| `?`          | option (postfix try)  |
| `!`          | result (postfix try)  |
| `@`          | adapt                 |
| `⊕⊖⊗⊘`      | result arithmetic     |
| `⊞⊟⊠⧄`      | option arithmetic     |
| `<` `>` `≤` `≥` | ordered comparison |
| `≠`          | not-equal             |
| `->`         | type hint separator   |

### Sample Code

```
type Point: { x: f32, y: f32 }
type Color: enum { atom Red, atom Blue, term Custom string }

// Collection literals
let xs: [i32] = [1, 2, 3]
let scores: ⟪string = int⟫ = ⟪"a" = 1, "b" = 2⟫
let ids: ⦃int⦄ = ⦃10, 20, 30⦄
let matrix: ⟦f64, 2⟧ = ⟦1.0 0.0, 0.0 1.0⟧
let data: ⟬x: int, y: int⟭ = ⟬
  x, y
  1, 2
  3, 4
⟭

// Type hints with ->
let n = : u32 -> 42
let typed_list = : [i32] -> [1, 2, 3]

// Generics with angle brackets
fun unwrap_or⟨T⟩(self: ?T, default: T): T where {
  T is move,
}
  if self <value>
    ret value
  else
    ret default
  end if
end fun

// Natural comparison
fun clamp(val: u32, lo: u32, hi: u32): u32
  if val < lo
    ret lo
  else if val > hi
    ret hi
  end if
  ret val
end fun

// Circled operators for result arithmetic
fun safe_add(a: u32, b: u32): !u32
  ret ok (a ⊕ b)
end fun

fun safe_ratio(a: u32, b: u32): !u32
  let sum = a ⊕ b
  ret ok (sum ⊘ 2)
end fun

// Squared operators for option arithmetic
fun maybe_add(a: u32, b: u32): ?u32
  let sum = a ⊞ b
  ret some (sum ⧄ 2)
end fun

// Mixed: adapt, try, comparison
fun process(vals: ?⟦u32, 1⟧, limit: int): !int
  let t = vals?
  let n: int = 42
  if n ≤ limit
    ret ok n@
  end if
  ret error "too large"
end fun

// If-binding with freed angle brackets
let opt: ?u32 = some 42
if opt <value>
  debuglog value
end if

let res: !u32 = ok 99
if res <value>
  debuglog value
else <e>
  debuglog e
end if

// Big data literal
let record = {
  name = "example",
  tags = ["fast", "typed"],
  scores = ⟪"x" = 1, "y" = 2⟫,
  ids = ⦃10, 20⦄,
  matrix = ⟦1.0 0.0, 0.0 1.0⟧,
  metrics = ⟬name, value; "latency", 0.5⟭,
  status = ok "healthy",
  backup = none,
  kind = (atom Normal)@,
  payload = data : u32 -> 42,
}
```


---


## Comparison

| Feature               | Current | A: Brackets | B: Operators | C: Full  |
|-----------------------|---------|-------------|--------------|----------|
| Distinct bracket pairs | 4+4ish | 9           | 6            | 9        |
| Single-char map open  | no      | yes         | no           | yes      |
| Single-char set open  | no      | yes         | no           | yes      |
| Single-char table brackets | no | yes         | yes          | yes      |
| Single-char tensor brackets | no | yes        | yes          | yes      |
| Natural `<` `>` comparison | no | yes         | no           | yes      |
| `!` purely result     | no (`!=`) | yes       | yes          | yes      |
| Checked arith readable | yes    | yes         | yes          | medium   |
| Human-typeable        | yes     | medium      | medium       | hard     |
| AI-parseable          | yes     | better      | better       | best     |

### Tradeoffs

**Scheme A** is the sweet spot for this thought experiment.
The bracket reform is the biggest single win:
9 visually distinct bracket pairs, each unambiguous,
and it unlocks natural `<` `>` comparison as a bonus.
Checked arithmetic stays readable with its ASCII sigil-logic.

**Scheme B** is the most conservative.
Good for a world where humans still type some code,
but AI reads/writes most of it.
The operator replacements (`≺ ≻ ≤ ≥ ≠ ≡`)
are individually each an improvement.

**Scheme C** pushes furthest.
Circled/squared arithmetic operators are elegant in principle
but sacrifice the transparent `op + mode` encoding --
you have to memorize that `⊕` means "add, error on overflow"
rather than reading `+!` as "plus, with error propagation".
The `->` type hint and `<value>` binding are nice refinements.

### Font Considerations

All proposed Unicode characters are in the
Mathematical Operators (U+2200-U+22FF),
Miscellaneous Mathematical Symbols-A/B (U+27C0-U+27EF, U+2980-U+29FF),
and Supplemental Mathematical Operators (U+2A00-U+2AFF) blocks.
These are well-supported in monospace programming fonts:
JetBrains Mono, Fira Code, Iosevic, Cascadia Code.
