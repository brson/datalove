# Control Flow in Datalove

### Unconditional and conditional loops

Unconditional loops are spelled `loop`.
They require a `break` or `ret` to exit.

```datalove
var x = 0

loop
  if x = 0
    set x = 1
  else if x = 10
    set x = 11
    break
  end if
  set x = x + 1
end loop

debuglog x
```

Conditional loops with `loop while`:

```datalove
var x = 0

loop while x != 10
  if x = 0
    set x = 1
  else if x = 10
    set x = 11
  else
    set x = x + 1
  end if
end loop

debuglog x
```


### Conditional initialization

Uninitialized vars can be initialized in branches,
but must be initialized in all branches if read afterward.

```datalove
var result: i32

if condition
  set result = 1
else
  set result = 2
end if

debuglog result   // ok - initialized on all paths
```

This won't compile:

```datalove
var result: i32

if condition
  set result = 1
end if

debuglog result   // error - may be uninitialized
```
