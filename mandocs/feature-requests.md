## 2026/10/01 Compare and eq for all types

```datalove
if some 1 == some 2
end if
```

## 2026/10/01 Call by module-qualified name

```datalove
require module sys/std/int

var counter = 10
if int.rem_checked(counter, 2) == some 0
end if
```