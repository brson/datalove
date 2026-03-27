# Unicode Sigil Proposal

Successor to `unicode-sigils-thought-experiment.md`.
Based on Scheme A from that document,
with revised map and set brackets.

ASCII fallbacks are deferred to a future proposal.
This document defines the unicode-canonical forms only.


## Design Principles

1. Every bracket pair has a distinct, matched open/close character.
2. Map and set brackets are from the same Unicode block (Misc. Math Symbols-B).
3. Each sigil has a single clear meaning.
4. `<` and `>` are restored to their natural role as comparison operators.


## Bracket Pairs

Nine distinct matched pairs.
The first three are unchanged;
the last four replace multi-character ASCII hacks.

| Purpose | Brackets | Unicode Name | Codepoints |
|---------|----------|--------------|------------|
| Tuple / grouping | `( )` | Parenthesis | U+0028, U+0029 |
| Struct / enum body | `{ }` | Curly bracket | U+007B, U+007D |
| List | `[ ]` | Square bracket | U+005B, U+005D |
| **Map** | `⦇ ⦈` | Z notation image bracket | U+2987, U+2988 |
| **Set** | `⦃ ⦄` | White curly bracket | U+2983, U+2984 |
| **Table** | `⟦ ⟧` | Double square bracket | U+27E6, U+27E7 |
| **Tensor** | `⟪ ⟫` | Double angle bracket | U+27EA, U+27EB |

Map `⦇ ⦈` and set `⦃ ⦄` are both from the
Misc. Math Symbols-B Unicode block (U+2980-U+29FF).
The image brackets `⦇ ⦈` originate in Z notation
where they denote relational image -- applying a mapping to a set --
making their heritage a natural fit for map semantics.


### Removed

| Old | Was | Replacement |
|-----|-----|-------------|
| `%{` `}` | Map open/close (asymmetric) | `⦇ ⦈` |
| `#{` `}` | Set open/close (asymmetric) | `⦃ ⦄` |
| `{|` `|}` | Table open/close (earmuff) | `⟦ ⟧` |
| `[|` `|]` | Tensor open/close (earmuff) | `⟪ ⟫` |
| `<` `>` | Angle bracket pair | No longer brackets; now comparison operators |


### Freed

| Brackets | Notes |
|----------|-------|
| `⟨ ⟩` | Math angle bracket (U+27E8/U+27E9). Available for future use (generics, type params). |
| `< >` | Now comparison operators, no longer a bracket pair. |


## Map Association: `↦`

Maps use `↦` (U+21A6, rightwards arrow from bar)
to separate keys from values.
This replaces the overloaded `=`
which also serves as struct field binding and `let`/`set` assignment.

```
⦇ "name" ↦ "Alice", "age" ↦ 30 ⦈
```

Structs keep `=`:

```
{ name = "Alice", age = 30 }
```

The visual distinction between `=` (naming a field)
and `↦` (associating a key) matches the semantic distinction:
structs have fixed known fields, maps have dynamic keys.


### Map Type Syntax

```
⦇K ↦ V⦈
```


## Comparison Operators

`<` and `>` are restored to their natural meaning.
The remaining comparisons get standard mathematical symbols.

| Old | New | Unicode Name | Codepoint |
|-----|-----|--------------|-----------|
| `.<` | `<` | Less-than sign | U+003C |
| `.>` | `>` | Greater-than sign | U+003E |
| `<=` | `≤` | Less-than or equal to | U+2264 |
| `>=` | `≥` | Greater-than or equal to | U+2265 |
| `==` | `≡` | Identical to | U+2261 |
| `!=` | `≢` | Not identical to | U+2262 |


## Unchanged Sigils

These are good design and stay as-is.

| Sigil | Purpose |
|-------|---------|
| `?` | Option type prefix, try-unwrap postfix |
| `!` | Result type prefix, try-unwrap postfix |
| `@` | Adapt (clone / widen / coerce) |
| `:` | Type annotation |
| `/` | Type hint separator, division, path separator |
| `.` | Field access, decimal point |
| `=` | Binding (`let`, `set`, struct fields) |
| `,` | Item separator |
| `;` | Statement separator |
| `\|x\|` | Binding pipes in if/match |
| `+! -! *! /!` | Checked-result arithmetic |
| `+? -? *? /?` | Checked-option arithmetic |


## Available Characters

After this proposal, these ASCII characters remain unassigned:

```
# % $ & ~ ^ ` \ < > |
```

`#` and `%` are freed from their map/set prefix roles.
`<` and `>` are comparison operators, not brackets.
`|` is used only inside binding pipes `|x|`, not as a standalone sigil.


## Type Syntax Summary

| Type | Syntax |
|------|--------|
| List | `[T]` |
| Map | `⦇K ↦ V⦈` |
| Set | `⦃T⦄` |
| Table | `⟦ col: T ⟧` |
| Tensor | `⟪T, N⟫` |
| Tuple | `(T1, T2)` |
| Struct | `{ x: T1, y: T2 }` |
| Option | `?T` |
| Result | `!T` |


## Literal Syntax Summary

| Literal | Syntax |
|---------|--------|
| List | `[1, 2, 3]` |
| Map | `⦇ "a" ↦ 1, "b" ↦ 2 ⦈` |
| Set | `⦃ 1, 2, 3 ⦄` |
| Table | `⟦ x, y; 1, 2; 3, 4 ⟧` |
| Tensor | `⟪ 1 2 3, 4 5 6 ⟫` |
| Tuple | `(1, 2)` |
| Struct | `{ x = 1, y = 2 }` |


## Sample Code

```
type Book: {
  title: string,
  author: string,
  year: i32,
  rating: f32,
  genres: ⦃enum { atom Fiction, atom Dystopia, atom SciFi }⦄,
  subtitle: ?string,
  translations: ⦇string ↦ bool⦈,
}

fun reserve_book(mut db: [Book], ref title: string): !()
  var i: index = 0
  loop while i < db.len
    if db[i]!.title ≡ title
      if db[i]!.available
        set db[i]!.available = false
        ret ok ()
      else
        ret er error atom BookNotAvailable
      end if
    end if
    set i = i +! 1
  end loop
  ret er error atom BookNotFound
end fun
```

```
// Data literal showcase
let record = {
  name = "example",
  tags = ["fast", "typed"],
  scores = ⦇ "x" ↦ 1, "y" ↦ 2 ⦈,
  ids = ⦃ 10, 20 ⦄,
  matrix = ⟪ 1.0 0.0, 0.0 1.0 ⟫,
  metrics = ⟦
    name, value
    "latency", 0.5
    "throughput", 120.0
  ⟧,
  status = ok "healthy",
  backup = none,
  kind = (atom Normal)@,
}
```

```
fun clamp(val: u32, lo: u32, hi: u32): u32
  if val < lo
    ret lo
  else if val > hi
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

fun find_threshold(vals: ?⟪u32, 1⟫, limit: u32): ?u32
  let t = vals?
  let n: u32 = 42
  if n ≤ limit
    ret some n
  end if
  ret none
end fun
```


## Font Support

All proposed characters are in the
Mathematical Operators (U+2200-U+22FF),
Miscellaneous Mathematical Symbols-B (U+2980-U+29FF),
and Supplemental Mathematical Operators (U+2A00-U+2AFF) blocks.
Well-supported in monospace programming fonts:
JetBrains Mono, Fira Code, Iosevka, Cascadia Code.
