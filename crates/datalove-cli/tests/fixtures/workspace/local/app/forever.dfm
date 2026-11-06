// A function whose one call is to itself, with its own parameters.
fun forever(n: u32): u32
  ret forever(n)
end fun
