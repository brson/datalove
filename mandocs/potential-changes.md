## Remove error data data keywords etc

remove data / error keywords - rely on ~ coercion?

similar coercion for "er error"




## Replace `=`

Current usage in maps is awkward,
also in struct field destructuring.
Maybe use arrows instead.




## Less than and equals

The dots are visually confusing with method/field dots:

`.<` `.>`




## Not-equals has a bang in it

Want ! to be reserved for error handling, but `!=` is unfortunate.




## map -> dict?

just to free up the word map




## Type hints

- `: type / expr` -> `: type is expr`
  - or `: type = ` <---- this might be the one because

```datalove
struct Foo {
  bar = 1,
  baz: int = 2,
}
```

vs

```datalove
struct Foo {
  bar = 1,
  baz = : int is 2,
}
```

tydec:
```datalove
struct Foo {
  bar: int,
  baz: int,
}
```

seems to be very orthogonal.
can we say "any `=` can be expanded to `: =`?