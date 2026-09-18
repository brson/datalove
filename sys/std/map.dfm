// Map module.
//
// Generic over the key and value types. A map is not converted when it is
// passed to one of these: it is borrowed, and the descriptor saying what it
// holds comes from the call site. A key being looked up is borrowed too, so it
// arrives as itself. A value crossing the boundary has no type the function
// knows, so it is carried as a `data` and moved back out where the type is
// known.

require rider std
require module sys/std/option

import option.zip_option
import std.map_len
import std.map_clear
import std.map_contains_key
import std.map_get
import std.map_insert
import std.map_remove
import std.map_key_at
import std.map_value_at
import std.list_push
import std.list_pop

// The number of entries.
fun len<K, V>(ref self: %{K = V}): index with { K is ord, }
  ret map_len(ref self)
end fun

// True if there are no entries.
fun is_empty<K, V>(ref self: %{K = V}): bool with { K is ord, }
  ret map_len(ref self) == (: index / 0)
end fun

// True if the key is present.
fun contains_key<K, V>(ref self: %{K = V}, ref key: K): bool with { K is ord, }
  ret map_contains_key(ref self, ref key)
end fun

// The value for a key, or none.
fun get<K, V>(ref self: %{K = V}, ref key: K): ?V with { K is ord, }
  ret map_get(ref self, ref key)
end fun

// Insert an entry, replacing any value already under the key.
fun insert<K, V>(mut self: %{K = V}, key: K, value: V) with { K is ord, }
  map_insert(mut self, key, value)
end fun

// Remove the entry under a key, if there is one.
// Take the entry out, giving back what was under the key.
//
// The value is read before the entry goes, which copies it. The tree destroys
// what it held rather than handing it over, so the only way to keep a value
// through a removal is to have made another first.
fun remove<K, V>(mut self: %{K = V}, ref key: K): ?V with { K is ord, }
  let taken = map_get(ref self, ref key)
  map_remove(mut self, ref key)
  ret taken
end fun

// Drop every entry.
fun clear<K, V>(mut self: %{K = V}) with { K is ord, }
  map_clear(mut self)
end fun

// The value under a key, or a default if there is none.
fun get_or<K, V>(ref self: %{K = V}, ref key: K, default: V): V with { K is ord, }
  if get(ref self, ref key) |value|
    ret value
  else
    ret default
  end if
end fun

// Insert only if the key is not already there. True if it was inserted.
//
// Unlike `insert`, this hands back whether anything happened, which is what
// `set.insert` already does for a set.
fun insert_if_absent<K, V>(mut self: %{K = V}, key: K, value: V): bool with { K is ord, }
  if contains_key(ref self, ref key)
    ret false
  else
    insert(mut self, key, value)
    ret true
  end if
end fun

// The key of the entry at a position in sort order, or none past the end.
//
// A map is a tree, so this walks to the position rather than jumping to it, and
// reaching every entry in turn costs more than it does for a list. It is here
// so that anything written as a loop over `len` and an index -- which is how
// everything over a collection is written -- can be written over a map too.
//
// The key and the value are reached separately because a position is what the
// tree walk gives; `entries` puts them back together.
fun key_at<K, V>(ref self: %{K = V}, i: index): ?K with { K is ord, }
  ret map_key_at(ref self, i)
end fun

// The value of the entry at a position in sort order.
fun value_at<K, V>(ref self: %{K = V}, i: index): ?V with { K is ord, }
  ret map_value_at(ref self, i)
end fun

// Every key, in sort order.
fun keys<K, V>(ref self: %{K = V}): [K] with { K is ord, }
  var built: [K] = []
  let n = len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if key_at(ref self, i) |k|
      list_push(mut built, k)
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret built
end fun

// Every value, in the order of its key.
fun values<K, V>(ref self: %{K = V}): [V] with { K is ord, }
  var built: [V] = []
  let n = len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if value_at(ref self, i) |v|
      list_push(mut built, v)
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret built
end fun

// Every entry as a key and its value, in sort order.
//
// A list of a pair of two type parameters is a shape erasure reaches: the pair
// is converted position by position on the way into the list, against the
// descriptor the call site handed over. Both are cloned, since the map keeps
// what it holds.
fun entries<K, V>(ref self: %{K = V}): [(K, V)] with { K is ord, }
  var built: [(K, V)] = []
  let n = len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if zip_option(key_at(ref self, i), value_at(ref self, i)) |pair|
      list_push(mut built, pair)
    else
      break
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret built
end fun

// A map of the given entries. A key appearing twice keeps the later value.
//
// The entries are moved rather than cloned: the list is taken by value and
// emptied from the back.
fun from_entries<K, V>(entries: [(K, V)]): %{K = V} with { K is ord, }
  var out: %{K = V} = %{}
  var src: [(K, V)] = entries
  loop
    if list_pop(mut src) |pair|
      insert(mut out, pair.0@, pair.1@)
    else
      break
    end if
  end loop
  ret out
end fun
