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
fun set<T>(mut self: [T], i: index, elem: T): bool
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
