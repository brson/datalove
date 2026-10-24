# Datalit Grammar

The concrete syntax of datalit, the data-literal language, as EBNF. This is a
reference: it states what the parser in `crates/datalove-datalit/src/parser`
accepts, over the lexer and bracket matcher in `crates/bcts`. Revised October
2026 against that code, with every case below that is not plainly read off
the source run through `datalove lit-ast` and `lit-tycheck`.

What a well-formed literal means, and which ones type-check, is in
[Datalit Typing Rules](datalit-typing-rules.md). Datafun embeds most of this
grammar, with differences listed in
[Datafun Grammar](datafun-ebnf.md#user-content-differences-from-datalit). The
user manual, `mandocs/datalit.md`, carries the same grammar in pieces beside
its prose; where the two disagree, see
[The manual's grammar](#user-content-the-manuals-grammar).

## Contents

- [Notation](#user-content-notation)
- [Grammar](#user-content-grammar)
- [Rules outside the grammar](#user-content-rules-outside-the-grammar)
- [Diagnostics](#user-content-diagnostics)
- [Divergences](#user-content-divergences)
  - [The manual's grammar](#user-content-the-manuals-grammar)
  - [The specification](#user-content-the-specification)

## Notation

ISO 14977 EBNF: `,` concatenates, `|` separates alternatives, `[ x ]` is
optional, `{ x }` repeats zero or more times, `"x"` is a terminal, `? x ?` is
a special sequence described in English, and `(* x *)` is a comment.
Comments in the grammar name the diagnostic a violation produces.

The terminals are tokens as `crates/bcts/src/lexer.rs` makes them, matched
longest first, so `%{`, `#{`, `{|`, `|}`, `[|` and `|]` are each one token:
`% {` and `[ | 1 | ]` are not maps or tensors. Brackets are matched before
parsing.

## Grammar

```ebnf
(* ===== Top level ===== *)
datalit        = ws, full_expr, ws ;                (* anything after it: D021 *)
full_expr      = [ type_hint, ws ], expr ;
type_hint      = ":", ws, type, ws, "/" ;           (* no "/": D049 *)

(* ===== Types ===== *)
type           = option_type | result_type | list_type | tuple_type
               | struct_type | table_type | map_type | set_type
               | tensor_type | atom_type | term_type | enum_type
               | scalar_type | type_name ;
scalar_type    = "bool" | "u8" | "u16" | "u32" | "u64"
               | "i8" | "i16" | "i32" | "i64"
               | "index" | "offset" | "f32" | "f64"
               | "int" | "string" | "data" | "error" ;
option_type    = "?", ws, type ;
result_type    = "!", ws, type ;
list_type      = "[", ws, type, [ ws, "," ], ws, "]" ;
tuple_type     = "(", ws, [ type_list ], ws, ")" ;  (* (T,) is the 1-tuple *)
struct_type    = "{", ws, [ type_field_list ], ws, "}" ;
table_type     = "{|", ws, [ type_field_list ], ws, "|}" ;
map_type       = "%{", ws, type, ws, "=", ws, type, [ ws, "," ], ws, "}" ;  (* D044 *)
set_type       = "#{", ws, type, [ ws, "," ], ws, "}" ;
tensor_type    = "[|", ws, type, ws, ",", ws, rank, [ ws, "," ], ws, "|]" ;  (* D047 *)
rank           = digit, { digit } ;                 (* 1 to 2^32-1, no "_": D045; 0: D042 *)
atom_type      = "atom", ws, ident ;                (* D051 *)
term_type      = "term", ws, ident, ws, type ;      (* D052 *)
enum_type      = "enum", ws, "{", ws, [ enum_variant_list ], ws, "}" ;  (* D032 *)
enum_variant_list = enum_variant, { ws, ",", ws, enum_variant }, [ ws, "," ] ;
enum_variant   = atom_type | term_type ;            (* else D053; names unique: D038 *)
type_name      = ident ;                            (* parsed, always refused: T059 *)

type_list      = type, { ws, ",", ws, type }, [ ws, "," ] ;
type_field_list = type_field, { ws, ",", ws, type_field }, [ ws, "," ] ;
type_field     = ident, ws, ":", ws, type ;         (* D048; names unique: D038 *)

(* ===== Expressions ===== *)
expr           = none_lit | bool_lit | numeric_lit | string_lit
               | option_expr | result_expr | existential_expr
               | group_expr | tuple_expr | list_expr | struct_expr
               | map_expr | set_expr | tensor_expr | table_expr
               | atom_expr | term_expr | enum_expr ;

none_lit       = "none" ;
bool_lit       = "true" | "false" ;
option_expr    = "some", ws, full_expr ;
result_expr    = ( "ok" | "er" ), ws, full_expr ;
existential_expr = ( "data" | "error" ), ws, full_expr ;
atom_expr      = "atom", ws, ident ;                (* D035 *)
term_expr      = "term", ws, ident, ws, full_expr ; (* D036 *)
enum_expr      = "enum", ws, "{", ws, full_expr, [ ws, "," ], ws, "}" ;  (* D037 *)

group_expr     = "(", ws, full_expr, ws, ")" ;      (* (e) is e *)
tuple_expr     = "(", ws, ")"
               | "(", ws, full_expr, ws, ",", [ ws, expr_list ], ws, ")" ;
list_expr      = "[", ws, [ expr_list ], ws, "]" ;
struct_expr    = "{", ws, [ field_list ], ws, "}" ;
map_expr       = "%{", ws, [ entry_list ], ws, "}" ;
set_expr       = "#{", ws, [ expr_list ], ws, "}" ;

expr_list      = full_expr, { ws, ",", ws, full_expr }, [ ws, "," ] ;
field_list     = field, { ws, ",", ws, field }, [ ws, "," ] ;
field          = ident, ws, "=", ws, full_expr ;    (* D018; names unique: D038 *)
entry_list     = entry, { ws, ",", ws, entry }, [ ws, "," ] ;
entry          = full_expr, ws, "=", ws, full_expr ;  (* D017 *)

(* Tensor. The rank is the longest comma_run plus one. *)
tensor_expr    = "[|", ws, [ tensor_header ], [ tensor_body ], ws, "|]" ;
tensor_header  = extent, { ws, extent }, ws, "|", ws ;  (* D040; body must fit: D041 *)
extent         = digit_run ;                        (* fits u32, else D040 *)
tensor_body    = tensor_row, { ws, comma_run, ws, tensor_row } ;
tensor_row     = full_expr, { ws, full_expr } ;
comma_run      = ",", { ws, "," } ;

(* Table. Between "{|" and "|}", outside nested brackets, ws does not
   include a newline: a newline separates rows. *)
table_expr     = "{|", tbl_ws,
                 [ table_header, { ws, newline, tbl_ws, table_row } ],
                 tbl_ws, "|}" ;
table_header   = ident, { ws, ",", ws, ident }, [ ws, "," ] ;   (* D031, D034, D038 *)
table_row      = full_expr, { ws, ",", ws, full_expr }, [ ws, "," ] ;  (* D030 *)
tbl_ws         = { ws | newline } ;

(* ===== Literals: one glued run, no ws or comment inside ===== *)
numeric_lit    = float_lit | hex_lit | int_lit ;   (* malformed or suffixed: D019 *)
int_lit        = [ "-" ], digit_run ;
float_lit      = [ "-" ], digit_run, ( ".", digit_run, [ exponent ] | exponent ) ;
exponent       = ( "e" | "E" ), [ "+" | "-" ], digit_run ;
hex_lit        = "0", ( "x" | "X" ), hex_run ;      (* no sign: D050 *)
digit_run      = digit, [ { digit | "_" }, digit ] ;
hex_run        = hex_digit, [ { hex_digit | "_" }, hex_digit ] ;
string_lit     = '"', { string_char }, '"' ;        (* unterminated: D020 *)
string_char    = escape_seq | ? any character except '"' and '\', newlines included ? ;
escape_seq     = "\", ( '"' | "\" | "n" | "r" | "t" | "0"
               | "u{", hex_digit, { hex_digit }, "}" ) ;  (* 1-6 digits, a scalar value: D039 *)

(* ===== Lexical ===== *)
ident          = ident_start, { ident_start | digit } ;  (* keywords allowed; non-ASCII: D043 *)
ident_start    = "a" .. "z" | "A" .. "Z" | "_" ;
digit          = "0" .. "9" ;
hex_digit      = digit | "a" .. "f" | "A" .. "F" ;
ws             = { whitespace | newline | comment } ;
whitespace     = ? any Unicode whitespace character but "\n" ? ;
newline        = "\n" | ";" ;                     (* the lexer reads ";" as a newline *)
comment        = line_comment | block_comment ;
line_comment   = "//", { ? any character except newline ? } ;
block_comment  = "/*", { block_comment | ? any character, not starting "/*" or "*/" ? }, "*/" ;
                 (* nests; unclosed: D020 *)
```

## Rules outside the grammar

- **Brackets match.** Every bracket is closed by its partner: a close with no
  open is D054, and an open never closed is D055.
- **A hint is a hint on one expression.** What follows `: T /` is a bare
  `expr`, so `: u8 / : u8 / 1` is D020. Only parentheses put a hint under a
  hint, `: u32 / (: u8 / 1)`, which the AST keeps as a `Group` and the type
  checker reads as two hints that must agree (`Check-Hinted`, T040).
- **Grouping is not a node.** `(e)` is `e`, and `(: T / e)` with no hint
  outside it is the hinted `e`. A one-tuple needs its comma: `(1,)`.
  `(,)` and `(1,,)` are D020.
- **Where the hint's `/` ends.** A `/` glued to `/` or `*` starts a comment,
  so `: u8/*c*/1` and `: u8 //1` lose their `/` (D049).
- **Signs.** A sign is a `-` glued to a decimal literal: `- 1` is D050, `-0xF`
  is D050, and `+1` is D020. A float needs digits on both sides of its point:
  `.5` is D020 and `1.` is D019. A letter on the end of a number is D019, not a
  suffix: `1u8` is written `: u8 / 1`.
- **Names.** Field, column and variant names and the names after `atom` and
  `term` are any `ident`, keywords included: `{none = 1}` and `atom true`
  parse. Datalit reserves no words; `true` is a keyword only where an
  expression is expected, and `True` there is D019. A `type_name` is any
  `ident` not already a type keyword; every one is refused by the type checker
  (T059), datalit having no aliases. A word beginning with a digit is no name
  anywhere: `: 5 / 1` is D046.
- **Tensor shape.** Without a header, the body splits on runs of exactly
  rank minus one commas, and recursively below that; groups along one axis
  must have the same shape (D023). A comma run with nothing before or after it
  is D033. Whitespace and comments may sit inside a run: `, ,` is `,,`. Inside
  a row, elements are simply written one after another, and spacing matters
  only where it decides what is glued: `[| 1 -2 |]` and `[|1-2|]` are two
  elements, `1` and `-2`, and `[| 1 - 2 |]` is D050. A `;` is a line break,
  which a tensor passes over like any other. With a header, the body is
  either flat, as many elements as the product of the extents, or shaped
  exactly as the header says (D041); zero extents are allowed, `[| 0 3 | |]`.
  A hint covers one element: `[| : u8 / 1 2 |]` is two elements.
- **Table rows.** A newline or a `;` separates rows, outside nested brackets
  and outside block comments; a line comment's own newline counts. A `;` is a
  line break -- the lexer takes it for a newline -- so a run of them is blank
  rows, which are free. A `,` with nothing before it is D033, a trailing one is
  fine. Every row has one cell per
  column (D030); each cell is one `full_expr`, so `{| x, y; 1 2, 3 |}` is D021.
  A header cell is one name (D031, D034), and names are unique (D038). The
  header is optional: `{||}` is the empty table.

## Diagnostics

Parse errors. File:line is where each is emitted; `type_hint.rs`,
`expr.rs`, `chunk.rs`, `state.rs` and `mod.rs` are in
`crates/datalove-datalit/src/parser/`.

| Code | Refuses | Where |
|---|---|---|
| D017 | Map entry without `=` | `expr.rs:107` |
| D018 | End of input or a bracket where an expression is expected; struct field without its name or `=` | `expr.rs:394,416,428` |
| D019 | A malformed number or one with a suffix; a bare identifier | `expr.rs:81,85,244` |
| D020 | An unexpected sigil or error token | `expr.rs:264` |
| D021 | Tokens left after an expression | `state.rs:286` |
| D022 | Tokens left inside a type's brackets | `state.rs:275` |
| D023 | Tensor groups of different shapes | `expr.rs:541`, `chunk.rs:334` |
| D030 | Table row with the wrong number of cells | `expr.rs:583`, `chunk.rs:183` |
| D031 | A table header cell that is not a name | `expr.rs:610` |
| D032 | `enum` type without `{` | `type_hint.rs:136` |
| D033 | A `,` with nothing before it, in a table or tensor | `expr.rs:450,518`, `chunk.rs:199` |
| D034 | An extra token in a table header cell | `expr.rs:629` |
| D035 | `atom` with no name | `expr.rs:191` |
| D036 | `term` with no name | `expr.rs:203` |
| D037 | `enum` literal without `{` | `expr.rs:216` |
| D038 | A duplicate field, column or variant name | `datalit/src/parser_util.rs:92` |
| D039 | A bad string escape | `expr.rs:254` |
| D040 | A bad or empty tensor header | `expr.rs:468` |
| D041 | A tensor body that does not fit its header | `expr.rs:486`, `chunk.rs:358` |
| D042 | A tensor type of rank 0 | `type_hint.rs:222` |
| D043 | A non-ASCII name | `mod.rs:40` |
| D044 | Map type without `=` | `type_hint.rs:183` |
| D045 | Bad tensor rank | `type_hint.rs:229` |
| D046 | Unexpected token in a type | `type_hint.rs:271` |
| D047 | Tensor type without its `,` | `type_hint.rs:213` |
| D048 | Struct or table type field without its name or `:` | `type_hint.rs:390,403` |
| D049 | Hint without its `/` | `expr.rs:43` |
| D050 | `-` not glued to a decimal number; a signed hex literal | `expr.rs:135,141` |
| D051 | Atom type with no name | `type_hint.rs:100,333` |
| D052 | Term type with no name | `type_hint.rs:115,353` |
| D053 | An enum variant that is not `atom` or `term` | `type_hint.rs:363` |
| D054 | A close bracket with no open | `mod.rs:37`, through `parser_util.rs` `emit_bracer_errors` |
| D055 | An open bracket never closed | `mod.rs:37`, through `parser_util.rs` `emit_bracer_errors` |

The parse codes start after the ownership codes (D001 to D015,
[spec Section 11.1](botspec.md#user-content-111-errors)); D044 to D053 are
faults that used to share a number with an ownership code or with another
parse fault. Line numbers are as of October 2026 and drift; the code string is
what to search for.

Type errors (T-codes) are listed in
[Datalit Typing Rules](datalit-typing-rules.md#user-content-diagnostics).

## Divergences

### The manual's grammar

`mandocs/datalit.md` is right, and this document was wrong, on grouping and
one-tuples, optional table type columns, tensor ranks, table literal rows,
comma runs with space inside them, Unicode whitespace, a line comment at end
of file, and `\u{}` taking 1 to 6 digits. Those are corrected above. Where the
manual still differs from the parser:

- Its `enum_expr` takes only an atom or term, which is what type-checks; the
  parser takes any `full_expr` and leaves the rest to the checker
  (`: enum { atom A } / enum { 1 }` is T032).
- It has no production for a type name; the parser takes one and the checker
  refuses it (T059). Both should say so.
- It requires an enum type to have a variant. `enum {}` parses and
  type-checks.
- It allows no space after `?` or `!` in a type; the parser does.
- Its prose says spaces separate tensor elements (lines 100 and 576); they are
  written one after another, as above.

### The specification

[Section 2](botspec.md#user-content-2-lexical-conventions) of the spec gives
the shared lexical rules, and [Section 4](botspec.md#user-content-4-type-hints)
hints; they agree with this grammar. What the spec says of datafun's syntax
that is not true of datalit is in
[Datafun Grammar](datafun-ebnf.md#user-content-differences-from-datalit).
