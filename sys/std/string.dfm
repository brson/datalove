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
// TODO: needs native rider support for option returns.
fun get_byte(ref self: string, index: index): ?u8
  ret none
end fun

// --- Character Access ---

// Returns the number of Unicode characters.
fun char_count(ref self: string): index
  ret string_char_count(self)
end fun

// Returns the character (codepoint) at the given character index, or none if out of bounds.
// intrinsic needed: string_char_at(ref string, index) -> ?u32
fun get_char(ref self: string, index: index): ?u32
  // TODO: icall string_char_at(self, index)
  ret none
end fun

// Returns the byte index of the n-th character, or none if out of bounds.
// intrinsic needed: string_char_to_byte_index(ref string, index) -> ?index
fun find_char(ref self: string, char_index: index): ?index
  // TODO: icall string_char_to_byte_index(self, char_index)
  ret none
end fun

// --- Slicing ---

// Returns a substring by byte range, or none if invalid or not on char boundary.
// intrinsic needed: string_slice(ref string, index, index) -> ?string
fun slice(ref self: string, start: index, end_idx: index): ?string
  // TODO: icall string_slice(self, start, end_idx)
  ret none
end fun

// Returns a substring from start to end of string.
// intrinsic needed: string_slice_from(ref string, index) -> ?string
fun slice_from(ref self: string, start: index): ?string
  // TODO: icall string_slice_from(self, start)
  ret none
end fun

// Returns a substring from beginning to end index.
// intrinsic needed: string_slice_to(ref string, index) -> ?string
fun slice_to(ref self: string, end_idx: index): ?string
  // TODO: icall string_slice_to(self, end_idx)
  ret none
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
// TODO: needs native rider support for option returns.
fun find(ref self: string, ref pattern: string): ?index
  ret none
end fun

// Returns the byte index of the last occurrence of pattern, or none.
// TODO: needs native rider support for option returns.
fun rfind(ref self: string, ref pattern: string): ?index
  ret none
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
// intrinsic needed: string_to_ascii_lowercase(ref string) -> string
fun to_ascii_lowercase(ref self: string): string
  // TODO: icall string_to_ascii_lowercase(self)
  ret ""
end fun

// Returns a new string with ASCII characters converted to uppercase.
// intrinsic needed: string_to_ascii_uppercase(ref string) -> string
fun to_ascii_uppercase(ref self: string): string
  // TODO: icall string_to_ascii_uppercase(self)
  ret ""
end fun

// Returns a new string with Unicode lowercase conversion.
// intrinsic needed: string_to_lowercase(ref string) -> string
fun to_lowercase(ref self: string): string
  // TODO: icall string_to_lowercase(self)
  ret ""
end fun

// Returns a new string with Unicode uppercase conversion.
// intrinsic needed: string_to_uppercase(ref string) -> string
fun to_uppercase(ref self: string): string
  // TODO: icall string_to_uppercase(self)
  ret ""
end fun

// --- Trimming ---

// Returns a new string with leading and trailing whitespace removed.
// intrinsic needed: string_trim(ref string) -> string
fun trim(ref self: string): string
  // TODO: icall string_trim(self)
  ret ""
end fun

// Returns a new string with leading whitespace removed.
// intrinsic needed: string_trim_start(ref string) -> string
fun trim_start(ref self: string): string
  // TODO: icall string_trim_start(self)
  ret ""
end fun

// Returns a new string with trailing whitespace removed.
// intrinsic needed: string_trim_end(ref string) -> string
fun trim_end(ref self: string): string
  // TODO: icall string_trim_end(self)
  ret ""
end fun

// Removes the prefix if present, returns the rest or none.
// intrinsic needed: string_strip_prefix(ref string, ref string) -> ?string
fun strip_prefix(ref self: string, ref prefix: string): ?string
  // TODO: icall string_strip_prefix(self, prefix)
  ret none
end fun

// Removes the suffix if present, returns the rest or none.
// intrinsic needed: string_strip_suffix(ref string, ref string) -> ?string
fun strip_suffix(ref self: string, ref suffix: string): ?string
  // TODO: icall string_strip_suffix(self, suffix)
  ret none
end fun

// --- In-place Mutation ---

// Appends a character (codepoint) to the string.
// intrinsic needed: string_push_char(mut string, u32) -> ()
fun push_char(mut self: string, ch: u32)
  // TODO: icall string_push_char(self, ch)
end fun

// Appends another string.
// intrinsic needed: string_push_str(mut string, ref string) -> ()
fun push_str(mut self: string, ref other: string)
  // TODO: icall string_push_str(self, other)
end fun

// Removes and returns the last character, or none if empty.
// intrinsic needed: string_pop(mut string) -> ?u32
fun pop(mut self: string): ?u32
  // TODO: icall string_pop(self)
  ret none
end fun

// Truncates the string to the given byte length.
// intrinsic needed: string_truncate(mut string, index) -> !()
fun truncate(mut self: string, new_len: index): !()
  // TODO: icall string_truncate(self, new_len)
  ret ok ()
end fun

// Clears the string, making it empty.
// intrinsic needed: string_clear(mut string) -> ()
fun clear(mut self: string)
  // TODO: icall string_clear(self)
end fun

// Inserts a character at the given byte index.
// intrinsic needed: string_insert_char(mut string, index, u32) -> !()
fun insert_char(mut self: string, index: index, ch: u32): !()
  // TODO: icall string_insert_char(self, index, ch)
  ret ok ()
end fun

// Inserts a string at the given byte index.
// intrinsic needed: string_insert_str(mut string, index, ref string) -> !()
fun insert_str(mut self: string, index: index, ref other: string): !()
  // TODO: icall string_insert_str(self, index, other)
  ret ok ()
end fun

// Removes and returns the character at the given byte index.
// intrinsic needed: string_remove(mut string, index) -> ?u32
fun remove(mut self: string, index: index): ?u32
  // TODO: icall string_remove(self, index)
  ret none
end fun

// --- Construction ---

// Creates a new empty string.
fun new(): string
  ret ""
end fun

// Creates a string from a single character (codepoint).
// intrinsic needed: string_from_char(u32) -> string
fun from_char(ch: u32): string
  // TODO: icall string_from_char(ch)
  ret ""
end fun

// Repeats the string n times.
// intrinsic needed: string_repeat(ref string, index) -> string
fun repeat(ref self: string, n: index): string
  // TODO: icall string_repeat(self, n)
  ret ""
end fun

// Concatenates two strings.
// intrinsic needed: string_concat(string, string) -> string
fun concat(self: string, other: string): string
  // TODO: icall string_concat(self, other)
  ret ""
end fun

// --- Splitting ---

// Splits on the first occurrence, returns (before, after) or none if not found.
// intrinsic needed: string_split_once(ref string, ref string) -> ?(string, string)
fun split_once(ref self: string, ref delimiter: string): ?(string, string)
  // TODO: icall string_split_once(self, delimiter)
  ret none
end fun

// Splits on the last occurrence, returns (before, after) or none if not found.
// intrinsic needed: string_rsplit_once(ref string, ref string) -> ?(string, string)
fun rsplit_once(ref self: string, ref delimiter: string): ?(string, string)
  // TODO: icall string_rsplit_once(self, delimiter)
  ret none
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
// intrinsic needed: string_replace(ref string, ref string, ref string) -> string
fun replace(ref self: string, ref pattern: string, ref replacement: string): string
  // TODO: icall string_replace(self, pattern, replacement)
  ret ""
end fun

// Replaces the first n occurrences of pattern.
// intrinsic needed: string_replacen(ref string, ref string, ref string, index) -> string
fun replacen(ref self: string, ref pattern: string, ref replacement: string, n: index): string
  // TODO: icall string_replacen(self, pattern, replacement, n)
  ret ""
end fun

// --- Character Predicates ---

// Returns true if all characters are ASCII.
fun is_ascii(ref self: string): bool
  ret string_is_ascii(self)
end fun

// Returns true if all bytes are ASCII alphabetic. Returns false if empty.
fun is_ascii_alphabetic(ref self: string): bool
  if is_empty(self)
    ret false
  end if
  var i: index = (: index / 0)
  let byte_len = len(self)
  let one = (: index / 1)
  loop while i .< byte_len
    if get_byte(self, i) |b|
      let is_upper = b >= (: u8 / 65) and b <= (: u8 / 90)
      let is_lower = b >= (: u8 / 97) and b <= (: u8 / 122)
      if not (is_upper or is_lower)
        ret false
      end if
    end if
    set i = icall add_wrapping_index(i, one)
  end loop
  ret true
end fun

// Returns true if all bytes are ASCII digits. Returns false if empty.
fun is_ascii_digit(ref self: string): bool
  if is_empty(self)
    ret false
  end if
  var i: index = (: index / 0)
  let byte_len = len(self)
  let one = (: index / 1)
  loop while i .< byte_len
    if get_byte(self, i) |b|
      if b .< (: u8 / 48) or b .> (: u8 / 57)
        ret false
      end if
    end if
    set i = icall add_wrapping_index(i, one)
  end loop
  ret true
end fun

// Returns true if all bytes are ASCII alphanumeric. Returns false if empty.
fun is_ascii_alphanumeric(ref self: string): bool
  if is_empty(self)
    ret false
  end if
  var i: index = (: index / 0)
  let byte_len = len(self)
  let one = (: index / 1)
  loop while i .< byte_len
    if get_byte(self, i) |b|
      let is_upper = b >= (: u8 / 65) and b <= (: u8 / 90)
      let is_lower = b >= (: u8 / 97) and b <= (: u8 / 122)
      let is_digit = b >= (: u8 / 48) and b <= (: u8 / 57)
      if not (is_upper or is_lower or is_digit)
        ret false
      end if
    end if
    set i = icall add_wrapping_index(i, one)
  end loop
  ret true
end fun

// Returns true if all bytes are ASCII whitespace. Returns false if empty.
fun is_ascii_whitespace(ref self: string): bool
  if is_empty(self)
    ret false
  end if
  var i: index = (: index / 0)
  let byte_len = len(self)
  let one = (: index / 1)
  loop while i .< byte_len
    if get_byte(self, i) |b|
      // Space, tab, newline, carriage return, form feed, vertical tab.
      let is_space = b == (: u8 / 32)
      let is_tab = b == (: u8 / 9)
      let is_newline = b == (: u8 / 10)
      let is_cr = b == (: u8 / 13)
      let is_ff = b == (: u8 / 12)
      let is_vt = b == (: u8 / 11)
      if not (is_space or is_tab or is_newline or is_cr or is_ff or is_vt)
        ret false
      end if
    end if
    set i = icall add_wrapping_index(i, one)
  end loop
  ret true
end fun

// --- Parsing ---

// Parses the string as an arbitrary-precision integer.
// intrinsic needed: string_parse_int(ref string) -> ?int
fun parse_int(ref self: string): ?int
  // TODO: icall string_parse_int(self)
  ret none
end fun

// Parses the string as u32.
// intrinsic needed: string_parse_u32(ref string) -> ?u32
fun parse_u32(ref self: string): ?u32
  // TODO: icall string_parse_u32(self)
  ret none
end fun

// Parses the string as i32.
// intrinsic needed: string_parse_i32(ref string) -> ?i32
fun parse_i32(ref self: string): ?i32
  // TODO: icall string_parse_i32(self)
  ret none
end fun

// Parses the string as f32.
// intrinsic needed: string_parse_f32(ref string) -> ?f32
fun parse_f32(ref self: string): ?f32
  // TODO: icall string_parse_f32(self)
  ret none
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
// intrinsic needed: u32_to_string(u32) -> string
fun from_u32(n: u32): string
  // TODO: icall u32_to_string(n)
  ret ""
end fun

// Converts i32 to string.
// intrinsic needed: i32_to_string(i32) -> string
fun from_i32(n: i32): string
  // TODO: icall i32_to_string(n)
  ret ""
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
