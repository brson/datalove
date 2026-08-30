// Option utilities.
//
// These are generic over the contained type. A type parameter is erased to
// `data` when the function is compiled, and the call site converts on the way
// in and back out, so there is one copy of each of these no matter how many
// types go through it.
//
// A type parameter is never copy, because the caller may supply a linear type.
// So a value taken out of an option by `if self |value|` has moved out of
// `self`, and these rebuild the option with `some value` rather than returning
// the `self` they destructured.

// True if the option holds a value.
fun is_some<T>(self: ?T): bool
  if self |value|
    ret true
  else
    ret false
  end if
end fun

// True if the option is empty.
fun is_none<T>(self: ?T): bool
  if self |value|
    ret false
  else
    ret true
  end if
end fun

// Returns the contained value or a default.
fun unwrap_or<T>(self: ?T, default: T): T
  if self |value|
    ret value
  else
    ret default
  end if
end fun

// Returns self if it holds a value, otherwise other.
fun or_option<T>(self: ?T, other: ?T): ?T
  if self |value|
    ret some value
  else
    ret other
  end if
end fun

// Returns whichever of the two holds a value, or none if both or neither do.
fun xor_option<T>(self: ?T, other: ?T): ?T
  if self |self_value|
    if other |other_value|
      ret none
    else
      ret some self_value
    end if
  else
    if other |other_value|
      ret some other_value
    else
      ret none
    end if
  end if
end fun

// Returns other if self holds a value, otherwise none.
fun and_option<T>(self: ?T, other: ?T): ?T
  if self |value|
    ret other
  else
    ret none
  end if
end fun

// Removes one level of nesting.
fun flatten<T>(self: ??T): ?T
  if self |inner|
    ret inner
  else
    ret none
  end if
end fun

// Converts to a result, using the given error for none.
fun ok_or<T>(self: ?T, err: error): !T
  if self |value|
    ret ok value
  else
    ret er err
  end if
end fun

// Returns the contained value or zero.
fun unwrap_or_zero(self: ?u32): u32
  if self |value|
    ret value
  else
    ret 0
  end if
end fun
