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
