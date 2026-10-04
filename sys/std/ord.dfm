// Comparison over any type.
//
// Every value in this language has a total order: the one sets and maps keep
// their keys in, which walks a value structurally and compares it against
// another of its type. `T is ord` is what asks for it, and it admits every
// type there is -- what it rules out is a type with no ordering, of which
// there are none today.
//
// This is not the same relation as the operators. They behave the standard
// way for floats, so NaN compares false against everything and the two zeros
// are equal, including inside an option or a tuple. The order here is IEEE
// 754-2008 `totalOrder`, under which NaN has a place and the two zeros do not.
// The two differ only over those, and only for floats.
//
// It is also more than the operators reach. `.<` is for numbers only, and `==`
// is refused for a list, a set, a map, a table, a tensor, a result and anything
// holding a type parameter; the functions here are not.

require rider std
require module sys/std/index

import index.shift_right_wrapping
import std.ord_compare
import std.list_len
import std.list_get
import std.list_push
import std.list_insert

// Negative when self sorts before other, zero when they sort together,
// positive when it sorts after.
fun compare<T>(ref self: T, ref other: T): i8 with { T is ord, }
  let a: T = self@
  let b: T = other@
  ret ord_compare(a, b)
end fun

fun equal<T>(ref self: T, ref other: T): bool with { T is ord, }
  let a: T = self@
  let b: T = other@
  ret ord_compare(a, b) == : i8 / 0
end fun

fun not_equal<T>(ref self: T, ref other: T): bool with { T is ord, }
  let a: T = self@
  let b: T = other@
  ret ord_compare(a, b) != : i8 / 0
end fun

fun less<T>(ref self: T, ref other: T): bool with { T is ord, }
  let a: T = self@
  let b: T = other@
  ret ord_compare(a, b) .< : i8 / 0
end fun

fun less_or_equal<T>(ref self: T, ref other: T): bool with { T is ord, }
  let a: T = self@
  let b: T = other@
  ret ord_compare(a, b) <= : i8 / 0
end fun

fun greater<T>(ref self: T, ref other: T): bool with { T is ord, }
  let a: T = self@
  let b: T = other@
  ret ord_compare(a, b) .> : i8 / 0
end fun

fun greater_or_equal<T>(ref self: T, ref other: T): bool with { T is ord, }
  let a: T = self@
  let b: T = other@
  ret ord_compare(a, b) >= : i8 / 0
end fun

// The smaller of two, or self when they sort together.
fun min<T>(self: T, other: T): T with { T is ord, }
  if less_or_equal(ref self, ref other)
    let unwanted = other
    ret self
  else
    let unwanted = self
    ret other
  end if
end fun

// The larger of two, or self when they sort together.
fun max<T>(self: T, other: T): T with { T is ord, }
  if greater_or_equal(ref self, ref other)
    let unwanted = other
    ret self
  else
    let unwanted = self
    ret other
  end if
end fun

// Self brought inside the range, which is low below and high above.
fun clamp<T>(self: T, low: T, high: T): T with { T is ord, }
  if less(ref self, ref low)
    let unwanted_self = self
    let unwanted_high = high
    ret low
  else
    if greater(ref self, ref high)
      let unwanted_self = self
      let unwanted_low = low
      ret high
    else
      let unwanted_low = low
      let unwanted_high = high
      ret self
    end if
  end if
end fun

// The two in order, the one that sorts first leading.
fun sorted_pair<T>(self: T, other: T): (T, T) with { T is ord, }
  if less_or_equal(ref self, ref other)
    ret (self, other)
  else
    ret (other, self)
  end if
end fun

// Whether the list holds an element that sorts together with this one.
fun contains<T>(ref self: [T], ref elem: T): bool with { T is ord, }
  var found = false
  let n = list_len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if list_get(ref self, i) |held|
      let same = equal(ref held, ref elem)
      let unwanted = held
      if same
        set found = true
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret found
end fun

// Where the first such element is, or none when there is none.
fun index_of<T>(ref self: [T], ref elem: T): ?index with { T is ord, }
  var at: ?index = none
  let n = list_len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if list_get(ref self, i) |held|
      let same = equal(ref held, ref elem)
      let unwanted = held
      if same
        if at |already|
          set at = some already
        else
          set at = some i
        end if
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret at
end fun

// How many elements sort together with this one.
fun count_of<T>(ref self: [T], ref elem: T): index with { T is ord, }
  var seen: index = : index / 0
  let n = list_len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if list_get(ref self, i) |held|
      let same = equal(ref held, ref elem)
      let unwanted = held
      if same
        set seen = icall add_wrapping_index(seen, : index / 1)
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret seen
end fun

// The element that sorts first, or none when there are none.
fun least<T>(ref self: [T]): ?T with { T is ord, }
  var best: ?T = none
  let n = list_len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if list_get(ref self, i) |elem|
      if best |held|
        set best = some min(held, elem)
      else
        set best = some elem
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret best
end fun

// The element that sorts last, or none when there are none.
fun greatest<T>(ref self: [T]): ?T with { T is ord, }
  var best: ?T = none
  let n = list_len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if list_get(ref self, i) |elem|
      if best |held|
        set best = some max(held, elem)
      else
        set best = some elem
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret best
end fun

// Whether no element sorts before the one in front of it.
//
// An empty list and a list of one are sorted, having nothing out of order.
fun is_sorted<T>(ref self: [T]): bool with { T is ord, }
  var ordered = true
  var previous: ?T = none
  let n = list_len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if list_get(ref self, i) |elem|
      if previous |before|
        let out_of_order = greater(ref before, ref elem)
        let unwanted_before = before
        if out_of_order
          set ordered = false
        end if
      end if
      set previous = some elem
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  if previous |last|
    let unwanted_last = last
  end if
  ret ordered
end fun

// A new list holding the same elements in order.
//
// Insertion sort, which keeps equal elements in the order they came in and
// asks nothing of the element but the comparison. The list is walked once per
// element placed.
fun sorted<T>(ref self: [T]): [T] with { T is ord, }
  var out: [T] = []
  let n = list_len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if list_get(ref self, i) |elem|
      // Where the first element that sorts after this one is, which is where
      // this one goes. Past the end when there is none.
      var at: index = list_len(ref out)
      var j: index = : index / 0
      let placed = list_len(ref out)
      loop while j .< placed
        if list_get(ref out, j) |existing|
          let after = greater(ref existing, ref elem)
          let unwanted_existing = existing
          if after
            set at = j
            break
          end if
        end if
        set j = icall add_wrapping_index(j, : index / 1)
      end loop
      let inserted = list_insert(mut out, at, elem)
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret out
end fun

// Where an element that sorts together with this one is, or none.
//
// The list has to be sorted already; on one that is not, the answer says
// nothing. Halves the range each round rather than walking it, which is what
// having the order is worth.
fun binary_search<T>(ref self: [T], ref elem: T): ?index with { T is ord, }
  var found: ?index = none
  var low: index = : index / 0
  var high: index = list_len(ref self)
  loop while low .< high
    let span = icall sub_wrapping_index(high, low)
    let mid = icall add_wrapping_index(low, shift_right_wrapping(span, : u32 / 1))
    if list_get(ref self, mid) |held|
      let probe: T = held@
      let wanted: T = elem@
      let order = ord_compare(probe, wanted)
      let unwanted_held = held
      if order == : i8 / 0
        set found = some mid
        break
      end if
      if order .< : i8 / 0
        set low = icall add_wrapping_index(mid, : index / 1)
      else
        set high = mid
      end if
    end if
  end loop
  ret found
end fun

// A new list with elements that sort together left in once.
//
// The list has to be sorted already, since only neighbours are compared.
fun deduped<T>(ref self: [T]): [T] with { T is ord, }
  var out: [T] = []
  var previous: ?T = none
  let n = list_len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if list_get(ref self, i) |elem|
      // What came before is compared and then let go; this element takes its
      // place either way, so that a run of three or more is one element and
      // not two.
      var keep = true
      if previous |before|
        let same = equal(ref before, ref elem)
        let unwanted_before = before
        if same
          set keep = false
        end if
      end if
      if keep
        call list_push(mut out, elem@)
      end if
      set previous = some elem
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  if previous |last|
    let unwanted_last = last
  end if
  ret out
end fun
