# Syntax Highlighting Demo

```datalove
// This is a line comment

/*
  This is a block comment
  spanning multiple lines
*/

require module sys/core/debug

// Keywords and types
let a: bool = true
let b: bool = false
let x: u32 = 42
let y: i64 = -100
let z: int = 9999999999999999999
let f: f32 = 3.14
let name: string = "hello world"
let name: list<u8> = [1, 2, 3]

// Option and result types with builtins
let maybe: ?u32 = some 10
let nothing: ?u32 = none
let success: !u32 = ok 200
let failure: !u32 = er error "oops"

// Function definition
fun calculate(a: u32, b: u32): !u32
  // Checked operators
  let sum = a +! b
  let diff = a -? b
  let prod = a *! b

  // Boolean operators
  let check = true and false or true
  let logic = a .< b implies b .> 0
  let neg = not check

  // Control flow
  if sum > 100
    ret ok sum
  else if diff |val|
    ret ok val
  else
    ret err error "failed"
  end if
end fun

// Struct and enum
struct Point {
  x: f32,
  y: f32,
}

enum Status {
  Active,
  Inactive,
  Pending(u32),
}

// Container types
let items: list<u32> = [1, 2, 3, 4, 5]
let lookup: map<u32, bool> = { 1 = true, 2 = false }
let unique: set<u32> = { 10, 20, 30 }

// Function call
let result = calculate(x, y)

// Match expression
match result |value|
  debug.print(value)
end match
```
