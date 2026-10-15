# Datafun Grammar

The concrete syntax of datafun, the statement and expression language of
scripts (`.dfs`) and modules (`.dfm`), as EBNF. This is a reference: it states
what the parser in `crates/datalove-datafun-parser` accepts, over the lexer
and bracket matcher in `crates/bcts`. Written October 2026 from that code,
with the cases that are not plainly read off the source run through the
parser.

It builds on [Datalit Grammar](datalit-ebnf.md), whose type syntax it shares
outright and whose value syntax it re-implements with
[differences](#user-content-differences-from-datalit). What a well-formed
program means is in [Datafun Typing Rules](datafun-typing-rules.md); the
[table of productions](#user-content-productions-and-their-typing-rules)
says which rule types each form.

## Contents

- [Notation](#user-content-notation)
- [Grammar](#user-content-grammar)
- [Rules outside the grammar](#user-content-rules-outside-the-grammar)
- [Productions and their typing rules](#user-content-productions-and-their-typing-rules)
- [Differences from datalit](#user-content-differences-from-datalit)
- [Divergences](#user-content-divergences)

## Notation

The EBNF is as in the [datalit grammar](datalit-ebnf.md#user-content-notation),
over tokens rather than characters: whitespace and comments between tokens are
not written. Two more notations carry what tokens alone cannot:

- `~x` means `x` is glued to the token before it: no space, newline or comment
  between them. Spacing decides fixity in datafun
  ([spec Section 2.4](botspec.md#user-content-24-spacing)), and `~` is where it
  does.
- `EOL` is a statement break: a newline before a token at bracket depth 0. A
  `;` is lexed as a newline, so it is one too. Inside any bracket pair a newline is not an `EOL`, nor is
  one inside a block comment. `EOLS` is one or more; blank lines are free.

Brackets, `( [ { {| [| %{ #{ < (| <|` and their partners, are matched before
parsing (P050, P051). `<` and `>` are always brackets, never operators.

The parser's sources are cited by file: `statement.rs`, `expr.rs`,
`literal.rs`, `state.rs` and `lib.rs` are in `crates/datalove-datafun-parser/src/`.

## Grammar

```ebnf
(* ===== Top level  (lib.rs:147-259) ===== *)
script        = [ EOLS ], [ top_stmt, { EOLS, top_stmt }, [ EOLS ] ] ;
module        = script ;            (* but see P077 below *)
interface     = [ EOLS ], [ iface_stmt, { EOLS, iface_stmt }, [ EOLS ] ] ;
iface_stmt    = native_fun | type_alias ;  (* a rider.dli; anything else: P084 *)
top_stmt      = decl_stmt | stmt ;
decl_stmt     = fun_def | native_fun | type_alias | require_stmt | import_stmt ;
stmt          = let_stmt | var_stmt | const_stmt | set_stmt | call_stmt
              | ret_stmt | if_stmt | match_stmt | loop_stmt | break_stmt
              | continue_stmt | debuglog_stmt ;
                (* chosen by the first word; anything else: P001 *)
block         = { ( stmt | decl_stmt ), EOLS } ;   (* decl_stmt in a block: P074 *)
end_line(k)   = "end", k ;         (* a line of its own; extra tokens: P033; missing: P034 *)

(* ===== Bindings  (statement.rs:157-411) ===== *)
let_stmt      = "let", binding, [ ":", type ], "=", expr ;      (* no "=": P083 *)
var_stmt      = "var", binding, [ ":", type ], "=", expr
              | "var", value_name, ":", type ;   (* pattern without value: P068; no type: P026 *)
const_stmt    = "const", value_name, [ ":", type ], "=", expr ; (* P024, P025 *)
binding       = value_name
              | "(", [ value_name, { ",", value_name }, [ "," ] ], ")"   (* (a): P070 *)
              | "{", [ field_pat, { ",", field_pat }, [ "," ] ], "}"
              | "atom", NAME
              | "term", NAME, value_name ;       (* one level deep; nested: P069 *)
field_pat     = NAME, [ "=", value_name ] ;      (* names unique: D038 *)

(* ===== Assignment  (statement.rs:413-511) ===== *)
set_stmt      = "set", NAME, { place_step }, assign_op, expr ;  (* no op: D025 *)
place_step    = ~".", field_sel
              | ~"[", expr, "]", [ ~"?" | ~"!" ] ;   (* spaced: P021, read anyway *)
assign_op     = "=" | "+=" | "-=" | "*=" | "/=" | "+!=" | "-!=" | "*!=" | "/!="
              | "+?=" | "-?=" | "*?=" | "/?=" ;

(* ===== Functions  (statement.rs:515-801) ===== *)
fun_def       = "fun", value_name, [ type_params ], param_list, [ ":", type ],
                [ where_clause ], EOLS,
                [ "inline", EOLS ],            (* elsewhere: P076 *)
                block, end_line("fun") ;
native_fun    = "native", "fun", value_name, [ type_params ], param_list,
                [ ":", type ], [ where_clause ] ;   (* P028, P029, P030 *)
type_params   = "<", [ type_name, { ",", type_name }, [ "," ] ], ">" ;
param_list    = "(", [ param, { ",", param }, [ "," ] ], ")" ;  (* P002 *)
param         = [ "const" ], [ "ref" | "mut" | "out" ], param_name, ":", type ;
                (* const with a mode: P082; no name: P011; no ":": P012 *)
where_clause  = "where", "{", [ bound, { ",", bound }, [ "," ] ], "}" ;  (* P061, P062 *)
bound         = NAME, "is", ( "ord" | "float" | "fixedint" ) ;
                (* P063, P060; a parameter bounded twice: P078 *)
type_alias    = "type", type_name, ":", type ;  (* P022, P023 *)

(* ===== Modules  (statement.rs:818-1020) ===== *)
require_stmt  = "require", ( "module", lib_path
                           | "data", lib_path, [ ":", type ]
                           | "rider", value_name ) ;           (* else P005 *)
lib_path      = NAME, "/", NAME, "/", NAME ;    (* P003, P004, P013-P016 *)
import_stmt   = "import", NAME, ".", NAME ;     (* P006, P017, P018 *)

(* ===== Control flow  (statement.rs:1022-1530) ===== *)
if_stmt       = "if", if_arm, end_line("if") ;
if_arm        = expr, [ pipe_binding ], EOLS, block,
                [ "else", [ pipe_binding ],
                  ( "if", if_arm | EOLS, block ) ] ;   (* else-if shares one "end if" *)
pipe_binding  = "|", value_name, "|" ;          (* P019, P020 *)
loop_stmt     = "loop", [ "while", expr ], EOLS, block, end_line("loop") ;
break_stmt    = "break" ;
continue_stmt = "continue" ;
match_stmt    = "match", expr, EOLS, { case_arm }, end_line("match") ;  (* P048 *)
case_arm      = "case", ( "atom", NAME | "term", NAME, value_name | "default" ),
                EOLS, block ;                   (* P044-P047; second default: P049 *)
ret_stmt      = "ret", [ expr ] ;
debuglog_stmt = "debuglog", expr ;
call_stmt     = "call", expr ;                  (* must parse to a call node: P031 *)

(* ===== Expressions  (expr.rs:22-91, precedence table 325-348) ===== *)
expr          = or_expr ;
or_expr       = and_expr, { ( "or" | "xor" ), and_expr } ;
and_expr      = cmp_expr, { "and", cmp_expr } ;
cmp_expr      = add_expr, [ cmp_op, add_expr ] ;    (* a second cmp_op: P067 *)
add_expr      = mul_expr, { add_op, mul_expr } ;
mul_expr      = postfix_expr, { mul_op, postfix_expr } ;
cmp_op        = "==" | "!=" | ".<" | ".>" | "<=" | ">=" ;
add_op        = "+" | "-" | "+!" | "-!" | "+?" | "-?" ;
mul_op        = "*" | "/" | "*!" | "/!" | "*?" | "/?" ;

postfix_expr  = unary, { postfix } ;            (* postfix takes the whole prefix chain *)
unary         = prefix_op, unary | primary ;
prefix_op     = "-" | "-?" | "-!" | "not" ;
postfix       = ~"?" | ~"!" | ~"@"
              | ~".", field_sel
              | ~"[", expr, "]", [ ~"?" | ~"!" ]
              | ~"(", [ arg_list ], ")" ;       (* only after NAME "." NAME: P072 *)
field_sel     = NAME | digit, { digit } ;       (* digits fit a u32; else, or nothing: P079 *)

primary       = hinted
              | "true" | "false" | "none"
              | ( "some" | "ok" | "er" | "data" | "error" ), payload
              | "atom", NAME                          (* P042 *)
              | "term", NAME, payload                 (* P043 *)
              | "enum", "{", expr, [ "," ], "}"       (* P071 *)
              | "icall", NAME, ~"(", [ arg_list ], ")"  (* P040, P041; spaced "(": P021 *)
              | dec_number | hex_number               (* no sign: "-" is prefix_op *)
              | string_lit                            (* D039 *)
              | NAME, [ ~"(", [ arg_list ], ")" ]     (* a call only when glued *)
              | paren | collection ;                  (* any other bracket: P081 *)
payload       = postfix_expr ;                  (* a binary operator after it: P065 *)
arg_list      = arg, { ",", arg }, [ "," ] ;
arg           = [ "ref" | "mut" | "out" ], expr ;
paren         = "(", [ expr, { ",", expr }, [ "," ] ], ")" ;
                (* one element, no comma: grouping; else a tuple; () is unit *)

(* Hints  (literal.rs:25-110) *)
hinted        = ":", type, "/", ( hint_literal | postfix_expr ) ;   (* no "/": D049 *)
hint_literal  = [ "-", ~ ], dec_number | hex_number | string_lit
              | "true" | "false" | "none"
              | ( "some" | "ok" | "er" | "data" | "error" ), payload
              | "atom", NAME | "term", NAME, payload     (* P042, P043 *)
              | "enum", "{", expr, [ "," ], "}"
              | paren | collection ;
                (* chosen by the first token; see "What a hint covers" below *)

(* Collections  (literal.rs:260-737) *)
collection    = list | set | map | struct | table | tensor ;
list          = "[", [ expr, { ",", expr }, [ "," ] ], "]" ;
set           = "#{", [ expr, { ",", expr }, [ "," ] ], "}" ;
map           = "%{", [ entry, { ",", entry }, [ "," ] ], "}" ;
entry         = expr, "=", expr ;               (* D017 *)
struct        = "{", [ sfield, { ",", sfield }, [ "," ] ], "}" ;
sfield        = NAME, "=", expr ;               (* D018, D038 *)
table         = "{|", [ ROWSEPS ], [ header_row, { ROWSEPS, data_row }, [ ROWSEPS ] ], "|}" ;
ROWSEPS       = ( newline | ";" ), { newline | ";" } ;
header_row    = NAME, { ",", NAME }, [ "," ] ;  (* D031, D034, D038 *)
data_row      = expr, { ",", expr }, [ "," ] ;  (* D030, D033 *)
tensor        = "[|", [ shape_header ], [ tensor_body ], "|]" ;   (* D040, D041 *)
shape_header  = digit_run, { digit_run }, "|" ;
tensor_body   = tensor_row, { comma_run, tensor_row } ;   (* D023, D033 *)
tensor_row    = expr, { expr } ;                (* elements end where fixity says *)
comma_run     = ",", { "," } ;

(* ===== Types ===== *)
type          = ? the datalit type grammar, datalit-ebnf.md, parsed by the same
                  code (datalove-datalit/src/parser/type_hint.rs:55-268) ? ;
                (* a type_name there is an alias or a type parameter here,
                   which F064 refuses when it names neither. There are no
                   function types and no type application. *)

(* ===== Lexical  (bcts lexer.rs, parser_util.rs) ===== *)
word          = wchar, { wchar } ;              (* [A-Za-z0-9_] or Unicode alphanumeric *)
NAME          = ? a word not beginning with an ASCII digit ? ;  (* non-ASCII: P075 *)
value_name    = NAME - ( "true" | "false" | "none" | "some" | "ok" | "er" | "data"
                       | "error" | "atom" | "term" | "enum" | "not" | "icall" ) ;
param_name    = value_name - ( "mut" | "out" | "ref" | "const" ) ;
type_name     = NAME - ( scalar type keyword | "atom" | "term" | "enum" ) ;
                (* each exclusion is P064, and the name is still taken *)
dec_number    = digit_run, [ ".", digit_run ], [ ( "e" | "E" ), [ "+" | "-" ], digit_run ] ;
                (* glued throughout; gaps: P052; letters on the end: P053 *)
hex_number    = "0", ( "x" | "X" ), hex_run ;
string_lit, digit_run, hex_run, comment, newline
              = ? as in the datalit grammar ? ;
```

## Rules outside the grammar

**Lines and blocks.**

- A statement ends at an `EOL`, so `let x = 1 +` then `2` on the next line is
  P081 and then P001, while the same split inside `(1 +` ... `2)` is one
  statement. A `;` is a line break, so a run of them is blank lines.
- `end`, `else` and `case` are recognised only at the start of a line. An
  `end fun` inside an open `if` is P001 for that line and then P034 for both
  blocks.
- `inline` is only the whole first line of a function body.
- `require`, `import`, `fun`, `native fun` and `type` are declarations, read
  only at the top level. Inside a block each is P074, though it is still parsed
  so that what is wrong inside it is reported too.
- In a module the top level holds declarations and `const` only; any other
  statement is P077 ([spec Section 9.1](botspec.md#user-content-91-hierarchy)).
  A rider interface holds `native fun` and `type` only; anything else is P084
  ([spec Section 9.5](botspec.md#user-content-95-native-riders)). Each is
  parsed by its own query, `parse_rider_interface`, and checked by
  `check_rider_interface` in `datalove-datafun-compiler`, whose accumulated
  diagnostics are the interface's.

**Spacing.** The glue test decides fixity
([spec Section 2.4](botspec.md#user-content-24-spacing)):

- A sigil binary operator is glued on both sides or on neither. `a -b` and
  `a- b` end the expression, and what follows is P021. Word operators
  (`and`, `or`, `xor`) have no such rule.
- A postfix operator, `? ! @ . [ (`, is glued on its left: `p . 0`, `x ?`,
  `f (1)` and `u8 .max(1)` are P021. After an index, `?` and `!` are glued to
  the `]`, so `xs[i]!=3` lexes `!=` and is P021. The steps of a `set` target
  are held to the same rule, as is the `(` after `icall`'s name.
- `NAME(` is a call only when glued.
- A prefix operator has no spacing rule: `- x` is a negation.
- A number is one glued run (P052, P053).
- After `: T /`, a `-` glued to a decimal word is the literal's sign, so
  `: i32 / -5` is the literal -5. Anywhere else, `-5` is the negation of `5`.
- In a tensor row, elements are separated by this rule alone:
  `[| 1 -2 |]` has two elements and `[| a + b c |]` two.

**Operators and payloads.**

- Comparisons do not chain. A second comparison at the same level is P067
  and parsing goes on left-associative; `(a == b) == c` and
  `a == b and c != d` are fine.
- The payload of `some`, `ok`, `er`, `data`, `error` and `term Name` is a
  postfix expression, and any binary operator straight after it is P065,
  whatever its precedence: `some 1 == x` is P065 and `x == some 1` is not.
- Postfix binds looser than prefix: `-x@` is `(-x)@` and `not a.b` is
  `(not a).b`. So `-u8.max_value()` is P072, the base being `-u8`.
- A glued `(` after anything but `NAME.NAME` is P072: `a.b.c(1)`, `f(x)(1)`.
- **What a hint covers.** A hint is a primary, so a binary operator after it
  applies to the hinted expression: `: u32 / x + 1` is `(: u32 / x) + 1`. Under
  the `hint_literal` alternative the hint sits on the literal and a later
  postfix is outside it (`: u32 / 5@` hints the `5`); under the `postfix_expr`
  alternative the hint covers the postfixes (`: u32 / o?` hints what `?`
  produces). Which alternative applies is decided by the first token after the
  `/`, so `: u32 / (o)?` hints `o`.

**Bindings and signatures.**

- A one-name tuple pattern needs its comma, `(a,)`; `(a)` is P070. A nested
  pattern is P069, and a name bound twice is D038.
- A binding `if` takes no part in an `else if` chain on either side (P073),
  and `else |e|` cannot be followed by `if` (P066).
- A line before the first `case` of a `match` is P048.
- The `where` clause is on the signature's line; on the next one it is P001.
- Every declared name is checked against the reserved words (P064),
  including the name a struct pattern's `{f}` shorthand binds.

**Recovery.** P021 is suppressed after any earlier error in the same
sub-parser, so `fun f(a, b: u32): u32 extra` reports the parameter and not
`extra`.

## Productions and their typing rules

| Production | Typing rules |
|---|---|
| `hinted` | `Syn-TypedExpr`, `Check-Hinted` |
| literals, collections, constructors | the datalit rules, [inherited](datafun-typing-rules.md#user-content-inherited-rules) |
| `add_op`, `mul_op`, `cmp_op`, `and`, `or`, `xor` | `Pair`, `Syn-Arith`, `Syn-Ord`, `Syn-Eq`, `Syn-Logic`, `Check-BinOp` |
| `prefix_op` | `Syn-Neg`, `Syn-NegChecked`, `Syn-NegOptional`, `Syn-Not`, `Check-Neg`, `Check-NegLit` |
| postfix `?`, `!` | `Syn-TryOption`, `Syn-TryResult` |
| postfix `@` | `Syn-Adapt`, `Check-Adapt` |
| postfix `.`, `[ ]` | `Syn-TupleProj`, `Syn-FieldProj`, `Syn-IndexOption`, `Syn-IndexResult` |
| calls, `icall` | `Syn-Call`, `Syn-GenericCall`, `Check-GenericCall`, `Syn-Intrinsic` |
| `binding` | `Pat-*`, `Let`, `Var-Uninit`, `Const` |
| `set_stmt` | `Place-*`, `Set`, `Set-Compound` |
| `if_stmt`, `loop_stmt`, `match_stmt`, `ret_stmt` | `If`, `If-Some`, `If-Ok`, `Loop`, `Loop-While`, `Match`, `Ret`, `Ret-Void` |
| `fun_def` | `Fun` |

## Differences from datalit

Datafun parses types with datalit's own code, and reuses its reserved-word
tables, duplicate check (D038), string decoding and table and tensor
splitting. Its value syntax is its own copy, in `literal.rs` and `expr.rs`,
and differs:

| Form | Datalit | Datafun |
|---|---|---|
| Payload of `some`, `ok`, `er`, `data`, `error`, `term` | `full_expr` | a postfix expression; a binary operator after it is P065 |
| A sign | a glued `-` on a decimal literal; `-0x10` is D050 | part of the literal only right after a hint; elsewhere prefix negation, so `-0x10` is legal |
| What a hint covers | one datalit expression | a literal form, or a postfix expression wrapped in a `Hinted` node |
| `(a, b)` | `AnonTuple` | `Tuple` unhinted, `AnonTuple` under a hint |
| `: T / (x)` | `Group` when under another hint | `Hinted(x)` |
| Tensor elements and table cells | datalit expressions | datafun expressions, with fixity ending tensor elements |
| Unmatched brackets | D054, D055 | P050, P051 |

The same faults get different codes in the two parsers:

| Fault | Datalit | Datafun |
|---|---|---|
| Malformed number; numeric suffix | D019 | P052; P053 |
| `enum` literal without `{` | D037 | P071 |
| `atom`, `term` without a name | D035, D036 | P042, P043 |
| An unexpected token | D020, D018 | P080, P081 |
| A non-ASCII name | D043 | P075 |

The rest are the same in both: a hint without its `/` (D049), a struct field
without its name or `=` (D018), a map entry without `=` (D017), D030, D031,
D033, D034, D038 to D041, tensor-shape D023, and every type diagnostic.

## Divergences

Places the parser and [the spec](botspec.md) disagree, left as they are.

- **A variable named `ref`, `mut` or `out` cannot be passed.** Section 2.1
  reserves those words only for parameters, so `let out = 1` is legal, but an
  argument starting with one is read as a mode marker (`expr.rs`,
  `parse_arg_mode`): `call f(out)` is P081. The reserved words are there to keep
  parsing simple, and this can be disambiguated some other way when it
  matters.
