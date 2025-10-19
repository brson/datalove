fun is_some(self: ?u32): bool
  if self |value|
    ret @true
  else
    ret @false
  end if
end fun

fun is_none(self: ?u32): bool
  if self |value|
    ret @false
  else
    ret @true
  end if
end fun

fun unwrap_or(self: ?u32, default: u32): u32
  if self |value|
    ret value
  else
    ret default
  end if
end fun

fun or_option(self: ?u32, other: ?u32): ?u32
  if self |value|
    ret self
  else
    ret other
  end if
end fun

fun xor_option(self: ?u32, other: ?u32): ?u32
  if self |self_value|
    if other |other_value|
      ret @none
    else
      ret self
    end if
  else
    if other |other_value|
      ret other
    else
      ret @none
    end if
  end if
end fun

fun and_option(self: ?u32, other: ?u32): ?u32
  if self |value|
    ret other
  else
    ret @none
  end if
end fun

fun unwrap_or_zero(self: ?u32): u32
  if self |value|
    ret value
  else
    ret 0
  end if
end fun
