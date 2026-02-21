# Place Expressions and Ephemeral References

Design notes for indexing, field access, and the compiler-internal
place/reference model.

## Place Expressions

Expressions in datalove are either **place expressions** (denoting a
storage location) or **value expressions** (producing a fresh owned value).

Place expressions:
- Variables: `x`
- Index: `a[i]`, `m[key]`
- Field access: `a.field`
- Tuple element: `a.0`
- Column projection: `table.col`
- Chains: `a[i].field`, `a.b[0]`

Value expressions:
- Literals, function calls, arithmetic results, constructors, `@` clones

Place expressions produce **ephemeral references** -- not first-class types,
but a compiler-internal concept. The reference mode is determined entirely
by the destination context.

## Destination Contexts

| Context | What the compiler emits | Example |
|---------|------------------------|---------|
| `ref` param | Immutable borrow | `foo(ref a[i]?)` |
| `mut` param | Mutable borrow; root must be `var` | `foo(mut a[i]?)` |
| binop operand | Immutable borrow (sec 6.6) | `a[i]? + 1` |
| `set` LHS | Mutation target; root must be `var` | `set a[i]? = 5` |
| consume (`in`, `let`, `ret`) | Copy for copy types; requires `@` for linear types | `let x = a[i]?` |

`out` params are not valid destinations for place expressions.
`out` is for fresh uninitialized bindings only -- index places
refer to already-initialized slots.

## Consume Context and Linear Types

A place expression in consume context can't move the element out of
its container (that would leave a hole). The rules mirror
`ref`/`mut` params (D003: CannotMoveBorrowed):

- Copy types: copy the value out. Free.
- Linear types: error. Use `@` to clone explicitly.

This is consistent with the rest of the language -- moves are always
visible, clones require `@`.

## Fallible Indexing

The `[]` operator is fallible: out of bounds, missing key, etc.
The result is a reference, not a value, and neither optional
references nor result references are first-class types.

### The `fallible_place<T>` Model

`[]` produces an ephemeral `fallible_place<T>` -- a compiler-internal
concept representing "this lookup might fail." The `?` or `!` postfix
operator resolves the failure strategy:

```
a[i]       // produces fallible_place<T> -- must be resolved
a[i]?      // option path: unwraps to place<T>, early-returns none
a[i]!      // result path: unwraps to place<T>, early-returns error
```

Neither `fallible_place<T>` nor `place<T>` appear in the type system.
Bare `a[i]` without `?` or `!` is a type error -- the fallible place
must be resolved. (Exception: map upsert in `set` LHS context, see
Map Indexing below.)

At runtime, `?` and `!` on a `fallible_place<T>` perform a
bounds/existence check (not a discriminant check like `?`/`!` on
`?T`/`!T` values). From the programmer's perspective the semantics
are identical: "try this, bail on failure."

### Parallel with Checked Arithmetic

This mirrors how checked arithmetic works: `+?` and `+!` are two
error strategies for the same operation (addition that might overflow).
`[]?` and `[]!` are two error strategies for the same operation
(lookup that might miss).

| Operation | `?` variant | `!` variant |
|-----------|-------------|-------------|
| Arithmetic | `a +? b` returns `?T` | `a +! b` returns `!T` |
| Indexing | `a[i]?` early-returns `none` | `a[i]!` early-returns `error` |

The enclosing function's return type determines which variant is
valid: `?` requires the function to return `?R`, `!` requires `!R`.

### The `!` Error Value

For the `!` path, the error produced on failure is a fixed string error
per collection type:
- Lists produce `error "index out of bounds"`.
- Maps produce `error "key not found"`.

### Examples

```datalove
// Option path (function returns ?T).
let x = a[0]?
let x = a[0]?@            // linear type, explicit clone
foo(ref a[0]?)             // borrow
set a[0]? = 5              // mutation

// Result path (function returns !T).
let x = a[0]!
foo(ref a[0]!)
set a[0]! = 5

// Binop operands (implicit borrow).
let sum = a[0]? + a[1]?

// Chained place expression.
set a[0]?.field? = 5
set a[0]!.field! = 5
```

### Fallible vs Infallible

All `[]` indexing is fallible. There is no infallible/panicking
variant -- datalove does not have panics.

## Mutability Propagation Through Chains

For a chained place expression like `a[i]?.field[j]?`:

- If the final destination is `mut` or `set`, every link needs
  mutable access.
- The root `a` must be `var`.
- Each intermediate step must permit mutation.

Propagation is backward from destination to root:
`set a[i]?.x = 5` requires `a` is var, `a[i]?` is a mutable
borrow, `.x` narrows to the field.

## Relation to Existing Language Features

The place/reference model is not new machinery. Variables already
behave this way:

- `let y = x` moves (or copies) x.
- `foo(ref x)` borrows x.
- `foo(mut x)` borrows x mutably.
- `set x = ...` mutates x.
- `x + y` borrows both.

Indexing and field access extend the set of things that can appear
on the source side. The destination-context resolution is the same.

The `?`/`!` overloading for fallible places parallels how `@` is
already overloaded: it means "clone" for linear types and "widen"
for integers, resolved by context.

## `set` with Chained Place Expressions

### Chain Structure

The LHS of `set` is a chain of navigation steps from a root variable
to a final target:

```
set a[i]?.field[j]? = expr
    ^ ^^^  ^^^^^  ^^
    |  |     |     |
    |  |     |     +-- step 3: index + unwrap (fallible)
    |  |     +-------- step 2: field navigation (infallible)
    |  +-------------- step 1: index + unwrap (fallible)
    +----------------- root (must be var)
```

Each step is either:
- **Infallible**: field access (`.field`), tuple element (`.0`) --
  just narrows the location.
- **Fallible**: index (`[expr]?` or `[expr]!`) -- bounds/existence
  check, early-return on failure.

### Evaluation Order

1. **Navigate LHS chain** -- evaluate index subexpressions, perform
   bounds checks, early-return on failure. Result: a resolved place
   (a computed address; nothing written yet).
2. **Evaluate RHS** -- produces the new value. All reads complete.
3. **Drop old value** at the place (if linear type).
4. **Store new value**.

Step 1 computes where the store will land but doesn't mutate anything.
The RHS in step 2 can safely read from the same collection, even the
same element:

```datalove
set a[0]? = a[0]?@ + 1    // read old, clone, add, store back
set a[0]? = a[1]?@         // copy element 1 into slot 0
```

The mutation only happens at step 4.

### Aliasing Rule

`set` claims mutable access to the root for the duration of the
statement. The RHS can ref-borrow the root (reading elements) but
cannot take mutable or consuming access:

```datalove
set a[0]? = a[1]?@         // OK: RHS ref-borrows a
set a[0]? = bar(mut a)      // ERROR: RHS mutates a
set a[0]? = consume(a)      // ERROR: RHS consumes a
```

This is more permissive than Rust's borrow checker (which rejects
`v[0] = v[1]`). Datalove can allow it because the compiler controls
evaluation order and knows the write doesn't happen until after all
reads complete.

### Drop Timing

When overwriting a slot containing a linear value, the old value must
be dropped. The ordering prevents holes:

- If LHS navigation fails (bounds check), we never evaluate the RHS.
  Collection unchanged.
- If RHS evaluation fails (early-return from `?`), we never reach the
  drop. Old value stays intact.
- Only on full success: drop old, store new.

```datalove
var names: [string] = ["alice", "bob"]
set names[0]? = "charlie"   // drops "alice", stores "charlie"
```

### Multiple `?` in the Chain

Each `?` is an independent early-return point, checked left to right:

```datalove
set a[i]?.b[j]? = 5
```

1. Bounds-check `a[i]` -- early-return none if absent.
2. Navigate to field `.b` (infallible).
3. Bounds-check `b[j]` -- early-return none if absent.
4. Evaluate RHS, drop old, store.

If the first check succeeds but the second fails, we bail.
No write, no drop, collection unchanged.

### Index Subexpressions Can Fail

The index expression itself can contain `?`:

```datalove
set a[f(x)?]? = 5
```

1. Evaluate `f(x)` -- returns `?index`.
2. `?` unwraps -- early-return none if `f` returned none.
3. Bounds-check `a[result]` -- early-return none if absent.
4. Store `5`.

### What `set` Does Not Support

- **`out` destinations** -- `set` writes to an initialized place.
  `out` is for uninitialized locations.
- **Structural modification** -- `set a[i]? = expr` overwrites an
  element. It does not insert or remove. Growing/shrinking needs
  different operations (push, remove, etc.).

## Map Indexing

Maps introduce an operation lists don't have: writing to a key that
doesn't yet exist.

### Update vs Upsert

`set` on maps has two forms, distinguished by the presence of `?`:

```datalove
set m[key]? = v    // update: overwrite existing, early-return none if absent
set m[key] = v     // upsert: insert if absent, overwrite if present
```

The `?`/`!` consistently means "fail if absent." Without `?`/`!`,
`set` creates the slot if needed. The upsert form always succeeds --
it produces a `place<V>` directly, not a `fallible_place<V>`.

The bare `m[key]` form (without `?`) is **only valid in `set` LHS
context for maps**. In read context, bare `m[key]` is still a type
error -- there's no value to produce when the key is absent.

### Collection behavior of bare `[]` on `set` LHS

| Collection | `set x[k] = v` | `set x[k]?/! = v` |
|------------|----------------|--------------------|
| List | type error | overwrite existing element |
| Map | upsert | overwrite existing value |

Lists have a fixed index space determined by their length. You can't
conjure a new index. Maps have a dynamic key space, so bare `[]` on
`set` LHS is meaningful.

Tensor and set indexing are not yet implemented.

### Key Ownership on Upsert

When `set m[key] = v` hits an existing entry, the provided key
was already evaluated for the lookup. The existing equal key stays
in the map; the provided key is dropped (if linear).

```datalove
var m: %{string = string} = %{ "a" = "one" }
set m["a"] = "uno"    // drops provided "a", drops "one", stores "uno"
set m["b"] = "two"    // consumes "b" and "two", inserts pair
```

Summary of the two cases:
- **Key found**: drop provided key (if linear), drop old value
  (if linear), store new value. Existing key stays.
- **Key not found**: consume provided key and value, insert pair.

### Map Evaluation Order

**Update (`?` form)** -- same as the general `set` model:

1. Evaluate key, look up in map, early-return none if absent.
2. Evaluate RHS.
3. Drop old value.
4. Store new value.

**Upsert (bare form):**

1. Evaluate key.
2. Look up key in map.
3. Evaluate RHS.
4. If key existed: drop provided key (if linear), drop old value
   (if linear), store new value.
5. If key absent: insert key + value pair.

The RHS is evaluated before any drops, same as the general case.

### Map Aliasing

Same rule: the RHS can ref-borrow the map but not mutate or
consume it.

```datalove
set m[0]? = m[1]?@        // OK: RHS ref-borrows m
set m[0] = m[1]?@         // OK: upsert, RHS ref-borrows m
set m[0] = bar(mut m)      // ERROR: RHS mutates m
```

### Sets

Sets don't have key-value pairs, so `[]` on the LHS of `set`
doesn't apply. Set membership checks and mutation use dedicated
operations (contains, insert, remove), not indexing syntax.
