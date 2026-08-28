// Arbitrary-precision signed integers.
//
// Unlike the fixed-width integer modules, int is unbounded, so there are no
// min_value/max_value constants and no checked, saturating, or wrapping
// variants: bare +, -, * and unary - can never overflow.
//
// int is a linear type, so these functions consume their arguments. Use the
// adapt operator (@) to retain a value across a call.
//
// Division is the one fallible operation, since a zero divisor has no answer.
// It is reached through /? and /!, so anything built on it returns an option
// rather than a bare int.

// Predicates.

fun is_zero(self: int): bool
  ret self == 0
end fun

fun is_positive(self: int): bool
  ret self .> 0
end fun

fun is_negative(self: int): bool
  ret self .< 0
end fun

// Sign.

// Returns 1 if positive, -1 if negative, and 0 if zero.
fun signum(self: int): int
  if self .> 0
    ret 1
  else
    if self .< 0
      ret -1
    else
      ret 0
    end if
  end if
end fun

// Absolute value. Total, since int has no MIN to overflow on.
fun abs(self: int): int
  if self .< 0
    ret -self
  else
    ret self
  end if
end fun

// Comparison.

fun min(self: int, other: int): int
  if self <= other
    ret self
  else
    ret other
  end if
end fun

fun max(self: int, other: int): int
  if self >= other
    ret self
  else
    ret other
  end if
end fun

fun clamp(self: int, min_val: int, max_val: int): int
  if self .< min_val
    ret min_val
  else
    if self .> max_val
      ret max_val
    else
      ret self
    end if
  end if
end fun

// Absolute difference. Always non-negative.
fun abs_diff(self: int, other: int): int
  if self >= other
    ret self - other
  else
    ret other - self
  end if
end fun

// Division.

// Quotient, truncated toward zero. None if other is zero.
fun div_checked(self: int, other: int): ?int
  ret some (self /? other)
end fun

// Remainder, taking the sign of self. None if other is zero.
fun rem_checked(self: int, other: int): ?int
  let quotient: int = self@ /? other@
  ret some (self - quotient * other)
end fun

// Quotient and remainder together. None if other is zero.
fun div_rem(self: int, other: int): ?(int, int)
  let quotient: int = self@ /? other@
  let scaled: int = quotient@
  let remainder: int = self - scaled * other
  ret some (quotient, remainder)
end fun

// Exponentiation.

// Raises self to the power of exp.
//
// The exponent is an index because it counts repetitions, matching the other
// repetition counts in the library. This performs exp multiplications rather
// than the usual squaring ladder, which needs bit operations int does not have.
fun pow(self: int, exp: index): int
  let limit: int = exp@
  var acc: int = 1
  var i: int = 0
  loop while i .< limit
    let factor: int = self@
    set acc = acc * factor
    set i = i + 1
  end loop
  ret acc
end fun

// Returns n factorial, or 1 when n is zero.
fun factorial(n: index): int
  let limit: int = n@
  var acc: int = 1
  var i: int = 1
  loop while i <= limit
    let factor: int = i@
    set acc = acc * factor
    set i = i + 1
  end loop
  ret acc
end fun
