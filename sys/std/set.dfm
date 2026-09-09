// Set module.
//
// Generic over the element type. A set is not converted when it is passed to
// one of these: it is borrowed, and the descriptor saying what it holds comes
// from the call site. An element being looked up is borrowed, so it arrives as
// itself; one being inserted is given away, so it is carried as a `data`.

require rider std
import std.set_len
import std.set_clear
import std.set_contains
import std.set_insert
import std.set_remove
import std.set_get
import std.list_push

// The number of elements.
fun len<T>(ref self: #{T}): index
  ret set_len(ref self)
end fun

// True if there are no elements.
fun is_empty<T>(ref self: #{T}): bool
  ret set_len(ref self) == (: index / 0)
end fun

// True if the element is present.
fun contains<T>(ref self: #{T}, ref elem: T): bool
  ret set_contains(ref self, ref elem)
end fun

// Add an element. False if it was already present.
fun insert<T>(mut self: #{T}, elem: T): bool
  ret set_insert(mut self, elem)
end fun

// Remove an element. False if it was not present.
fun remove<T>(mut self: #{T}, ref elem: T): bool
  ret set_remove(mut self, ref elem)
end fun

// Drop every element.
fun clear<T>(mut self: #{T})
  set_clear(mut self)
end fun

// The element at a position in sort order, or none past the end.
//
// A set is a tree, so this walks to the position rather than jumping to it, and
// reaching every element in turn costs more than it does for a list. It is here
// so that anything written as a loop over `len` and an index -- which is how
// everything over a collection is written -- can be written over a set too.
fun get<T>(ref self: #{T}, i: index): ?T
  ret set_get(ref self, i)
end fun

// Every element, in sort order.
fun to_list<T>(ref self: #{T}): [T]
  var built: [T] = []
  let n = len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if get(ref self, i) |elem|
      list_push(mut built, elem)
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret built
end fun

// Every element of either.
fun union_with<T>(ref self: #{T}, ref other: #{T}): #{T}
  var built: #{T} = #{}
  let n = len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if get(ref self, i) |elem|
      let added = insert(mut built, elem)
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  let m = len(ref other)
  var j: index = : index / 0
  loop while j .< m
    if get(ref other, j) |elem|
      let added = insert(mut built, elem)
    end if
    set j = icall add_wrapping_index(j, : index / 1)
  end loop
  ret built
end fun

// Every element of both.
//
// The element is cloned into the result rather than moved, because a linear
// value has to go somewhere on every path and there is no way to say "drop
// this one". Moving it only when it is wanted leaves the other branch holding
// it, which the ownership checker refuses.
fun intersection_with<T>(ref self: #{T}, ref other: #{T}): #{T}
  var built: #{T} = #{}
  let n = len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if get(ref self, i) |elem|
      if contains(ref other, ref elem)
        let added = insert(mut built, elem@)
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret built
end fun

// Every element of self that other does not have.
fun difference_with<T>(ref self: #{T}, ref other: #{T}): #{T}
  var built: #{T} = #{}
  let n = len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if get(ref self, i) |elem|
      if contains(ref other, ref elem)
      else
        let added = insert(mut built, elem@)
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret built
end fun

// True if every element of self is in other.
//
// The answer is accumulated rather than returned as soon as it is known,
// because returning out of `if get(...) |elem|` leaves that branch having
// disposed of `elem` and the other not, which the ownership checker refuses.
fun is_subset_of<T>(ref self: #{T}, ref other: #{T}): bool
  var all_present: bool = true
  let n = len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if get(ref self, i) |elem|
      if contains(ref other, ref elem)
      else
        set all_present = false
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret all_present
end fun
