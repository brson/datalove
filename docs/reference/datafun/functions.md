# Datafun Functions

Datafun supports function definitions and function calls with full recursion support.

## Function Definitions

```datafun
fun function_name(param1: Type1, param2: Type2): ReturnType
  # Function body
  ret expression
end fun
```

### Syntax

- Function parameters must have explicit type annotations
- Return type must be explicitly specified
- Function body can contain multiple statements
- Must end with `ret` statement

### Examples

**No parameters:**
```datafun
fun get_five(): @u32
  ret @5
end fun
```

**Single parameter:**
```datafun
fun double(x: @u32): @u32
  ret x + x
end fun
```

**Multiple parameters:**
```datafun
fun add(a: @u32, b: @u32): @u32
  ret a + b
end fun
```

## Function Calls

```datafun
function_name(arg1, arg2, ...)
```

Function calls can be used anywhere an expression is expected:

```datafun
let result = double(@21)  # result = @42
let sum = add(@10, @20)   # sum = @30
```

## Recursion

Datafun supports recursive function calls with automatic stack overflow protection (max depth: 1000).

✅ Functions can call themselves
✅ Type checking validates recursive calls
✅ Stack overflow protection prevents infinite recursion
✅ Conditional statements enable practical recursion

### Practical Recursion with Conditionals

With conditional statements, you can write terminating recursive functions like factorial:

```datafun
fun factorial(n: @u32): @u32
  if n .<= @1
    ret @1
  else
    ret n * factorial(n - @1)
  end if
end fun

let result = factorial(@5)  # result = @120
```

### Infinite Recursion Protection

Without conditionals to terminate recursion, functions will hit stack overflow protection:

```datafun
fun infinite(n: @u32): @u32
  ret infinite(n)
end fun

# This will error: StackOverflow after 1000 calls
let output = infinite(@0)
```

## Type Checking

- Function signature is checked when defined
- Argument count must match parameter count
- Argument types must match parameter types
- Return expression must match return type

## Scoping

- Parameters shadow variables of the same name
- Original variables are restored after function returns
- Functions see global variables defined before them

## Conditional Statements

Datafun supports if/else statements for conditional branching.

### Syntax

```datafun
if condition
  # statements executed if condition is true
end if
```

With else clause:

```datafun
if condition
  # statements executed if condition is true
else
  # statements executed if condition is false
end if
```

### Condition Types

The condition must be a boolean expression. Comparison operators return booleans:

- `.<` - less than
- `.>` - greater than
- `<=` - less than or equal
- `>=` - greater than or equal
- `==` - equal
- `!=` - not equal

### Examples

**Simple conditional:**
```datafun
fun abs(x: @i32): @i32
  if x .< @0
    ret @0 - x
  else
    ret x
  end if
end fun
```

**Conditional in recursion:**
```datafun
fun factorial(n: @u32): @u32
  if n .<= @1
    ret @1
  else
    ret n * factorial(n - @1)
  end if
end fun
```

**Multiple statements in branches:**
```datafun
fun classify(x: @u32): @u32
  if x .< @10
    ret @1
  else
    ret @2
  end if
end fun
```

### Limitations

- No `else if` or `elif` - use nested if statements
- No boolean operators (`and`, `or`, `not`) yet
- No ternary conditional expressions

## See Also

- [Datalit Types](../datalit/types.md)
- [Binary Operators](./operators.md)
