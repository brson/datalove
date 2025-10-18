fun add(x: @u32, y: @u32): @u32
  ret x + y
end fun

fun negate(x: @u32): @u32
  ret @0 - x
end fun

fun double(x: @u32): @u32
  ret x + x
end fun
