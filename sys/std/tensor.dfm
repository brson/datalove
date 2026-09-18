// Tensor module.
//
// These are written at **rank 1**, which is the shape a list-like walk wants.
// The rank is part of a tensor's type rather than a parameter -- `[|T, 2|]` is
// a different type from `[|T, 1|]` and there is no way to write "a tensor of
// any rank" -- so a function over rank 2 would have to be a second one of each.
// `rank` is here so that a caller can at least ask.
//
// What a tensor has that a list does not is a shape: its length along axis 0
// lives there rather than in its header, which is why `len` is a native when a
// list's could have been. What it lacks is any way to grow: a tensor is
// created at a size and keeps it, so there is no `push`, no `pop` and no
// `insert`. `set` writes in place and `to_list` is how one is turned into
// something that can grow.
//
// Indexing a rank-1 tensor gives an element. Indexing a higher rank gives a
// *view* of the slab below it, which the language will not let a binding hold,
// so nothing here returns one.

require rider std
require module sys/std/list

import list.push
import std.tensor_len
import std.tensor_rank

// The number of elements along axis 0.
fun len<T>(ref self: [|T, 1|]): index
  ret tensor_len(ref self)
end fun

// How many axes the tensor has. Always 1 for the tensors these functions take,
// and here because a caller holding one of another rank has nothing else to
// ask.
fun rank<T>(ref self: [|T, 1|]): index
  ret tensor_rank(ref self)
end fun

// True if there are no elements.
fun is_empty<T>(ref self: [|T, 1|]): bool
  ret tensor_len(ref self) == (: index / 0)
end fun

// The element at an index, or none if the index is past the end.
fun get<T>(ref self: [|T, 1|], i: index): ?T
  ret some (self[i]?@)
end fun

// The first element, or none if there are none.
fun first<T>(ref self: [|T, 1|]): ?T
  ret get(ref self, : index / 0)
end fun

// The last element, or none if there are none. The subtraction carries the
// empty case: `-?` gives up on underflow, which returns none from here.
fun last<T>(ref self: [|T, 1|]): ?T
  ret get(ref self, tensor_len(ref self) -? (: index / 1))
end fun

// Replace the element at an index. False if the index is past the end.
//
// A tensor cannot grow, so this is the only way to put something in one.
fun set_at<T>(mut self: [|T, 1|], i: index, elem: T): ?bool
  set self[i]? = elem
  ret some true
end fun

// The elements as a list, in order.
fun to_list<T>(ref self: [|T, 1|]): [T]
  var built: [T] = []
  let n = tensor_len(ref self)
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
