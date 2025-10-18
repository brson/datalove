fun min_value(): u32
  ret 0
end fun

fun max_value(): u32
  ret 4294967295 
end fun

fun bits(): u32
  ret 32
end fun



fun add(x: u32, y: u32): u32
  ret x + y
end fun

fun negate(x: u32): u32
  ret 0 - x
end fun

fun double(x: u32): u32
  ret x + x
end fun
