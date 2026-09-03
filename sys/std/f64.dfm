require rider std
import std.f64_sin
import std.f64_cos
import std.f64_tan
import std.f64_asin
import std.f64_acos
import std.f64_atan
import std.f64_sinh
import std.f64_cosh
import std.f64_tanh
import std.f64_exp
import std.f64_exp2
import std.f64_ln
import std.f64_log2
import std.f64_log10
import std.f64_cbrt
import std.f64_atan2
import std.f64_log
import std.f64_pow
import std.f64_hypot
import std.f64_rem
import std.f64_from_int

// Constants.

fun nan(): f64
  ret (: f64 / 0x7FF8000000000000)
end fun

fun infinity(): f64
  ret (: f64 / 0x7FF0000000000000)
end fun

fun neg_infinity(): f64
  ret (: f64 / 0xFFF0000000000000)
end fun

fun max_value(): f64
  ret (: f64 / 0x7FEFFFFFFFFFFFFF)
end fun

fun min_value(): f64
  ret (: f64 / 0x0010000000000000)
end fun

fun min_positive(): f64
  ret (: f64 / 0x0000000000000001)
end fun

fun epsilon(): f64
  ret (: f64 / 0x3CB0000000000000)
end fun

// Classification.

fun is_nan(self: f64): bool
  ret icall is_nan_f64(self)
end fun

fun is_infinite(self: f64): bool
  ret icall is_infinite_f64(self)
end fun

fun is_finite(self: f64): bool
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

fun is_normal(self: f64): bool
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

fun is_subnormal(self: f64): bool
  if is_finite(self)
    if is_zero(self)
      ret false
    else
      let bits = icall f64_to_bits(self)
      let exponent_mask = (: u64 / 0x7FF0000000000000)
      let exponent = icall bitand_u64(bits, exponent_mask)
      let zero = : u64 / 0
      ret exponent == zero
    end if
  else
    ret false
  end if
end fun

fun is_sign_positive(self: f64): bool
  let bits = icall f64_to_bits(self)
  let sign_bit = (: u64 / 0x8000000000000000)
  let zero = : u64 / 0
  ret icall bitand_u64(bits, sign_bit) == zero
end fun

fun is_sign_negative(self: f64): bool
  let bits = icall f64_to_bits(self)
  let sign_bit = (: u64 / 0x8000000000000000)
  let zero = : u64 / 0
  ret icall bitand_u64(bits, sign_bit) != zero
end fun

// Sign and absolute value.

fun abs(self: f64): f64
  ret icall abs_f64(self)
end fun

fun signum(self: f64): f64
  if is_nan(self)
    ret self
  else
    if is_sign_positive(self)
      ret (: f64 / 1.0)
    else
      ret (: f64 / -1.0)
    end if
  end if
end fun

fun copysign(self: f64, sign: f64): f64
  ret icall copysign_f64(self, sign)
end fun

// Rounding.

fun floor(self: f64): f64
  ret icall floor_f64(self)
end fun

fun ceil(self: f64): f64
  ret icall ceil_f64(self)
end fun

fun round(self: f64): f64
  ret icall round_f64(self)
end fun

fun trunc(self: f64): f64
  ret icall trunc_f64(self)
end fun

fun fract(self: f64): f64
  ret self - trunc(self)
end fun

// Arithmetic.

fun recip(self: f64): f64
  ret (: f64 / 1.0) / self
end fun

fun sqrt(self: f64): f64
  ret icall sqrt_f64(self)
end fun

fun min(self: f64, other: f64): f64
  ret icall min_f64(self, other)
end fun

fun max(self: f64, other: f64): f64
  ret icall max_f64(self, other)
end fun

fun clamp(self: f64, min_val: f64, max_val: f64): f64
  ret min(max(self, min_val), max_val)
end fun

// Bit manipulation.

fun to_bits(self: f64): u64
  ret icall f64_to_bits(self)
end fun

fun from_bits(bits: u64): f64
  ret icall bits_to_f64(bits)
end fun

// Comparison utilities.

fun is_zero(self: f64): bool
  ret self == (: f64 / 0.0)
end fun

// Total ordering comparison. Returns -1 if self < other, 0 if equal, 1 if self > other.
// Uses IEEE 754 totalOrder semantics: -NaN < -Inf < ... < -0 < +0 < ... < +Inf < +NaN.
fun total_cmp(self: f64, other: f64): i32
  let a_bits = icall f64_to_bits(self)
  let b_bits = icall f64_to_bits(other)

  // Convert to signed for total ordering.
  let sign_bit = (: u64 / 0x8000000000000000)
  let zero = : u64 / 0
  let a_signed = icall u64_to_i64(a_bits)
  let b_signed = icall u64_to_i64(b_bits)

  // For negative numbers (sign bit set), flip all bits except sign to get correct ordering.
  var a_ord: i64 = a_signed
  var b_ord: i64 = b_signed

  if icall bitand_u64(a_bits, sign_bit) != zero
    let mask = (: u64 / 0x7FFFFFFFFFFFFFFF)
    set a_ord = icall u64_to_i64(icall bitxor_u64(a_bits, mask))
  end if

  if icall bitand_u64(b_bits, sign_bit) != zero
    let mask = (: u64 / 0x7FFFFFFFFFFFFFFF)
    set b_ord = icall u64_to_i64(icall bitxor_u64(b_bits, mask))
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
fun sin(self: f64): f64
  ret f64_sin(self)
end fun

// Cosine of an angle in radians.
fun cos(self: f64): f64
  ret f64_cos(self)
end fun

// Tangent of an angle in radians.
fun tan(self: f64): f64
  ret f64_tan(self)
end fun

// Arcsine in radians, in [-pi/2, pi/2]. Nan outside [-1, 1].
fun asin(self: f64): f64
  ret f64_asin(self)
end fun

// Arccosine in radians, in [0, pi]. Nan outside [-1, 1].
fun acos(self: f64): f64
  ret f64_acos(self)
end fun

// Arctangent in radians, in [-pi/2, pi/2].
fun atan(self: f64): f64
  ret f64_atan(self)
end fun

// Hyperbolic sine.
fun sinh(self: f64): f64
  ret f64_sinh(self)
end fun

// Hyperbolic cosine.
fun cosh(self: f64): f64
  ret f64_cosh(self)
end fun

// Hyperbolic tangent.
fun tanh(self: f64): f64
  ret f64_tanh(self)
end fun

// e raised to the power of self.
fun exp(self: f64): f64
  ret f64_exp(self)
end fun

// 2 raised to the power of self.
fun exp2(self: f64): f64
  ret f64_exp2(self)
end fun

// Natural logarithm. Nan for a negative self, -inf for zero.
fun ln(self: f64): f64
  ret f64_ln(self)
end fun

// Base 2 logarithm.
fun log2(self: f64): f64
  ret f64_log2(self)
end fun

// Base 10 logarithm.
fun log10(self: f64): f64
  ret f64_log10(self)
end fun

// Cube root, defined for negative values as well.
fun cbrt(self: f64): f64
  ret f64_cbrt(self)
end fun

// Arctangent of self/other in radians, using the signs of both to place the result in the right quadrant.
fun atan2(self: f64, other: f64): f64
  ret f64_atan2(self, other)
end fun

// Logarithm of self in the given base.
fun log(self: f64, base: f64): f64
  ret f64_log(self, base)
end fun

// self raised to the power of exp.
fun pow(self: f64, exp: f64): f64
  ret f64_pow(self, exp)
end fun

// The length of the hypotenuse, without the overflow that squaring both sides invites.
fun hypot(self: f64, other: f64): f64
  ret f64_hypot(self, other)
end fun

// The remainder of self/other, taking the sign of self. This is what fmod computes, not a modulo.
fun rem(self: f64, other: f64): f64
  ret f64_rem(self, other)
end fun

// Constants.

// The ratio of a circle's circumference to its diameter.
fun pi(): f64
  ret 3.14159265358979311599796346854418516159057617187500
end fun

// The base of the natural logarithm.
fun e(): f64
  ret 2.71828182845904509079559829842764884233474731445312
end fun

// Two pi: one full turn in radians.
fun tau(): f64
  ret 6.28318530717958623199592693708837032318115234375000
end fun

// Angle conversion.

fun to_degrees(self: f64): f64
  ret self * (180.0 / pi())
end fun

fun to_radians(self: f64): f64
  ret self * (pi() / 180.0)
end fun

// Conversion from the integers.
//
// The language does not widen between integers and floats, so this is the
// way across. None when the value is too large for a f64 to hold; a value
// that fits but has more digits than the format carries is rounded.
fun from_int(ref n: int): ?f64
  ret f64_from_int(ref n)
end fun

// Conversion from the other width.
//
// The language does not widen between the float widths on its own - see
// botspec section 10 - so this is the way up. `f32.from_f64` is the way down.

// Every f32 is an f64 exactly, so this loses nothing and cannot fail. It is
// the way back from a value that took the narrower type by default.
fun from_f32(x: f32): f64
  ret icall f32_to_f64(x)
end fun
