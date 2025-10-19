fun min_value(): u32
  ret 0
end fun

fun max_value(): u32
  ret 4294967295
end fun

fun bits(): u32
  ret 32
end fun

fun count_ones(self: u32): u32
  // todo
  ret 0
end fun

fun count_zeros(self: u32): u32
  ret sub_saturating(bits(), count_ones(self))
end fun

fun leading_zeros(self: u32): u32
  // todo
  ret 0
end fun

fun trailing_zeros(self: u32): u32
  // todo
  ret 0
end fun

fun leading_ones(self: u32): u32
  // todo
  ret 0
end fun

fun trailing_ones(self: u32): u32
  // todo
  ret 0
end fun

fun cast_signed(self: u32): i32
  // todo
  ret 0
end fun

fun swap_bytes(self: u32): u32
  // todo
  ret 0
end fun

fun reverse_bits(self: u32): u32
  // todo
  ret 0
end fun

fun from_be(other: u32): u32
  // todo
  ret 0
end fun

fun from_le(other: u32): u32
  // todo
  ret 0
end fun

fun to_be(self: u32): u32
  // todo
  ret 0
end fun

fun to_le(self: u32): u32
  // todo
  ret 0
end fun

fun add_checked(self: u32, other: u32): ?u32
  // todo
  ret @none // fixme @-required
end fun

fun add_checked_signed(self: u32, other: i32): ?u32
  // todo
  ret @none // fixme @-required
end fun

fun sub_checked(self: u32, other: u32): ?u32
  // todo
  ret @none // fixme @-required
end fun

fun sub_checked_signed(self: u32, other: i32): ?u32
  // todo
  ret @none // fixme @-required
end fun

fun mul_checked(self: u32, other: u32): ?u32
  // todo
  ret @none // fixme @-required
end fun

fun div_checked(self: u32, other: u32): ?u32
  // todo
  ret @none // fixme @-required
end fun

fun add_saturating(self: u32, other: u32): u32
  // todo
  ret 0
end fun

fun sub_saturating(self: u32, other: u32): u32
  // todo
  ret 0
end fun

fun mul_saturating(self: u32, other: u32): u32
  // todo
  ret 0
end fun

fun div_saturating(self: u32, other: u32): ?u32
  // todo
  ret @none // fixme @-required
end fun

fun add_wrapping(self: u32, other: u32): u32
  // todo
  ret 0
end fun

fun sub_wrapping(self: u32, other: u32): u32
  // todo
  ret 0
end fun

fun mul_wrapping(self: u32, other: u32): u32
  // todo
  ret 0
end fun

fun div_wrapping(self: u32, other: u32): ?u32
  // todo
  ret @none // fixme @-required
end fun

fun neg_checked(self: u32): ?u32
  if self == 0
    ret 0
  else
    ret @none // fixme @
  end if
end fun

fun shift_left(self: u32, other: u32): ?u32
  ret @none // fixme @
end fun

fun shift_right(self: u32, other: u32): ?u32
  ret @none // fixme @
end fun

fun shift_left_saturating(self: u32, other: u32): u32
  ret 0
end fun

fun shift_right_saturating(self: u32, other: u32): u32
  ret 0
end fun

fun shift_left_wrapping(self: u32, other: u32): u32
  ret 0
end fun

fun shift_right_wrapping(self: u32, other: u32): u32
  ret 0
end fun

fun rotate_left(self: u32, n: u32): u32
  // todo
  ret 0
end fun

fun rotate_right(self: u32, n: u32): u32
  // todo
  ret 0
end fun

fun bitnot(self: u32): u32
  // todo
  ret 0
end fun

fun bitand(self: u32, n: u32): u32
  // todo
  ret 0
end fun

fun bitor(self: u32, n: u32): u32
  // todo
  ret 0
end fun

fun bitxor(self: u32, n: u32): u32
  // todo
  ret 0
end fun
