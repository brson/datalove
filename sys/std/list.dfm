// List module.
//
// These are generic over the element type. A list is not converted when it is
// passed to one: a borrowed parameter goes across as it stands, and the
// descriptor that says what it holds comes from the call site, which is the
// place that knows. So there is one of each of these however many element
// types go through them, and passing a list to one costs nothing.
//
// An element is a different matter. A generic function has no slot that fits a
// value of a type it does not know, so an element crossing that boundary is
// carried as a `data`, which fits any value and carries what is needed to
// clone and drop it. The descriptors are what tell the implementation which of
// the two it is looking at.

require rider std
require module sys/std/option

import option.zip_option
import std.list_len
import std.list_get
import std.list_push
import std.list_pop
import std.list_clear
import std.list_set
import std.list_insert
import std.list_remove
import std.list_reserve

// The number of elements.
fun len<T>(ref self: [T]): index
  ret list_len(ref self)
end fun

// True if there are no elements.
fun is_empty<T>(ref self: [T]): bool
  ret list_len(ref self) == (: index / 0)
end fun

// The element at an index, or none if the index is past the end.
fun get<T>(ref self: [T], i: index): ?T
  ret list_get(ref self, i)
end fun

// Append an element.
fun push<T>(mut self: [T], elem: T)
  list_push(mut self, elem)
end fun

// The first element, or none if there are none.
fun first<T>(ref self: [T]): ?T
  ret list_get(ref self, : index / 0)
end fun

// The last element, or none if there are none. The subtraction carries the
// empty case: `-?` gives up on underflow, which returns none from here.
fun last<T>(ref self: [T]): ?T
  ret list_get(ref self, list_len(ref self) -? (: index / 1))
end fun

// Replace the element at an index. False if the index is past the end.
fun replace<T>(mut self: [T], i: index, elem: T): bool
  ret list_set(mut self, i, elem)
end fun

// Insert an element, shifting the rest right. The index may be the length,
// which appends. False if it is past that.
fun insert<T>(mut self: [T], i: index, elem: T): bool
  ret list_insert(mut self, i, elem)
end fun

// Take the element at an index out, shifting the rest down.
fun remove<T>(mut self: [T], i: index): ?T
  ret list_remove(mut self, i)
end fun

// Exchange the elements at two indices. False if either is past the end.
//
// This is written here rather than in the rider because it needs nothing the
// language cannot say about a `T`: the two elements are taken out and put
// back, and are only ever moved.
fun swap<T>(mut self: [T], i: index, j: index): bool
  if i == j
    ret i .< len(ref self)
  end if
  var lo = i
  var hi = j
  if i .> j
    set lo = j
    set hi = i
  end if
  // The higher one comes out first so that the lower index still means what
  // it did.
  if remove(mut self, hi) |high_elem|
    if remove(mut self, lo) |low_elem|
      let put_low = insert(mut self, lo, high_elem)
      let put_high = insert(mut self, hi, low_elem)
      ret put_low and put_high
    else
      // `lo` is below `hi`, which was in bounds, so the list is still long
      // enough and this does not happen. The element has to go somewhere
      // regardless, and where it came from is the only honest place.
      let restored = insert(mut self, hi, high_elem)
      ret false
    end if
  end if
  ret false
end fun

// A list holding just one element.
fun of<T>(elem: T): [T]
  var built: [T] = []
  push(mut built, elem)
  ret built
end fun

// A list of n clones of one element.
fun repeated<T>(ref elem: T, n: index): [T]
  var built: [T] = []
  var i: index = : index / 0
  loop while i .< n
    push(mut built, elem@)
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret built
end fun

// The first n elements, or all of them if there are fewer.
fun take<T>(ref self: [T], n: index): [T]
  var built: [T] = []
  var i: index = : index / 0
  loop while i .< n
    if get(ref self, i) |elem|
      push(mut built, elem)
    else
      break
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret built
end fun

// Everything after the first n elements.
fun skip<T>(ref self: [T], n: index): [T]
  var built: [T] = []
  let total = len(ref self)
  var i: index = n
  loop while i .< total
    if get(ref self, i) |elem|
      push(mut built, elem)
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret built
end fun

// Move every element of another list onto the end of this one, in order,
// leaving the other empty.
//
// `concat` builds a third list and leaves both alone, which costs a clone of
// every element of both. This costs none: it takes the other list by value and
// the elements are only ever moved.
//
// It goes by the back twice because that is the only end a list gives an
// element up from. The first pass reverses and the second puts it back.
fun extend<T>(mut self: [T], other: [T])
  var src: [T] = other
  var back: [T] = []
  loop
    if pop(mut src) |elem|
      push(mut back, elem)
    else
      break
    end if
  end loop
  loop
    if pop(mut back) |elem|
      push(mut self, elem)
    else
      break
    end if
  end loop
end fun

// One list holding every element of every inner list, in order.
fun flattened<T>(ref self: [[T]]): [T]
  var built: [T] = []
  var i: index = : index / 0
  let n = len(ref self)
  loop while i .< n
    if get(ref self, i) |inner|
      extend(mut built, inner)
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret built
end fun

// Pairs of elements at the same position, stopping at the shorter list.
//
// The elements are cloned, since neither list gives anything up. The pairing
// is `option.zip_option`, which is where "both or neither" is already written.
fun zip<A, B>(ref self: [A], ref other: [B]): [(A, B)]
  var built: [(A, B)] = []
  let n = len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if zip_option(get(ref self, i), get(ref other, i)) |pair|
      push(mut built, pair)
    else
      break
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret built
end fun

// Drop everything past the first n elements.
//
// The popped element is bound and not used, which is how it is let go of; the
// loop ends because each pop shortens the list.
fun truncate<T>(mut self: [T], n: index)
  loop while n .< len(ref self)
    let gone = pop(mut self)
  end loop
end fun

// A new list with the elements in the opposite order.
//
// This and `concat` build a list whose element type this function does not
// know. That works because the caller hands over a descriptor for it: the
// call site is the only place that knows what `T` was bound to.
fun reversed<T>(ref self: [T]): [T]
  var out: [T] = []
  var src: [T] = self@
  loop
    if pop(mut src) |elem|
      push(mut out, elem)
    else
      break
    end if
  end loop
  ret out
end fun

// A new list with other's elements after self's. Neither is disturbed.
fun concat<T>(ref self: [T], ref other: [T]): [T]
  var out: [T] = self@
  let n = len(ref other)
  var i: index = : index / 0
  loop while i .< n
    if get(ref other, i) |elem|
      push(mut out, elem)
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret out
end fun

// Make room for at least n more elements.
fun reserve<T>(mut self: [T], n: index)
  list_reserve(mut self, n)
end fun

// Drop every element, leaving an empty list.
fun clear<T>(mut self: [T])
  list_clear(mut self)
end fun

// Take the last element off, or none if there are none.
fun pop<T>(mut self: [T]): ?T
  ret list_pop(mut self)
end fun
