// Set module.
//
// Generic over the element type. A set or an element goes across as it
// stands, and the descriptor saying what it is comes from the call site.

require rider std
import std.set_len
import std.set_clear
import std.set_contains
import std.set_insert
import std.set_remove
import std.set_into_list
import std.list_pop

// The number of elements.
fun len<T>(ref self: #{T}): index with { T is ord, }
  inline
  ret set_len(ref self)
end fun

// True if there are no elements.
fun is_empty<T>(ref self: #{T}): bool with { T is ord, }
  inline
  ret set_len(ref self) == (: index / 0)
end fun

// True if the element is present.
fun contains<T>(ref self: #{T}, ref elem: T): bool with { T is ord, }
  inline
  ret set_contains(ref self, ref elem)
end fun

// Add an element. False if it was already present.
fun insert<T>(mut self: #{T}, elem: T): bool with { T is ord, }
  inline
  ret set_insert(mut self, elem)
end fun

// Remove an element. False if it was not present.
fun remove<T>(mut self: #{T}, ref elem: T): bool with { T is ord, }
  inline
  ret set_remove(mut self, ref elem)
end fun

// Drop every element.
fun clear<T>(mut self: #{T}) with { T is ord, }
  call set_clear(mut self)
end fun

// Every element, in sort order.
fun to_list<T>(ref self: #{T}): [T] with { T is ord, }
  var built: [T] = []
  call set_into_list(ref self, mut built)
  ret built
end fun

// Every element of either.
fun union_with<T>(ref self: #{T}, ref other: #{T}): #{T} with { T is ord, }
  var built: #{T} = #{}
  for elem in ref self
    let added = insert(mut built, elem@)
  end for
  for elem in ref other
    let added = insert(mut built, elem@)
  end for
  ret built
end fun

// Every element of both.
fun intersection_with<T>(ref self: #{T}, ref other: #{T}): #{T} with { T is ord, }
  var built: #{T} = #{}
  for elem in ref self
    if contains(ref other, ref elem)
      let added = insert(mut built, elem@)
    end if
  end for
  ret built
end fun

// Every element of self that other does not have.
fun difference_with<T>(ref self: #{T}, ref other: #{T}): #{T} with { T is ord, }
  var built: #{T} = #{}
  for elem in ref self
    if not contains(ref other, ref elem)
      let added = insert(mut built, elem@)
    end if
  end for
  ret built
end fun

// True if every element of self is in other.
fun is_subset_of<T>(ref self: #{T}, ref other: #{T}): bool with { T is ord, }
  for elem in ref self
    if not contains(ref other, ref elem)
      ret false
    end if
  end for
  ret true
end fun

// A set of the given elements, in sort order and without repeats.
//
// The elements are moved rather than cloned: the list is taken by value and
// emptied from the back.
fun from_list<T>(items: [T]): #{T} with { T is ord, }
  var out: #{T} = #{}
  var src: [T] = items
  loop
    if list_pop(mut src) |elem|
      let fresh = insert(mut out, elem)
    else
      break
    end if
  end loop
  ret out
end fun
