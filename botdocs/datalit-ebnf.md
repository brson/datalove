# Datalit Grammar

The concrete syntax of datalit, the data-literal language, as EBNF. This is a
reference: it states what the parser in `crates/datalove-datalit/src/parser`
accepts, over the lexer and bracket matcher in `crates/bcts`, and a test
holds it to that (see [Checking the grammar](#user-content-checking-the-grammar)).

What a well-formed literal means, and which ones type-check, is in
[Datalit Typing Rules](datalit-typing-rules.md). Datafun embeds most of this
grammar, with differences listed in
[Datafun Grammar](datafun-ebnf.md#user-content-differences-from-datalit). The
user manual, `mandocs/datalit.md`, describes the syntax in prose; where it
disagrees with the parser, see [The manual](#user-content-the-manual).

## Contents

- [Notation](#user-content-notation)
- [Grammar](#user-content-grammar)
- [Rules outside the grammar](#user-content-rules-outside-the-grammar)
- [Recovery](#user-content-recovery)
- [Checking the grammar](#user-content-checking-the-grammar)
- [Diagnostics](#user-content-diagnostics)
- [Divergences](#user-content-divergences)
  - [The manual](#user-content-the-manual)
  - [The specification](#user-content-the-specification)

## Notation

ISO 14977 EBNF: `,` concatenates, `|` separates alternatives, `[ x ]` is
optional, `{ x }` repeats zero or more times, `( x )` groups, `a - b` is what
`a` matches that `b` does not, `"x"` is a terminal (with no escapes, but
`"\n"` is a newline character), `"a" .. "z"` is a range of characters,
`? x ?` is a special sequence described in English, and `(* x *)` is a
comment. Comments in the grammar name the diagnostic a violation produces.

The grammar is over the tokens of [Lexical Grammar](lexical-grammar.md): a
terminal matches a token by its text, and the whitespace and comments
between tokens are not written. So `%{`, `#{`, `{|`, `|}`, `[|`, `|]` and `->`
are each one token, and `% {` and `[ | 1 | ]` are not maps or tensors;
`truefalse` is one name, not two booleans; and `: u8/*c*/1` and `: u8 //1`
are D049, the comment having taken the hint's `/`. Brackets are matched
before parsing.

A newline means nothing but in a table, where it is the token `newline` and
breaks a row, as a `;` does. A type reads a newline as whitespace, so a table
type may be written over several lines, but a `;` in one is an error. A `;`
outside a table is a token no rule takes.

The rules under `(* Written in characters *)` describe the text of one glued
run of tokens, character by character, as the lexical grammar does: a
number, a string, a rank. A rule over tokens that uses one matches the tokens
of a glued run whose text it derives. `? no "." or word glued after it ?`
says a number is the whole of what is glued there, so `1.x` is a malformed
number, not `1` and something after it.

## Grammar

```ebnf
(* ===== Top level ===== *)
datalit        = full_expr ;                       (* anything after it: D021 *)
full_expr      = [ type_hint ], expr ;
type_hint      = ":", type, "/" ;                  (* no "/": D049 *)

(* ===== Types ===== *)
type           = option_type | result_type | list_type | tuple_type
               | struct_type | table_type | map_type | set_type
               | tensor_type | atom_type | term_type | enum_type
               | rec_type | scalar_type | type_name ;
scalar_type    = "bool" | "u8" | "u16" | "u32" | "u64"
               | "i8" | "i16" | "i32" | "i64"
               | "index" | "offset" | "f32" | "f64"
               | "int" | "string" | "data" | "error" ;
option_type    = "?", type ;
result_type    = "!", type ;
list_type      = "[", type, [ "," ], "]" ;
tuple_type     = "(", [ type_list ], ")" ;        (* (T,) is the 1-tuple *)
struct_type    = "{", [ type_field_list ], "}" ;
table_type     = "{|", [ type_field_list ], "|}" ;
map_type       = "%{", type, "->", type, [ "," ], "}" ;  (* D044 *)
set_type       = "#{", type, [ "," ], "}" ;
tensor_type    = "[|", type, ",", rank, [ "," ], "|]" ;  (* D047 *)
atom_type      = "atom", NAME ;                    (* D051 *)
term_type      = "term", NAME, type ;              (* D052 *)
enum_type      = "enum", "{", [ enum_variant_list ], "}" ;  (* D032 *)
enum_variant_list = enum_variant, { ",", enum_variant }, [ "," ] ;
enum_variant   = atom_type | term_type ;           (* else D053; names unique: D038 *)
rec_type       = "rec", NAME, type ;               (* no name: D056; a type word: D057 *)
type_name      = NAME - ( scalar_type | "atom" | "term" | "enum" | "rec" ) ;
                 (* a rec's name in its body, else T059 *)

type_list      = type, { ",", type }, [ "," ] ;
type_field_list = type_field, { ",", type_field }, [ "," ] ;
type_field     = NAME, ":", type ;                 (* D048; names unique: D038 *)

(* ===== Expressions ===== *)
expr           = none_lit | bool_lit | numeric_lit | string_lit
               | option_expr | result_expr | existential_expr
               | group_expr | tuple_expr | list_expr | struct_expr
               | map_expr | set_expr | tensor_expr | table_expr
               | atom_expr | term_expr | enum_expr ;

none_lit       = "none" ;
bool_lit       = "true" | "false" ;
option_expr    = "some", full_expr ;
result_expr    = ( "ok" | "er" ), full_expr ;
existential_expr = ( "data" | "error" ), full_expr ;
atom_expr      = "atom", NAME ;                    (* D035 *)
term_expr      = "term", NAME, full_expr ;         (* D036 *)
enum_expr      = "enum", "{", full_expr, [ "," ], "}" ;  (* D037 *)

group_expr     = "(", full_expr, ")" ;             (* (e) is e *)
tuple_expr     = "(", ")"
               | "(", full_expr, ",", [ expr_list ], ")" ;
list_expr      = "[", [ expr_list ], "]" ;
struct_expr    = "{", [ field_list ], "}" ;
map_expr       = "%{", [ entry_list ], "}" ;
set_expr       = "#{", [ expr_list ], "}" ;

expr_list      = full_expr, { ",", full_expr }, [ "," ] ;
field_list     = field, { ",", field }, [ "," ] ;
field          = NAME, "=", full_expr ;            (* D058; names unique: D038 *)
entry_list     = entry, { ",", entry }, [ "," ] ;
entry          = full_expr, "->", full_expr ;      (* D017 *)

(* Tensor. The rank is the longest comma_run plus one. *)
tensor_expr    = "[|", [ tensor_header ], [ tensor_body ], "|]" ;
tensor_header  = extent, { extent }, "|" ;         (* D040; body must fit: D041 *)
extent         = digit_run ;                       (* fits u32, else D040 *)
tensor_body    = tensor_row, { comma_run, tensor_row } ;
tensor_row     = full_expr, { full_expr } ;
comma_run      = ",", { "," } ;

(* Table. A newline or a ";" breaks a row. *)
table_expr     = "{|", { row_break },
                 [ table_header, { row_break, { row_break }, table_row } ],
                 { row_break }, "|}" ;
row_break      = newline | ";" ;
table_header   = NAME, { ",", NAME }, [ "," ] ;    (* D031, D034, D038 *)
table_row      = full_expr, { ",", full_expr }, [ "," ] ;  (* D030 *)

(* ===== Tokens  (lexical-grammar.md) ===== *)
NAME           = ? an ASCII word not beginning with a digit ? ;
                 (* keywords allowed; non-ASCII: D043 *)
newline        = ? a newline in a table, outside brackets nested in it ? ;
numeric_lit    = number, ? no "." or word glued after it ? ;

(* Written in characters, each one glued run of tokens. *)
number         = float_lit | hex_lit | int_lit ;   (* malformed: D019; suffixed: D060 *)
int_lit        = [ "-" ], digit_run ;
float_lit      = [ "-" ], digit_run, ( ".", digit_run, [ exponent ] | exponent ) ;
exponent       = ( "e" | "E" ), [ "+" | "-" ], digit_run ;
hex_lit        = "0", ( "x" | "X" ), hex_run ;     (* no sign: D061 *)
rank           = digit, { digit } ;                (* 1 to 2^32-1, no "_": D045; 0: D042 *)
digit_run      = digit, [ { digit | "_" }, digit ] ;
hex_run        = hex_digit, [ { hex_digit | "_" }, hex_digit ] ;
string_lit     = '"', { string_char }, '"' ;       (* unterminated: D020 *)
string_char    = escape_seq | ? any character except '"' and '\', newlines included ? ;
escape_seq     = "\", ( '"' | "\" | "n" | "r" | "t" | "0"
               | "u{", hex_digit, { hex_digit }, "}" ) ;  (* 1-6 digits, a scalar value: D039 *)
digit          = "0" .. "9" ;
hex_digit      = digit | "a" .. "f" | "A" .. "F" ;
```

## Rules outside the grammar

The parser refuses some text the grammar derives, by the rules below, and
then only with these codes: D023, D030, D038, D039, D040, D041, D042, D045,
D057. Everything else it refuses, the grammar does not derive, and everything
it accepts, the grammar does. `crates/datalove-doc-fixtures` checks this (see
[Checking the grammar](#user-content-checking-the-grammar)).

- **Brackets match.** Every bracket is closed by its partner: a close with no
  open is D054, and an open never closed is D055.
- **A hint is a hint on one expression.** What follows `: T /` is a bare
  `expr`, so `: u8 / : u8 / 1` is D020, which says to parenthesize the inner
  hint, as datafun's P080 does. Only parentheses put a hint under a
  hint, `: u32 / (: u8 / 1)`, which the AST keeps as one `Hinted` node under
  another and the type checker reads as two hints that must agree
  (`Check-Hinted`, T040).
- **Grouping is not a node.** `(e)` is `e`, and `(: T / e)` with no hint
  outside it is the hinted `e`. A one-tuple needs its comma: `(1,)`.
  `(,)` and `(1,,)` are D020.
- **Signs.** A sign is a `-` glued to a decimal literal: `- 1` is D050, `-0xF`
  is D061, and `+1` is D020. A float needs digits on both sides of its point:
  `.5` is D020 and `1.` is D019. A letter on the end of a number is D060, not a
  suffix: `1u8` is written `: u8 / 1`.
- **Names.** Field, column and variant names and the names after `atom` and
  `term` are any `NAME`, keywords included: `{none = 1}` and `atom true`
  parse. Datalit reserves no words; `true` is a keyword only where an
  expression is expected, and `True` there is D059. The type checker refuses
  every `type_name` that is not the name of a `rec` around it (T059), datalit
  having no aliases. A word beginning with a digit is no name anywhere:
  `: 5 / 1` is D046.
- **Recursive types.** `rec L T` names `T` as `L` inside `T`; the uses of `L`
  parse as `type_name`s, and the type checker tells them apart. A `rec`'s name
  is a name of a type, so it cannot be a word that is a type or starts one
  (D057): `rec int [int]` and `rec rec [rec]` are refused. What a `rec` may
  hold is the type checker's business (T062 to T065, in
  [the typing rules](datalit-typing-rules.md)).
- **Tensor shape.** Without a header, the body splits on runs of exactly
  rank minus one commas, and recursively below that; groups along one axis
  must have the same shape (D023). A comma run with nothing before or after it
  is D033. Whitespace and comments may sit inside a run: `, ,` is `,,`. Inside
  a row, elements are simply written one after another, and spacing matters
  only where it decides what is glued: `[| 1 -2 |]` and `[|1-2|]` are two
  elements, `1` and `-2`, and `[| 1 - 2 |]` is D050. With a header, the body is
  either flat, as many elements as the product of the extents, or shaped
  exactly as the header says (D041); zero extents are allowed, `[| 0 3 | |]`.
  A hint covers one element: `[| : u8 / 1 2 |]` is two elements.
- **Table rows.** A `;` breaks a row as a newline does, so a run of them is
  blank rows, which are free. It does nothing else: `[1; 2]` is D021, and a
  `;` in a table type is D022. A `,` with nothing before it is D033, a trailing one is
  fine. Every row has one cell per column (D030); each cell is one
  `full_expr`, so `{| x, y; 1 2, 3 |}` is D021. A header cell is one name
  (D031, D034), and names are unique (D038). The header is optional: `{||}`
  is the empty table.

The examples above, and more, each with the codes the parser refuses it
with, or `ok` (see [Checking the grammar](#user-content-checking-the-grammar)):

```examples datalit
D059       truefalse
ok         [| 12 |]
ok         [|1-2|]
D049       : u8/*c*/1
D049       : u8 //1
D055       (//)
D054       1)
D055       [1, 2
D020       : u8 / : u8 / 1
ok         : u32 / (: u8 / 1)
ok         (1,)
D020       (,)
D020       (1,,)
D050       - 1
D061       -0xF
D020       +1
D020       .5
D019       1.
D060       1u8
ok         {none = 1}
ok         atom true
D059       True
D046       : 5 / 1
D057       : rec int [int] / []
D057       : rec rec [rec] / []
ok         [| 1 -2 |]
D050       [| 1 - 2 |]
D023       [| 1 2, 3 |]
D033       [| , 1 |]
ok         {| x, y;; 1, 2; |}
D021       [1; 2]
D021       1;
D022       : {| a: int; b: int |} {||}
ok         [| 0 3 | |]
D041       [| 2 2 | 1 2 3 |]
ok         [| : u8 / 1 2 |]
ok         {| x, y
              1, 2
              3, 4 |}
ok         {| x, y; 1, 2,; ; 3, 4 |}
ok         {| x, y // a comment; not a row
              1, 2 /* nor
              this */ |}
ok         : {| a: int,
                b: int |} / {||}
ok         : {| a:
                int |} / {||}
D022       : {| a: int
                b: int |} / {||}
D033       {| x, y; , 1 |}
D030       {| x, y; 1 |}
D021       {| x, y; 1 2, 3 |}
D038       {| x, x; 1, 2 |}
ok         {||}
```

## Recovery

A literal is one expression, so one statement however many lines it spans,
and the parser reports one error of it: the first, in the order of the
source, with what it led to dropped. The errors found before parsing,
unmatched brackets (D054, D055) and names outside ASCII (D043), are among
them. The first error is on the token where the grammar first cannot go on,
or on a construct that ends there, or in the gap before what is there instead
of a missing token, but for an unmatched bracket, which is reported where it
opens. `recovery_contract` in `crates/datalove-doc-fixtures/tests/recovery_tests.rs`
holds the parser to this, as it holds datafun's to
[its rules](datafun-ebnf.md#user-content-recovery).

```examples datalit
D020       [1,
            +1,
            +2]
D033       {| x, y; , 1; , 2 |}
D055       [1, (2
```

## Checking the grammar

`crates/datalove-doc-fixtures/tests/datalit_grammar_tests.rs` reads the
grammar block above and runs it, as an Earley recognizer, beside the parser.
Whatever the parser accepts the recognizer must too, and whatever the
recognizer accepts the parser must accept or refuse only with a code from
the list at the top of [Rules outside the grammar](#user-content-rules-outside-the-grammar).
The test reads that list from this document.

Its inputs are the `examples` blocks above, every `.dlt` file under
`crates/`, strings generated from the grammar, and each of those with a
character changed. A disagreement is shrunk to the shortest
input that still disagrees.

The recognizer reads the tokens the real lexer makes, which
[Lexical Grammar](lexical-grammar.md) holds to its own grammar, with a
`newline` before a token in a `{|` `|}` pair, outside the brackets nested in
it. A `type` matches a run of tokens with its brackets matched, read by the
type rules with the `newline`s in it dropped, and a rule written in
characters matches a glued run by its text. The grammar is otherwise run as
written. What each `? x ?` means is in
`crates/datalove-doc-fixtures/src/token_grammar.rs` and `datalit_grammar.rs`
beside it, and a new special sequence fails the test until it is given a
meaning there too.

An `examples` block is fenced as ```` ```examples datalit ````. Each example
begins on a line of its own with `ok` or the codes the parser refuses it
with, joined by commas and in the order of the source, then spaces and the
input. Lines after it indented to where its input began continue it. The test
holds the parser to exactly those codes, besides holding it and the grammar
to each other.

So an edit here that the parser does not match fails the test. A new rule
in English that the grammar does not express needs the test taught it, or
the code it refuses with added to the list.

## Diagnostics

Parse errors. File:line is where each is emitted; `type_hint.rs`,
`expr.rs`, `chunk.rs`, `state.rs` and `mod.rs` are in
`crates/datalove-datalit/src/parser/`.

| Code | Refuses | Where |
|---|---|---|
| D017 | Map entry without `->` | `expr.rs:107` |
| D018 | End of input or a bracket where an expression is expected | `expr.rs:418,426` |
| D019 | A malformed number | `expr.rs:81` |
| D020 | An unexpected sigil or error token, or a hint directly under another | `expr.rs:292,69` |
| D021 | Tokens left after an expression | `state.rs:286` |
| D022 | Tokens left inside a type's brackets | `state.rs:275` |
| D023 | Tensor groups of different shapes | `expr.rs:541`, `chunk.rs:334` |
| D030 | Table row with the wrong number of cells | `expr.rs:583`, `chunk.rs:183` |
| D031 | A table header cell that is not a name | `expr.rs:610` |
| D032 | `enum` type without `{` | `type_hint.rs:159` |
| D033 | A `,` with nothing before it, in a table or tensor | `expr.rs:450,518`, `chunk.rs:199` |
| D034 | An extra token in a table header cell | `expr.rs:629` |
| D035 | `atom` with no name | `expr.rs:191` |
| D036 | `term` with no name | `expr.rs:203` |
| D037 | `enum` literal without `{` | `expr.rs:216` |
| D038 | A duplicate field, column or variant name | `datalit/src/parser_util.rs:92` |
| D039 | A bad string escape | `expr.rs:254` |
| D040 | A bad or empty tensor header | `expr.rs:468` |
| D041 | A tensor body that does not fit its header | `expr.rs:486`, `chunk.rs:358` |
| D042 | A tensor type of rank 0 | `type_hint.rs:245` |
| D043 | A non-ASCII name | `mod.rs:40` |
| D044 | Map type without `->` | `type_hint.rs:206` |
| D045 | Bad tensor rank | `type_hint.rs:252` |
| D046 | Unexpected token in a type | `type_hint.rs:294` |
| D047 | Tensor type without its `,` | `type_hint.rs:236` |
| D048 | Struct or table type field without its name or `:` | `type_hint.rs:413,426` |
| D049 | Hint without its `/` | `expr.rs:93` |
| D050 | `-` not glued to a decimal number | `expr.rs:141` |
| D051 | Atom type with no name | `type_hint.rs:100,356` |
| D052 | Term type with no name | `type_hint.rs:115,376` |
| D053 | An enum variant that is not `atom` or `term` | `type_hint.rs:386` |
| D054 | A close bracket with no open | `mod.rs:37`, through `parser_util.rs` `emit_bracer_errors` |
| D055 | An open bracket never closed | `mod.rs:37`, through `parser_util.rs` `emit_bracer_errors` |
| D056 | `rec` type with no name | `type_hint.rs:130` |
| D057 | `rec` type named with a word that is a type, or starts one | `type_hint.rs:132` |
| D058 | Struct field without its name or `=` | `expr.rs:416,428` |
| D059 | A bare identifier | `expr.rs:244` |
| D060 | A number with a suffix | `expr.rs:85` |
| D061 | A signed hex literal | `expr.rs:135` |

D001 to D016 were the ownership codes, now O001 to O016
([spec Section 11.1](botspec.md#user-content-111-errors)), and are retired.
D044 to D053 and D058 to D061 are faults that used to share a number with an
ownership code or with another parse fault. Line numbers are as of November
2026 and drift; the code is a `DCode` variant in
`crates/datalove-diagnostic-codes`, and its name is what to search for.

Type errors (T-codes) are listed in
[Datalit Typing Rules](datalit-typing-rules.md#user-content-diagnostics).

## Divergences

### The manual

Its prose says spaces separate tensor elements (`mandocs/datalit.md`, Lexical
structure and Tensor); they are written one after another, as above.

### The specification

[Section 2](botspec.md#user-content-2-lexical-conventions) of the spec gives
the shared lexical rules, and [Section 4](botspec.md#user-content-4-type-hints)
hints; they agree with this grammar. What the spec says of datafun's syntax
that is not true of datalit is in
[Datafun Grammar](datafun-ebnf.md#user-content-differences-from-datalit).
