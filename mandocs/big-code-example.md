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
let bytes: list<u8> = [1, 2, 3]

// Option and result types with builtins
let maybe: ?u32 = some 10
let nothing: ?u32 = none
let success: !u32 = ok 200
let failure: !u32 = er error "oops"

// Function definition with all brace types: { } ( ) < > [ ]
fun calculate(a: u32, b: u32): !u32
  // Checked-result operators: +! -! *! /! !
  let sum = a +! b
  let neg = -!a
  let prod = a *! b
  let div = a /! b
  let unwrap = success!

  // Checked-option operators: +? -? *? /? ?
  let opt_sum = a +? b
  let opt_neg = -?a
  let opt_prod = a *? b
  let opt_div = a /? b
  let opt_unwrap = maybe?

  // Comparison operators: .< .> <= >= == !=
  if a .< b
    ret ok 1
  else if a .> b
    ret ok 2
  else if a <= b
    ret ok 3
  else if a >= b
    ret ok 4
  else if a == b
    ret ok 5
  else if a != b
    ret ok 6
  end if

  ret ok sum
end fun

// Struct with curly braces { }
struct Point {
  x: f32,
  y: f32,
}

// Enum with parens ( ) and curlies { }
enum Status {
  Active,
  Inactive,
  Pending(u32),
}

// Container types with angle < > and square [ ] braces
let items: list<u32> = [1, 2, 3, 4, 5]
let lookup: map<u32, bool> = { 1 = true, 2 = false }
let unique: set<u32> = { 10, 20, 30 }

// Function call with parens ( )
let result = calculate(x, y)
```
