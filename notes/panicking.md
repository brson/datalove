We're going to try making all funs total,
but if that doesn't work.

## Panicking

Datafun supports limited panicking:
the thread immediately halts,
frees all local memory,
and owned global memory,
and returns control to the embedder or system.
What happens to other threads in a multithreaded scenario is tbd.

`panic` is a statement:

```
fun expect(val: ?u32): u32
  if val |val|
    ret val
  else |error|
    panic #"oops"
  end if
end
```

Note that panic accepts any type on the global heap,
and we can use `clone` to duplicate to the global heap:

```
fun expect(val: ?u32): u32
  if val |val|
    ret val
  else |error|
    panic error.clone
  end if
end
```

> `clone` is a dual-positon-op that works either prefix
  or as a `.clone` postfix.

Panicking unwrap with the `.X` postfix operator:

```
fun expect(val: ?u32): u32
  ret val.X
end
```

`panic` and `.X` are the only ways to panic.
The type system does track which functions panic,
but it is not exposed in the type signature.
It is possible to assert non-panicking:

```
fun transform_option(val: ?u32): ?u32
  let val = val?
  ret val + 1
end

make transform_option total
```


### Panicking math ops

```

```