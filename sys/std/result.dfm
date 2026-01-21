// Result utilities for !u32 (monomorphic).

// True if result is Ok.
fun is_ok(self: !u32): bool
  if self |value|
    ret true
  else |error|
    ret false
  end if
end fun

// True if result is Err.
fun is_err(self: !u32): bool
  if self |value|
    ret false
  else |error|
    ret true
  end if
end fun

// Returns the contained value or a default.
fun unwrap_or(self: !u32, default: u32): u32
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
fun to_option(self: !u32): ?u32
  if self |value|
    ret some value
  else |error|
    ret none
  end if
end fun
