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
  ret bits() -| count_ones(self)
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

fun rotate_left(self: u32, n: u32): u32
  // todo
  ret 0
end fun

fun rotate_right(self: u32, n: u32): u32
  // todo
  ret 0
end fun

fun shift_left(self: u32, n: u32): u32
  // todo
  ret 0
end fun

fun shift_right_logical(self: u32, n: u32): u32
  // todo
  ret 0
end fun

fun shift_right_arithmetic(self: u32, n: u32): u32
  // todo
  ret 0
end fun

fun swap_bytes(self: u32): u32
  // todo
  ret 0
end fun
