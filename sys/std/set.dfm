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
import std.list_pop

// The number of elements.
fun len<T>(ref self: #{T}): index with { T is ord, }
  ret set_len(ref self)
end fun

// True if there are no elements.
fun is_empty<T>(ref self: #{T}): bool with { T is ord, }
  ret set_len(ref self) == (: index / 0)
end fun

// True if the element is present.
fun contains<T>(ref self: #{T}, ref elem: T): bool with { T is ord, }
  ret set_contains(ref self, ref elem)
end fun

// Add an element. False if it was already present.
fun insert<T>(mut self: #{T}, elem: T): bool with { T is ord, }
  ret set_insert(mut self, elem)
end fun

// Remove an element. False if it was not present.
fun remove<T>(mut self: #{T}, ref elem: T): bool with { T is ord, }
  ret set_remove(mut self, ref elem)
end fun

// Drop every element.
fun clear<T>(mut self: #{T}) with { T is ord, }
  call set_clear(mut self)
end fun

// The element at a position in sort order, or none past the end.
//
// A set is a tree, so this walks to the position rather than jumping to it, and
// reaching every element in turn costs more than it does for a list. It is here
// so that anything written as a loop over `len` and an index -- which is how
// everything over a collection is written -- can be written over a set too.
fun get<T>(ref self: #{T}, i: index): ?T with { T is ord, }
  ret set_get(ref self, i)
end fun

// Every element, in sort order.
fun to_list<T>(ref self: #{T}): [T] with { T is ord, }
  var built: [T] = []
  let n = len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if get(ref self, i) |elem|
      call list_push(mut built, elem)
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret built
end fun

// Every element of either.
fun union_with<T>(ref self: #{T}, ref other: #{T}): #{T} with { T is ord, }
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
// The element is disposed of on both paths, because a value moved on one path
// has to be moved on every one: drop points here are static, so the checker
// will not take a value whose fate depends on the branch. The element that is
// not wanted goes into a binding that ends with the branch.
fun intersection_with<T>(ref self: #{T}, ref other: #{T}): #{T} with { T is ord, }
  var built: #{T} = #{}
  let n = len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if get(ref self, i) |elem|
      if contains(ref other, ref elem)
        let added = insert(mut built, elem)
      else
        let unwanted = elem
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret built
end fun

// Every element of self that other does not have.
fun difference_with<T>(ref self: #{T}, ref other: #{T}): #{T} with { T is ord, }
  var built: #{T} = #{}
  let n = len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if get(ref self, i) |elem|
      if contains(ref other, ref elem)
        let unwanted = elem
      else
        let added = insert(mut built, elem)
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret built
end fun

// True if every element of self is in other.
//
// The answer is accumulated rather than returned as soon as it is known,
// because returning out of `if get(...) |elem|` disposes of `elem` on that path
// and not the other, and a value's fate has to be the same on every one.
fun is_subset_of<T>(ref self: #{T}, ref other: #{T}): bool with { T is ord, }
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
