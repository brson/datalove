// String module for UTF-8 strings.
// Based on Rust's str/String APIs.

require rider std
import std.string_len
import std.string_char_count
import std.string_is_ascii
import std.string_contains
import std.string_starts_with
import std.string_ends_with
import std.string_eq
import std.string_cmp
import std.string_eq_ignore_ascii_case
import std.string_is_ascii_alphabetic
import std.string_is_ascii_digit
import std.string_is_ascii_alphanumeric
import std.string_is_ascii_whitespace
import std.string_trim
import std.string_trim_start
import std.string_trim_end
import std.string_to_ascii_lowercase
import std.string_to_ascii_uppercase
import std.string_repeat
import std.string_concat
import std.string_replace
import std.string_get_byte
import std.string_find
import std.string_rfind
import std.string_strip_prefix
import std.string_strip_suffix
import std.string_to_lowercase
import std.string_to_uppercase
import std.string_from_char
import std.string_replacen
import std.string_push_str
import std.string_push_char
import std.string_clear
import std.string_pop
import std.string_truncate
import std.string_remove
import std.string_insert_char
import std.string_insert_str
import std.string_char_at
import std.string_char_to_byte_index
import std.string_slice
import std.string_slice_from
import std.string_slice_to
import std.string_parse_u32
import std.string_parse_i32
import std.string_parse_f32
import std.string_from_u32
import std.string_from_i32
import std.string_split_once
import std.string_rsplit_once

// --- Basic Properties ---

// Returns the length in bytes.
fun len(ref self: string): index
  ret string_len(self)
end fun

// Returns true if the string is empty.
fun is_empty(ref self: string): bool
  ret len(self) == (: index / 0)
end fun

// --- Byte Access ---

// Returns the byte at the given index, or none if out of bounds.
fun get_byte(ref self: string, index: index): ?u8
  ret string_get_byte(self, index)
end fun

// --- Character Access ---

// Returns the number of Unicode characters.
fun char_count(ref self: string): index
  ret string_char_count(self)
end fun

// Returns the character (codepoint) at the given character index, or none if out of bounds.
fun get_char(ref self: string, index: index): ?u32
  ret string_char_at(self, index)
end fun

// Returns the byte index of the n-th character, or none if out of bounds.
fun find_char(ref self: string, char_index: index): ?index
  ret string_char_to_byte_index(self, char_index)
end fun

// --- Slicing ---

// Returns a substring by byte range, or none if invalid or not on char boundary.
fun slice(ref self: string, start: index, end_idx: index): ?string
  ret string_slice(self, start, end_idx)
end fun

// Returns a substring from start to end of string.
fun slice_from(ref self: string, start: index): ?string
  ret string_slice_from(self, start)
end fun

// Returns a substring from beginning to end index.
fun slice_to(ref self: string, end_idx: index): ?string
  ret string_slice_to(self, end_idx)
end fun

// --- Searching ---

// Returns true if the string contains the given substring.
fun contains(ref self: string, ref pattern: string): bool
  ret string_contains(self, pattern)
end fun

// Returns true if the string starts with the given prefix.
fun starts_with(ref self: string, ref prefix: string): bool
  ret string_starts_with(self, prefix)
end fun

// Returns true if the string ends with the given suffix.
fun ends_with(ref self: string, ref suffix: string): bool
  ret string_ends_with(self, suffix)
end fun

// Returns the byte index of the first occurrence of pattern, or none.
fun find(ref self: string, ref pattern: string): ?index
  ret string_find(self, pattern)
end fun

// Returns the byte index of the last occurrence of pattern, or none.
fun rfind(ref self: string, ref pattern: string): ?index
  ret string_rfind(self, pattern)
end fun

// --- Comparison ---

// Returns true if two strings are equal.
fun eq(ref self: string, ref other: string): bool
  ret string_eq(self, other)
end fun

// Compares two strings lexicographically. Returns -1, 0, or 1.
fun cmp(ref self: string, ref other: string): i32
  ret string_cmp(self, other)
end fun

// Case-insensitive equality (ASCII only).
fun eq_ignore_ascii_case(ref self: string, ref other: string): bool
  ret string_eq_ignore_ascii_case(self, other)
end fun

// --- Case Conversion ---

// Returns a new string with ASCII characters converted to lowercase.
fun to_ascii_lowercase(ref self: string): string
  ret string_to_ascii_lowercase(self)
end fun

// Returns a new string with ASCII characters converted to uppercase.
fun to_ascii_uppercase(ref self: string): string
  ret string_to_ascii_uppercase(self)
end fun

// Returns a new string with Unicode lowercase conversion.
fun to_lowercase(ref self: string): string
  ret string_to_lowercase(self)
end fun

// Returns a new string with Unicode uppercase conversion.
fun to_uppercase(ref self: string): string
  ret string_to_uppercase(self)
end fun

// --- Trimming ---

// Returns a new string with leading and trailing whitespace removed.
fun trim(ref self: string): string
  ret string_trim(self)
end fun

// Returns a new string with leading whitespace removed.
fun trim_start(ref self: string): string
  ret string_trim_start(self)
end fun

// Returns a new string with trailing whitespace removed.
fun trim_end(ref self: string): string
  ret string_trim_end(self)
end fun

// Removes the prefix if present, returns the rest or none.
fun strip_prefix(ref self: string, ref prefix: string): ?string
  ret string_strip_prefix(self, prefix)
end fun

// Removes the suffix if present, returns the rest or none.
fun strip_suffix(ref self: string, ref suffix: string): ?string
  ret string_strip_suffix(self, suffix)
end fun

// --- In-place Mutation ---

// Appends a character (codepoint) to the string.
fun push_char(mut self: string, ch: u32)
  string_push_char(self, ch)
end fun

// Appends another string.
fun push_str(mut self: string, ref other: string)
  string_push_str(self, other)
end fun

// Removes and returns the last character, or none if empty.
fun pop(mut self: string): ?u32
  ret string_pop(self)
end fun

// Truncates the string to the given byte length.
fun truncate(mut self: string, new_len: index)
  string_truncate(self, new_len)
end fun

// Clears the string, making it empty.
fun clear(mut self: string)
  string_clear(self)
end fun

// Inserts a character at the given byte index.
fun insert_char(mut self: string, index: index, ch: u32)
  string_insert_char(self, index, ch)
end fun

// Inserts a string at the given byte index.
fun insert_str(mut self: string, index: index, ref other: string)
  string_insert_str(self, index, other)
end fun

// Removes and returns the character at the given byte index.
fun remove(mut self: string, index: index): ?u32
  ret string_remove(self, index)
end fun

// --- Construction ---

// Creates a new empty string.
fun new(): string
  ret ""
end fun

// Creates a string from a single character (codepoint).
fun from_char(ch: u32): string
  ret string_from_char(ch)
end fun

// Repeats the string n times.
fun repeat(ref self: string, n: index): string
  ret string_repeat(self, n)
end fun

// Concatenates two strings.
fun concat(ref a: string, ref b: string): string
  ret string_concat(a, b)
end fun

// --- Splitting ---

// Splits on the first occurrence, returns (before, after) or none if not found.
fun split_once(ref self: string, ref delimiter: string): ?(string, string)
  ret string_split_once(self, delimiter)
end fun

// Splits on the last occurrence, returns (before, after) or none if not found.
fun rsplit_once(ref self: string, ref delimiter: string): ?(string, string)
  ret string_rsplit_once(self, delimiter)
end fun

// Splits into a list of strings by delimiter.
// intrinsic needed: string_split(ref string, ref string) -> [string]
fun split(ref self: string, ref delimiter: string): [string]
  // TODO: icall string_split(self, delimiter)
  ret []
end fun

// Splits into lines.
// intrinsic needed: string_lines(ref string) -> [string]
fun lines(ref self: string): [string]
  // TODO: icall string_lines(self)
  ret []
end fun

// Splits by whitespace.
// intrinsic needed: string_split_whitespace(ref string) -> [string]
fun split_whitespace(ref self: string): [string]
  // TODO: icall string_split_whitespace(self)
  ret []
end fun

// --- Replacement ---

// Replaces all occurrences of pattern with replacement.
fun replace(ref self: string, ref pattern: string, ref replacement: string): string
  ret string_replace(self, pattern, replacement)
end fun

// Replaces the first n occurrences of pattern.
fun replacen(ref self: string, ref pattern: string, ref replacement: string, n: index): string
  ret string_replacen(self, pattern, replacement, n)
end fun

// --- Character Predicates ---

// Returns true if all characters are ASCII.
fun is_ascii(ref self: string): bool
  ret string_is_ascii(self)
end fun

// Returns true if all bytes are ASCII alphabetic. Returns false if empty.
fun is_ascii_alphabetic(ref self: string): bool
  ret string_is_ascii_alphabetic(self)
end fun

// Returns true if all bytes are ASCII digits. Returns false if empty.
fun is_ascii_digit(ref self: string): bool
  ret string_is_ascii_digit(self)
end fun

// Returns true if all bytes are ASCII alphanumeric. Returns false if empty.
fun is_ascii_alphanumeric(ref self: string): bool
  ret string_is_ascii_alphanumeric(self)
end fun

// Returns true if all bytes are ASCII whitespace. Returns false if empty.
fun is_ascii_whitespace(ref self: string): bool
  ret string_is_ascii_whitespace(self)
end fun

// --- Parsing ---

// Parses the string as an arbitrary-precision integer.
// intrinsic needed: string_parse_int(ref string) -> ?int
fun parse_int(ref self: string): ?int
  // TODO: icall string_parse_int(self)
  ret none
end fun

// Parses the string as u32.
fun parse_u32(ref self: string): ?u32
  ret string_parse_u32(self)
end fun

// Parses the string as i32.
fun parse_i32(ref self: string): ?i32
  ret string_parse_i32(self)
end fun

// Parses the string as f32.
fun parse_f32(ref self: string): ?f32
  ret string_parse_f32(self)
end fun

// Parses the string as bool ("true" or "false").
fun parse_bool(ref self: string): ?bool
  if eq(self, "true")
    ret some true
  else
    if eq(self, "false")
      ret some false
    else
      ret none
    end if
  end if
end fun

// --- Formatting ---

// Converts an arbitrary-precision integer to string.
// intrinsic needed: int_to_string(int) -> string
fun from_int(n: int): string
  // TODO: icall int_to_string(n)
  ret ""
end fun

// Converts u32 to string.
fun from_u32(n: u32): string
  ret string_from_u32(n)
end fun

// Converts i32 to string.
fun from_i32(n: i32): string
  ret string_from_i32(n)
end fun

// Converts bool to "true" or "false".
fun from_bool(b: bool): string
  if b
    ret "true"
  else
    ret "false"
  end if
end fun

// --- Joining ---

// Joins a list of strings with a separator.
// intrinsic needed: string_join(ref [string], ref string) -> string
fun join(ref parts: [string], ref separator: string): string
  // TODO: icall string_join(parts, separator)
  ret ""
end fun
