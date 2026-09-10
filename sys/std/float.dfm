// Functions over either float.
//
// A type parameter bounded to `float` is `f32` or `f64`, and in exchange for
// saying so the body may do what both have in common: the arithmetic, the
// comparisons, and the operations the rider offers below. Which of the two it
// turned out to be is read off the descriptor that travels with the value, the
// same way a collection's elements are.
//
// So there is one of each of these rather than one per width, and a caller
// picks by what it passes.

require rider std
import std.float_abs
import std.float_sqrt
import std.float_floor
import std.float_ceil
import std.float_round
import std.float_trunc
import std.float_fract
import std.float_recip
import std.float_signum
import std.float_neg

// The magnitude, without its sign.
fun abs<T: float>(self: T): T
  ret float_abs(self)
end fun

// The square root.
fun sqrt<T: float>(self: T): T
  ret float_sqrt(self)
end fun

// The largest whole number no greater than self.
fun floor<T: float>(self: T): T
  ret float_floor(self)
end fun

// The smallest whole number no less than self.
fun ceil<T: float>(self: T): T
  ret float_ceil(self)
end fun

// The nearest whole number, halves away from zero.
fun round<T: float>(self: T): T
  ret float_round(self)
end fun

// The whole part, dropping the fraction.
fun trunc<T: float>(self: T): T
  ret float_trunc(self)
end fun

// The fractional part, keeping the sign.
fun fract<T: float>(self: T): T
  ret float_fract(self)
end fun

// One divided by self.
fun recip<T: float>(self: T): T
  ret float_recip(self)
end fun

// 1, -1 or a nan, matching self's sign.
fun signum<T: float>(self: T): T
  ret float_signum(self)
end fun

// Self with its sign flipped.
fun neg<T: float>(self: T): T
  ret float_neg(self)
end fun

// The smaller of the two. A nan compares false against everything, so a nan
// on the left gives the right.
fun min<T: float>(self: T, other: T): T
  if self .< other
    ret self
  else
    ret other
  end if
end fun

// The larger of the two.
fun max<T: float>(self: T, other: T): T
  if self .> other
    ret self
  else
    ret other
  end if
end fun

// Self held between two bounds.
fun clamp<T: float>(self: T, low: T, high: T): T
  ret min(max(self, low), high)
end fun

// The point a fraction of the way from a to b.
fun lerp<T: float>(a: T, b: T, t: T): T
  ret a + (b - a) * t
end fun

// True if self is a nan, which is the one value that is not equal to itself.
fun is_nan<T: float>(self: T): bool
  ret self != self
end fun

// The difference between the two, without its sign.
fun distance<T: float>(self: T, other: T): T
  ret float_abs(self - other)
end fun
