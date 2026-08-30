// Map module.
//
// Generic over the key and value types. A map is not converted when it is
// passed to one of these: it is borrowed, and the descriptor saying what it
// holds comes from the call site. A key being looked up is borrowed too, so it
// arrives as itself. A value crossing the boundary has no type the function
// knows, so it is carried as a `data` and moved back out where the type is
// known.

require rider std
import std.map_len
import std.map_clear
import std.map_contains_key
import std.map_get
import std.map_insert
import std.map_remove

// The number of entries.
fun len<K, V>(ref self: %{K = V}): index
  ret map_len(ref self)
end fun

// True if there are no entries.
fun is_empty<K, V>(ref self: %{K = V}): bool
  ret map_len(ref self) == (: index / 0)
end fun

// True if the key is present.
fun contains_key<K, V>(ref self: %{K = V}, ref key: K): bool
  ret map_contains_key(ref self, ref key)
end fun

// The value for a key, or none.
fun get<K, V>(ref self: %{K = V}, ref key: K): ?V
  ret map_get(ref self, ref key)
end fun

// Insert an entry, replacing any value already under the key.
fun insert<K, V>(mut self: %{K = V}, key: K, value: V)
  map_insert(mut self, key, value)
end fun

// Remove the entry under a key, if there is one.
fun remove<K, V>(mut self: %{K = V}, ref key: K)
  map_remove(mut self, ref key)
end fun

// Drop every entry.
fun clear<K, V>(mut self: %{K = V})
  map_clear(mut self)
end fun
