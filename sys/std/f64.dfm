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
      ret exponent ≡ zero
    end if
  else
    ret false
  end if
end fun

fun is_sign_positive(self: f64): bool
  let bits = icall f64_to_bits(self)
  let sign_bit = (: u64 / 0x8000000000000000)
  let zero = : u64 / 0
  ret icall bitand_u64(bits, sign_bit) ≡ zero
end fun

fun is_sign_negative(self: f64): bool
  let bits = icall f64_to_bits(self)
  let sign_bit = (: u64 / 0x8000000000000000)
  let zero = : u64 / 0
  ret icall bitand_u64(bits, sign_bit) ≢ zero
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
  ret self ≡ (: f64 / 0.0)
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

  if icall bitand_u64(a_bits, sign_bit) ≢ zero
    let mask = (: u64 / 0x7FFFFFFFFFFFFFFF)
    set a_ord = icall u64_to_i64(icall bitxor_u64(a_bits, mask))
  end if

  if icall bitand_u64(b_bits, sign_bit) ≢ zero
    let mask = (: u64 / 0x7FFFFFFFFFFFFFFF)
    set b_ord = icall u64_to_i64(icall bitxor_u64(b_bits, mask))
  end if

  if a_ord < b_ord
    ret (: i32 / -1)
  else
    if a_ord > b_ord
      ret : i32 / 1
    else
      ret : i32 / 0
    end if
  end if
end fun
