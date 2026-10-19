// Moving values in and out of places.
//
// A `mut` parameter cannot be moved out of, because the caller still holds
// it, and `set` destroys what it overwrites. So without these the only way to
// get a value out of a place is to clone it. `replace` and `swap` move one out
// and another in at the same step, which leaves the place holding something
// the whole time, and cost no clone whatever the type is.
//
// These are generic over the type moved, so they work on a whole collection,
// an element reached through an index, a field of a `var` aggregate, or
// anything else that can be passed `mut`.

require rider std

import std.mem_replace
import std.mem_swap

// Put a value in a place and return the one that was there.
fun replace<T>(mut dest: T, src: T): T
  inline
  ret mem_replace(mut dest, src)
end fun

// Exchange the values in two places.
//
// The two have to be rooted at different bindings, which rules out the two
// halves of one tuple (D010). `list.swap` exchanges two elements of one list.
fun swap<T>(mut a: T, mut b: T)
  inline
  call mem_swap(mut a, mut b)
end fun

// Let go of a value now rather than at the end of its scope.
//
// The parameter takes the value and the body does nothing with it, so it is
// destroyed when the call returns.
fun drop<T>(value: T)
  inline
end fun
