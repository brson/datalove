## map -> dict?

just to free up the word map


## Type hints

- `: type / expr` -> `: type is expr`
  - or `: type = ` <---- this might be the one because

```
struct Foo {
  bar = 1,
  baz: int = 2,
}
```

vs

```
struct Foo {
  bar = 1,
  baz = : int is 2,
}
```

tydec:
```
struct Foo {
  bar: int,
  baz: int,
}
```

seems to be very orthogonal.
can we say "any `=` can be expanded to `: =`?