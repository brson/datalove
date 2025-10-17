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

### Recursion Works

✅ Functions can call themselves
✅ Type checking validates recursive calls
✅ Stack overflow protection prevents infinite recursion

### Current Limitation: No Conditionals

❌ **Practical recursive algorithms require conditional statements (if/when), which are not yet implemented.**

Without conditionals, you cannot write terminating recursive functions like factorial or fibonacci. For example, this factorial definition **cannot be written** in current datafun:

```datafun
# NOT VALID - no if/when statements exist yet
fun factorial(n: @u32): @u32
  if n .<= @1
    ret @1
  else
    ret n * factorial(n - @1)
  end if
end fun
```

### Infinite Recursion Example

Without conditionals, recursive functions will hit stack overflow protection:

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

## Next Steps

To enable practical recursion, datafun needs:

1. **Conditional statements** - `if/when/else` for branching logic
2. **Comparison expressions** - already have `.<`, `.>`, `<=`, `>=`, `==`, `!=`
3. **Boolean expressions** - `and`, `or`, `not`

## See Also

- [Datalit Types](../datalit/types.md)
- [Binary Operators](./operators.md)
