// Functions over any fixed-width integer.
//
// A type parameter bounded to `fixedint` is one of the ten: `u8`, `i8`, `u16`,
// `i16`, `u32`, `i32`, `u64`, `i64`, `index` or `offset`. In exchange for
// saying so the body may do what all ten have in common, which is the
// comparisons and the checked and optional arithmetic. Bare `+` is not among
// them, because a fixed-width integer does not have one either.
//
// Which of the ten it turned out to be is read off the descriptor that travels
// with the value, the same way a collection's elements are, so there is one of
// each of these rather than one per width. Where the per-width modules differ
// -- `abs` is not a thing an unsigned integer does, and `count_ones` returns a
// width the caller has to name -- they keep their own.
//
// Nothing here can write a literal, because a literal has to be written at
// some type and the type is what nobody has picked yet. So there is no
// `zero`, no `min_value` and nothing built on one; those stay per-width.

require rider std
import std.list_len
import std.list_get

// The smaller of two, or self when they are equal.
fun min<T>(self: T, other: T): T with { T is fixedint, }
  if self <= other
    let unwanted = other
    ret self
  else
    let unwanted = self
    ret other
  end if
end fun

// The larger of two, or self when they are equal.
fun max<T>(self: T, other: T): T with { T is fixedint, }
  if self >= other
    let unwanted = other
    ret self
  else
    let unwanted = self
    ret other
  end if
end fun

// Self brought inside the range, which is min_val below and max_val above.
fun clamp<T>(self: T, min_val: T, max_val: T): T with { T is fixedint, }
  if self .< min_val
    let unwanted_self = self
    let unwanted_max = max_val
    ret min_val
  else
    if self .> max_val
      let unwanted_self = self
      let unwanted_min = min_val
      ret max_val
    else
      let unwanted_min = min_val
      let unwanted_max = max_val
      ret self
    end if
  end if
end fun

// The distance between two, which has no sign.
//
// Optional because the answer is taken at the operands' own type, and a
// signed one has no room for the whole span: the distance from its most
// negative value to its largest is one past the top of the range. Every
// unsigned pair fits, and so does every signed pair that is not that wide.
fun abs_diff<T>(self: T, other: T): ?T with { T is fixedint, }
  if self >= other
    ret some (self -? other)
  else
    ret some (other -? self)
  end if
end fun

// The two in order, smaller first.
fun sorted_pair<T>(self: T, other: T): (T, T) with { T is fixedint, }
  if self <= other
    ret (self, other)
  else
    ret (other, self)
  end if
end fun

// Whether self lies in the range, which is min_val below and max_val above.
fun in_range<T>(self: T, min_val: T, max_val: T): bool with { T is fixedint, }
  let inside = self >= min_val and self <= max_val
  let unwanted_self = self
  let unwanted_min = min_val
  let unwanted_max = max_val
  ret inside
end fun

// The smallest element, or none when there are none.
fun least<T>(ref self: [T]): ?T with { T is fixedint, }
  var best: ?T = none
  let n = list_len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if list_get(ref self, i) |elem|
      if best |held|
        if elem .< held
          let unwanted = held
          set best = some elem
        else
          let unwanted = elem
          set best = some held
        end if
      else
        set best = some elem
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret best
end fun

// The largest element, or none when there are none.
fun greatest<T>(ref self: [T]): ?T with { T is fixedint, }
  var best: ?T = none
  let n = list_len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if list_get(ref self, i) |elem|
      if best |held|
        if elem .> held
          let unwanted = held
          set best = some elem
        else
          let unwanted = elem
          set best = some held
        end if
      else
        set best = some elem
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret best
end fun

// Every element added together, or none when there are none.
//
// The running total starts at the first element rather than at zero, because
// a zero cannot be written at a type nobody has named. None also comes back
// when the total runs past the width, which is what `+?` reports.
fun sum<T>(ref self: [T]): ?T with { T is fixedint, }
  var total: ?T = none
  let n = list_len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if list_get(ref self, i) |elem|
      if total |running|
        let combined = running +? elem
        let unwanted_running = running
        let unwanted_elem = elem
        set total = some combined
      else
        set total = some elem
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret total
end fun

// Every element multiplied together, or none when there are none.
//
// None also comes back when the product runs past the width, as `sum`'s does.
fun product<T>(ref self: [T]): ?T with { T is fixedint, }
  var running: ?T = none
  let n = list_len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if list_get(ref self, i) |elem|
      if running |so_far|
        let combined = so_far *? elem
        let unwanted_so_far = so_far
        let unwanted_elem = elem
        set running = some combined
      else
        set running = some elem
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret running
end fun

// Whether the elements are in non-decreasing order.
//
// An empty list and a list of one are sorted, having nothing out of order.
fun is_sorted<T>(ref self: [T]): bool with { T is fixedint, }
  let n = list_len(ref self)
  var previous: ?T = none
  // Every element is looked at even once a pair is found out of order,
  // because leaving early would move the element held here in one branch and
  // not the other, which is refused.
  var ordered = true
  var i: index = : index / 0
  loop while i .< n
    if list_get(ref self, i) |elem|
      if previous |before|
        let out_of_order = before .> elem
        let unwanted_before = before
        if out_of_order
          set ordered = false
        end if
        set previous = some elem
      else
        set previous = some elem
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  if previous |last|
    let unwanted_last = last
  end if
  ret ordered
end fun
