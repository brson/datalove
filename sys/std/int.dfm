require rider std
require module sys/std/index
import std.int_from_f64
import std.int_from_f32
import index.bitand
import index.shift_right_wrapping

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
// repetition counts in the library. It is walked a bit at a time, so this
// costs a multiplication per bit rather than one per repetition; the bit
// operations are index's, since int has none of its own.
fun pow(self: int, exp: index): int
  var acc: int = 1
  var base: int = self
  var rest: index = exp
  loop while rest .> (: index / 0)
    if bitand(rest, (: index / 1)) == (: index / 1)
      let factor: int = base@
      set acc = acc * factor
    end if
    let squared: int = base@
    set base = base * squared
    set rest = shift_right_wrapping(rest, : u32 / 1)
  end loop
  ret acc
end fun

// Greatest common divisor of the magnitudes, by Euclid.
//
// Zero divides nothing, so gcd(n, 0) and gcd(0, n) are abs(n), and gcd(0, 0)
// is zero.
fun gcd(self: int, other: int): int
  var a: int = abs(self)
  var b: int = abs(other)
  loop while b != 0
    let divisor: int = b@
    if rem_checked(a, divisor) |remainder|
      set a = b
      set b = remainder
    else
      // Unreachable: the loop condition rules out a zero divisor.
      set a = b
      set b = 0
    end if
  end loop
  ret a
end fun

// Least common multiple of the magnitudes.
//
// Reduced by the divisor before multiplying, so the intermediate stays near
// the answer rather than growing to the product of both sides.
//
// The only multiple of zero is zero, so lcm(n, 0) and lcm(0, 0) are zero.
fun lcm(self: int, other: int): int
  let a: int = abs(self)
  let b: int = abs(other)
  let divisor: int = gcd(a@, b@)
  if is_zero(divisor@)
    ret 0
  else
    if div_checked(a, divisor) |reduced|
      ret reduced * b
    else
      // Unreachable: the branch above took the zero divisor.
      ret 0
    end if
  end if
end fun

// The largest integer whose square is at most self, by Newton's method.
//
// None for a negative self, which has no integer square root.
fun sqrt(self: int): ?int
  if self .< 0
    ret none
  else
    if self .< 2
      ret some self
    else
      // Newton's method, descending to the root from an overestimate. The
      // divisors are the successive estimates, which stay at or above one,
      // so /? is reached only where it answers.
      var x: int = self@
      var y: int = (self@ + 1) /? 2
      loop while y .< x
        let next: int = y@
        set x = next
        let divisor: int = x@
        let quotient: int = self@ /? divisor
        set y = (x@ + quotient) /? 2
      end loop
      ret some x
    end if
  end if
end fun

// Conversion from the floats.
//
// The language does not widen between integers and floats, so this is the way
// across. The value is truncated toward zero; a nan or an infinity has no
// integer to name and gives none.
fun from_f64(x: f64): ?int
  ret int_from_f64(x)
end fun

fun from_f32(x: f32): ?int
  ret int_from_f32(x)
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
