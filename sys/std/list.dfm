// List module.
//
// These are generic over the element type. A list is not converted when it is
// passed to one: a borrowed parameter goes across as it stands, with a
// descriptor saying what it holds, and the implementation reads the element
// type from that. So there is one of each of these however many element types
// go through them, and passing a list to one costs nothing.
//
// What is generic here is exactly what can be: a parameter the callee only
// borrows. A parameter the callee takes ownership of has to be erased, because
// its slot needs a size, and a `data` is the one shape that fits any value. A
// return is the same problem seen from the other end, which is why `get` and
// `pop` are spelled out per type below rather than written once: the generic
// natives they call are already generic, but a generic wrapper would need a
// local of a type it does not know.

require rider std
import std.list_len
import std.list_get
import std.list_pop
import std.list_push

// The number of elements.
fun len<T>(ref self: [T]): index
  ret list_len(ref self)
end fun

// True if there are no elements.
fun is_empty<T>(ref self: [T]): bool
  ret list_len(ref self) == (: index / 0)
end fun

// The element at an index, or none if the index is past the end.
fun get_u32(ref self: [u32], i: index): ?u32
  ret list_get(ref self, i)
end fun

fun get_string(ref self: [string], i: index): ?string
  ret list_get(ref self, i)
end fun

// Append an element.
fun push_u32(mut self: [u32], elem: u32)
  list_push(mut self, elem)
end fun

fun push_string(mut self: [string], elem: string)
  list_push(mut self, elem)
end fun

// Take the last element off, or none if there are none.
fun pop_u32(mut self: [u32]): ?u32
  ret list_pop(mut self)
end fun

fun pop_string(mut self: [string]): ?string
  ret list_pop(mut self)
end fun
