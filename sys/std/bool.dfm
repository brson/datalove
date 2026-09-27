
// Logical AND.
fun and(self: bool, other: bool): bool
  if self
    ret other
  else
    ret false
  end if
end fun

// Logical OR.
fun or(self: bool, other: bool): bool
  if self
    ret true
  else
    ret other
  end if
end fun

// Logical XOR.
fun xor(self: bool, other: bool): bool
  if self
    if other
      ret false
    else
      ret true
    end if
  else
    if other
      ret true
    else
      ret false
    end if
  end if
end fun

// Logical implication (self -> other).
fun implies(self: bool, other: bool): bool
  if self
    ret other
  else
    ret true
  end if
end fun

// Returns Some(value) if self is true, else None.
fun then_some<T>(self: bool, value: T): ?T
  if self
    ret some value
  else
    ret none
  end if
end fun
