// Result utilities.
//
// These are generic over the ok type. The error side is always `error`, which
// is one type, so only the ok side takes a parameter.
//
// A type parameter is never copy, because the caller may supply a linear type,
// so a value taken out by `if self |value|` has moved out of `self`.

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

// Returns the contained value or zero.
fun unwrap_or_zero(self: !u32): u32
  if self |value|
    ret value
  else |error|
    ret 0
  end if
end fun

// Note: or_result and and_result cannot be reliably implemented due to
// linear semantics. The current analysis incorrectly flags valid patterns
// as UseAfterMove when using parameters across if branches.

// Convert to option, discarding error.
fun to_option<T>(self: !T): ?T
  if self |value|
    ret some value
  else |error|
    ret none
  end if
end fun
