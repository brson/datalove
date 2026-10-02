require rider std
import std.f32_sin
import std.f32_cos
import std.f32_tan
import std.f32_asin
import std.f32_acos
import std.f32_atan
import std.f32_sinh
import std.f32_cosh
import std.f32_tanh
import std.f32_exp
import std.f32_exp2
import std.f32_ln
import std.f32_log2
import std.f32_log10
import std.f32_cbrt
import std.f32_atan2
import std.f32_log
import std.f32_pow
import std.f32_hypot
import std.f32_rem
import std.f32_from_int

// Constants.

fun nan(): f32
  ret (: f32 / 0x7FC00000)
end fun

fun infinity(): f32
  ret (: f32 / 0x7F800000)
end fun

fun neg_infinity(): f32
  ret (: f32 / 0xFF800000)
end fun

fun max_value(): f32
  ret (: f32 / 0x7F7FFFFF)
end fun

fun min_value(): f32
  ret (: f32 / 0x00800000)
end fun

fun min_positive(): f32
  ret (: f32 / 0x00000001)
end fun

fun epsilon(): f32
  ret (: f32 / 0x34000000)
end fun

// Classification.

fun is_nan(self: f32): bool
  ret icall is_nan_f32(self)
end fun

fun is_infinite(self: f32): bool
  ret icall is_infinite_f32(self)
end fun

fun is_finite(self: f32): bool
  if is_nan(self)
    ret false
  else
    if is_infinite(self)
      ret false
    else
      ret true
    end if
  end if
end fun

fun is_normal(self: f32): bool
  if is_finite(self)
    if is_zero(self)
      ret false
    else
      if is_subnormal(self)
        ret false
      else
        ret true
      end if
    end if
  else
    ret false
  end if
end fun

fun is_subnormal(self: f32): bool
  if is_finite(self)
    if is_zero(self)
      ret false
    else
      let bits = icall f32_to_bits(self)
      let exponent_mask = : u32 / 0x7F800000
      let exponent = icall bitand_u32(bits, exponent_mask)
      ret exponent == (: u32 / 0)
    end if
  else
    ret false
  end if
end fun

fun is_sign_positive(self: f32): bool
  let bits = icall f32_to_bits(self)
  let sign_bit = : u32 / 0x80000000
  ret icall bitand_u32(bits, sign_bit) == (: u32 / 0)
end fun

fun is_sign_negative(self: f32): bool
  let bits = icall f32_to_bits(self)
  let sign_bit = : u32 / 0x80000000
  ret icall bitand_u32(bits, sign_bit) != (: u32 / 0)
end fun

// Sign and absolute value.

fun abs(self: f32): f32
  ret icall abs_f32(self)
end fun

fun signum(self: f32): f32
  if is_nan(self)
    ret self
  else
    if is_sign_positive(self)
      ret 1.0
    else
      ret -1.0
    end if
  end if
end fun

fun copysign(self: f32, sign: f32): f32
  ret icall copysign_f32(self, sign)
end fun

// Rounding.

fun floor(self: f32): f32
  ret icall floor_f32(self)
end fun

fun ceil(self: f32): f32
  ret icall ceil_f32(self)
end fun

fun round(self: f32): f32
  ret icall round_f32(self)
end fun

fun trunc(self: f32): f32
  ret icall trunc_f32(self)
end fun

fun fract(self: f32): f32
  ret self - trunc(self)
end fun

// Arithmetic.

fun recip(self: f32): f32
  ret 1.0 / self
end fun

fun sqrt(self: f32): f32
  ret icall sqrt_f32(self)
end fun

fun min(self: f32, other: f32): f32
  ret icall min_f32(self, other)
end fun

fun max(self: f32, other: f32): f32
  ret icall max_f32(self, other)
end fun

fun clamp(self: f32, min_val: f32, max_val: f32): f32
  ret min(max(self, min_val), max_val)
end fun

// Bit manipulation.

fun to_bits(self: f32): u32
  ret icall f32_to_bits(self)
end fun

fun from_bits(bits: u32): f32
  ret icall bits_to_f32(bits)
end fun

// Comparison utilities.

fun is_zero(self: f32): bool
  ret self == 0.0
end fun

// Total ordering comparison. Returns -1 if self < other, 0 if equal, 1 if self > other.
// Uses IEEE 754 totalOrder semantics: -NaN < -Inf < ... < -0 < +0 < ... < +Inf < +NaN.
fun total_cmp(self: f32, other: f32): i32
  let a_bits = icall f32_to_bits(self)
  let b_bits = icall f32_to_bits(other)

  // Convert to signed for total ordering.
  let sign_bit = : u32 / 0x80000000
  let a_signed = icall u32_to_i32(a_bits)
  let b_signed = icall u32_to_i32(b_bits)

  // For negative numbers (sign bit set), flip all bits except sign to get correct ordering.
  var a_ord: i32 = a_signed
  var b_ord: i32 = b_signed

  if icall bitand_u32(a_bits, sign_bit) != (: u32 / 0)
    let mask = : u32 / 0x7FFFFFFF
    set a_ord = icall u32_to_i32(icall bitxor_u32(a_bits, mask))
  end if

  if icall bitand_u32(b_bits, sign_bit) != (: u32 / 0)
    let mask = : u32 / 0x7FFFFFFF
    set b_ord = icall u32_to_i32(icall bitxor_u32(b_bits, mask))
  end if

  if a_ord .< b_ord
    ret (: i32 / -1)
  else
    if a_ord .> b_ord
      ret : i32 / 1
    else
      ret : i32 / 0
    end if
  end if
end fun

// Transcendental functions.
//
// Unlike the rest of this module these are calls into the platform's math
// library rather than machine instructions, so they cost a call and are
// correctly rounded only as far as that library is.

// Sine of an angle in radians.
fun sin(self: f32): f32
  ret f32_sin(self)
end fun

// Cosine of an angle in radians.
fun cos(self: f32): f32
  ret f32_cos(self)
end fun

// Tangent of an angle in radians.
fun tan(self: f32): f32
  ret f32_tan(self)
end fun

// Arcsine in radians, in [-pi/2, pi/2]. Nan outside [-1, 1].
fun asin(self: f32): f32
  ret f32_asin(self)
end fun

// Arccosine in radians, in [0, pi]. Nan outside [-1, 1].
fun acos(self: f32): f32
  ret f32_acos(self)
end fun

// Arctangent in radians, in [-pi/2, pi/2].
fun atan(self: f32): f32
  ret f32_atan(self)
end fun

// Hyperbolic sine.
fun sinh(self: f32): f32
  ret f32_sinh(self)
end fun

// Hyperbolic cosine.
fun cosh(self: f32): f32
  ret f32_cosh(self)
end fun

// Hyperbolic tangent.
fun tanh(self: f32): f32
  ret f32_tanh(self)
end fun

// e raised to the power of self.
fun exp(self: f32): f32
  ret f32_exp(self)
end fun

// 2 raised to the power of self.
fun exp2(self: f32): f32
  ret f32_exp2(self)
end fun

// Natural logarithm. Nan for a negative self, -inf for zero.
fun ln(self: f32): f32
  ret f32_ln(self)
end fun

// Base 2 logarithm.
fun log2(self: f32): f32
  ret f32_log2(self)
end fun

// Base 10 logarithm.
fun log10(self: f32): f32
  ret f32_log10(self)
end fun

// Cube root, defined for negative values as well.
fun cbrt(self: f32): f32
  ret f32_cbrt(self)
end fun

// Arctangent of self/other in radians, using the signs of both to place the result in the right quadrant.
fun atan2(self: f32, other: f32): f32
  ret f32_atan2(self, other)
end fun

// Logarithm of self in the given base.
fun log(self: f32, base: f32): f32
  ret f32_log(self, base)
end fun

// self raised to the power of exp.
fun pow(self: f32, exp: f32): f32
  ret f32_pow(self, exp)
end fun

// The length of the hypotenuse, without the overflow that squaring both sides invites.
fun hypot(self: f32, other: f32): f32
  ret f32_hypot(self, other)
end fun

// The remainder of self/other, taking the sign of self. This is what fmod computes, not a modulo.
fun rem(self: f32, other: f32): f32
  ret f32_rem(self, other)
end fun

// Constants.

// The ratio of a circle's circumference to its diameter.
fun pi(): f32
  ret 3.1415927410125732421875
end fun

// The base of the natural logarithm.
fun e(): f32
  ret 2.71828174591064453125
end fun

// Two pi: one full turn in radians.
fun tau(): f32
  ret 6.283185482025146484375
end fun

// Angle conversion.

fun to_degrees(self: f32): f32
  ret self * (180.0 / pi())
end fun

fun to_radians(self: f32): f32
  ret self * (pi() / 180.0)
end fun

// Conversion from the integers.
//
// The language does not widen between integers and floats, so this is the
// way across. None when the value is too large for a f32 to hold; a value
// that fits but has more digits than the format carries is rounded.
fun from_int(ref n: int): ?f32
  ret f32_from_int(ref n)
end fun

// Conversion from the other width.
//
// `@` widens an f32 to an f64 but never narrows, so this is the way down.
// `f64.from_f32` and `@` are the way up.
//
// Narrowing rounds to the nearest f32, so it loses precision by design. A
// value too small to hold rounds to zero, which is the nearest f32 to it and
// so an answer like any other. A value too large does not: an infinity is not
// near anything, so that is none rather than a number the caller did not
// mean. A nan or an infinity converts to itself, being the same value in
// either width.
fun from_f64(x: f64): ?f32
  let narrowed: f32 = icall f64_to_f32(x)
  if is_finite(narrowed) or is_nan(narrowed)
    ret some narrowed
  else
    // The result is infinite, which is either the infinity that went in or a
    // value too large to hold. Widening back is exact, so the two are told
    // apart by whether what comes back is what went in. The intrinsics are
    // reached directly because f64's own predicates answer to names this
    // module already has.
    let widened: f64 = icall f32_to_f64(narrowed)
    if widened == x
      ret some narrowed
    else
      ret none
    end if
  end if
end fun
