## 2026/10/08 Lossless mixed-type math ops

Being able to do

```
set sum += i
```

where sum is a bigint and i is not would be a strong surface-level semantic
leading to an optimization, not requiring a peephole for small bigint consts.
would prefer to have it at the typecheck level for mixed-type ops
and not a general widening feature (widening would imply i gets converted to a bigint, defeating the optimization).


## 2026/10/08 Saturating and wrapping math ops

zig-style `+|`, `+%` operators.
still might be nice in a scripting language, especially to avoid `?` and `!`.
also the compound versions, `+|=`, `+%=`.


## 2026/10/07 Collection iteration

Iterating maps and sets is expensive because it requires reindexing every item.


## 2026/10/06 Generic type aliases

Function arguments can already specify generic types
so it would be nice if type aliases could do the same.


## 2026/10/03 Enum subtyping and synthesis

This is ugly:

```datalove
{
  genres = : #{enum { atom Fiction, atom Utopia }} / #{ atom Fiction, atom Utopia },
}
```

Would be much better to synthesize an enum type,
but that's probably not viable without subtyping so the synthesized type can be assigned to the 'real' type.
Synthesis would require unifying entire collections though, expensive.


## 2026/10/02 Bare binops that produce optionals

Would be nice:

```datalove
fun do_some_math(a: u32, b: u32, c: u32): ?u32
  ret (a + b) / c
end fun
```

## 2026/10/02 Compare and eq for more types

```datalove
if some 1 == some 2
end if
```

Semantics for aggregates get complex.
