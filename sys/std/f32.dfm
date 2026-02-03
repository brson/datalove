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
