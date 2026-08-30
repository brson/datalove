// List module.
//
// The natives underneath are generic over the element type: each parameter
// arrives as a pointer and a descriptor, so one implementation reads the
// element type off the list it was handed and serves every element type.
//
// The wrappers here are per type. A generic wrapper would be the natural
// spelling, and in the interpreter it works, because a value there carries its
// descriptor and a borrowed parameter therefore reaches the callee describing
// what it really is. The compiled backends pick the descriptor for an argument
// from its static type when they emit the call, and a generic function's
// static type says `data` where the parameter stood, so they would describe a
// list of u32 as a list of data. Until a generic function carries descriptors
// for its type parameters, the call site has to be the place that knows.

require rider std
import std.list_len
import std.list_get
import std.list_push
import std.list_pop

// The number of elements.
fun len_u32(ref self: [u32]): index
  ret list_len(ref self)
end fun

fun len_string(ref self: [string]): index
  ret list_len(ref self)
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
