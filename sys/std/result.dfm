// Result utilities.
//
// These are generic over the ok type. The error side is always `error`, which
// is one type, so only the ok side takes a parameter.
//
// A type parameter is never copy, because the caller may supply a linear type,
// so a value taken out by `if self |value|` has moved out of `self`.

require module sys/std/fixedint

import fixedint.zero

// True if result is Ok.
fun is_ok<T>(self: !T): bool
  if self |value|
    ret true
  else |error|
    ret false
  end if
end fun

// True if result is Err.
fun is_err<T>(self: !T): bool
  if self |value|
    ret false
  else |error|
    ret true
  end if
end fun

// Returns the contained value or a default.
fun unwrap_or<T>(self: !T, default: T): T
  if self |value|
    ret value
  else |error|
    ret default
  end if
end fun

// The contained value, or the type's own zero.
//
// Bounded to `fixedint` rather than written at one width, because a zero can
// be made at any of them: `sys/std/fixedint` asks the rider for one and the
// call site's descriptor says which.
fun unwrap_or_zero<T>(self: !T): T with { T is fixedint, }
  if self |value|
    ret value
  else |failure|
    let unwanted = failure
    ret zero()
  end if
end fun

// Returns other if self is Ok, otherwise self's error.
fun and_result<T>(self: !T, other: !T): !T
  if self |value|
    ret other
  else |e|
    ret er e
  end if
end fun

// Pairs two results, which is ok only when both are, and otherwise carries the
// first error.
//
// The two need not hold the same type, and the pair is built inside a function
// that knows neither.
fun zip_result<A, B>(self: !A, other: !B): !(A, B)
  if self |a|
    if other |b|
      ret ok (a, b)
    else |e|
      ret er e
    end if
  else |e|
    ret er e
  end if
end fun

// Returns self if it is Ok, otherwise other.
fun or_result<T>(self: !T, other: !T): !T
  if self |value|
    ret ok value
  else |e|
    ret other
  end if
end fun

// The error, if there is one.
fun err_of<T>(self: !T): ?error
  if self |value|
    ret none
  else |e|
    ret some e
  end if
end fun

// Convert to option, discarding error.
fun to_option<T>(self: !T): ?T
  if self |value|
    ret some value
  else |error|
    ret none
  end if
end fun
