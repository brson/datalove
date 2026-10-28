// Each use here is above its declaration, which a module refuses as a
// script does. `helper` is called above it, and that is fine: a function is
// in scope throughout.
fun get(): i64
  ret helper() + LIMIT
end fun
fun magnitude(x: i64): i64
  ret i64.abs(x)
end fun
fun helper(): i64
  ret 1
end fun
const LIMIT: i64 = 10
require module sys/std/i64
