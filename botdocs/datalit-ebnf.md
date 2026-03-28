# Datalit Grammar (EBNF)

```ebnf
(* ===== Top Level ===== *)
datalit        = ws, [ type_hint ], expr, ws ;

type_hint      = ":", ws, type, ws, "/" ;

(* ===== Types ===== *)
type           = option_type | result_type | list_type | tuple_type
               | struct_type | table_type | map_type | set_type
               | tensor_type | atom_type | term_type | enum_type
               | scalar_type | type_alias ;

scalar_type    = "bool" | "u8" | "u16" | "u32" | "u64"
               | "i8" | "i16" | "i32" | "i64"
               | "index" | "offset" | "f32" | "f64"
               | "int" | "string" | "data" | "error" ;

option_type    = "?", type ;
result_type    = "!", type ;
list_type      = "[", ws, type, ws, "]" ;
tuple_type     = "(", ws, [ type_list ], ws, ")" ;
struct_type    = "{", ws, [ type_field_list ], ws, "}" ;
table_type     = "\u27E6", ws, type_field_list, ws, "\u27E7" ;
map_type       = "\u2987", ws, type, ws, "\u21A6", ws, type, ws, "\u2988" ;
set_type       = "\u2983", ws, type, ws, "\u2984" ;
tensor_type    = "\u27EA", ws, type, ws, ",", ws, int_lit, ws, "\u27EB" ;
atom_type      = "atom", ws, ident ;
term_type      = "term", ws, ident, ws, type ;
enum_type      = "enum", ws, "{", ws, enum_variant_list, ws, "}" ;
enum_variant_list = enum_variant, { ws, ",", ws, enum_variant }, [ ws, "," ] ;
enum_variant   = atom_type | term_type ;
type_alias     = ident ;

type_list      = type, { ws, ",", ws, type }, [ ws, "," ] ;
type_field_list= type_field, { ws, ",", ws, type_field }, [ ws, "," ] ;
type_field     = ident, ws, ":", ws, type ;

(* ===== Expressions ===== *)
expr           = none_lit | bool_lit | numeric_lit | string_lit
               | option_expr | result_expr | existential_expr
               | tuple_expr | list_expr | struct_expr
               | map_expr | set_expr
               | tensor_expr | table_expr ;

(* Literals *)
none_lit       = "none" ;
bool_lit       = "true" | "false" ;
numeric_lit    = float_lit | hex_lit | int_lit ;
int_lit        = [ "-" ], digit, { digit } ;
float_lit      = [ "-" ], digit, { digit }, ".", digit, { digit } ;
hex_lit        = [ "-" ], "0", ( "x" | "X" ), hex_digit, { hex_digit } ;
string_lit     = '"', { string_char }, '"' ;

(* Option/Result/Existential constructors *)
option_expr    = "some", ws, full_expr ;
result_expr    = ( "ok" | "er" ), ws, full_expr ;
existential_expr = ( "data" | "error" ), ws, full_expr ;

(* Container expressions *)
tuple_expr     = "(", ws, [ expr_list ], ws, ")" ;
list_expr      = "[", ws, [ expr_list ], ws, "]" ;
struct_expr    = "{", ws, [ field_list ], ws, "}" ;
map_expr       = "\u2987", ws, [ entry_list ], ws, "\u2988" ;
set_expr       = "\u2983", ws, [ expr_list ], ws, "\u2984" ;

expr_list      = full_expr, { ws, ",", ws, full_expr }, [ ws, "," ] ;
full_expr      = [ type_hint ], expr ;
field_list     = field, { ws, ",", ws, field }, [ ws, "," ] ;
field          = ident, ws, "=", ws, full_expr ;
entry_list     = entry, { ws, ",", ws, entry }, [ ws, "," ] ;
entry          = full_expr, ws, "\u21A6", ws, full_expr ;

(* Tensor: \u27EA data \u27EB with multi-comma separators *)
tensor_expr    = "\u27EA", ws, [ tensor_body ], ws, "\u27EB" ;
tensor_body    = tensor_group, { multi_comma, ws, tensor_group }, [ multi_comma ] ;
tensor_group   = tensor_row, { ws, ",", ws, tensor_row } ;
tensor_row     = full_expr, { ws, full_expr } ;  (* space-separated within row *)
multi_comma    = ",", ",", { "," } ;  (* ,, for 3D, ,,, for 4D, etc. *)

(* Table: header row + data rows *)
table_expr     = "\u27E6", ws, table_header, table_rows, ws, "\u27E7" ;
table_header   = ident, { ws, ",", ws, ident }, row_sep ;
table_rows     = { table_row } ;
table_row      = full_expr, { ws, ",", ws, full_expr }, [ row_sep ] ;
row_sep        = ";" | newline ;

(* ===== Lexical ===== *)
ident          = alpha, { alpha | digit | "_" } ;
alpha          = "a"-"z" | "A"-"Z" | "_" ;
digit          = "0"-"9" ;
hex_digit      = digit | "a"-"f" | "A"-"F" ;
string_char    = escape_seq | ? any char except '"' and '\' ? ;
escape_seq     = "\", ( '"' | "\" | "n" | "r" | "t" | "0"
               | ( "x", hex_digit, hex_digit )
               | ( "u", "{", hex_digit, { hex_digit }, "}" ) ) ;

(* Whitespace and comments *)
ws             = { whitespace | comment } ;
whitespace     = " " | "\t" | "\n" | "\r" ;
comment        = line_comment | block_comment ;
line_comment   = "//", { ? any char except newline ? }, newline ;
block_comment  = "/*", { ? any char or nested block_comment ? }, "*/" ;
newline        = "\n" | "\r\n" ;
```

## Unicode Bracket Reference

| Symbol | Codepoint | Name | Use |
|--------|-----------|------|-----|
| `⦇` `⦈` | U+2987, U+2988 | Z notation image bracket | Map |
| `⦃` `⦄` | U+2983, U+2984 | White curly bracket | Set |
| `⟦` `⟧` | U+27E6, U+27E7 | Double square bracket | Table |
| `⟪` `⟫` | U+27EA, U+27EB | Double angle bracket | Tensor |
| `↦` | U+21A6 | Rightwards arrow from bar | Map key-value separator |
| `≤` `≥` | U+2264, U+2265 | Less/greater-or-equal | Comparison |
| `≡` `≢` | U+2261, U+2262 | Identical / not identical | Equality |

## Design Notes

1. **Type hints precede values**: `: Type / value` syntax annotates the following expression
2. **Prefix notation for option/result types**: `?T` for Option, `!T` for Result
3. **Keywords for constructors**: `some`, `ok`, `er`, `none`, `data`, `error`. Constructor payloads are `full_expr` (may include inline type hints, e.g. `some : u32 / 42`)
4. **Struct fields use `=`** in expressions but `:` in type hints
5. **Maps use `↦`** to separate keys from values: `⦇ key ↦ value ⦈`
6. **Tensors**: `⟪ data ⟫` with multi-comma separators; spaces separate innermost elements, `,` separates rows, `,,` separates slabs, etc. Shape is inferred from structure. Trailing commas preserve rank when outermost dimension is 1.
7. **Tables**: double square bracket delimiters `⟦ ... ⟧`, header then data rows
8. **Trailing commas** allowed everywhere
9. **Block comments** can nest
