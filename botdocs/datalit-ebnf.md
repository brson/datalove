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
table_type     = "{|", ws, type_field_list, ws, "|}" ;
map_type       = "map", ws, "<", ws, type, ws, ",", ws, type, ws, ">" ;
set_type       = "set", ws, "<", ws, type, ws, ">" ;
tensor_type    = "tensor", ws, "<", ws, type, ws, ",", ws, int_lit, ws, ">" ;
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
map_expr       = "map", ws, "{", ws, [ entry_list ], ws, "}" ;
set_expr       = "set", ws, "{", ws, [ expr_list ], ws, "}" ;

expr_list      = full_expr, { ws, ",", ws, full_expr }, [ ws, "," ] ;
full_expr      = [ type_hint ], expr ;
field_list     = field, { ws, ",", ws, field }, [ ws, "," ] ;
field          = ident, ws, "=", ws, full_expr ;
entry_list     = entry, { ws, ",", ws, entry }, [ ws, "," ] ;
entry          = full_expr, ws, "=", ws, full_expr ;

(* Tensor: shape followed by elements *)
tensor_expr    = "tensor", ws, shape, ws, tensor_data ;
shape          = "[", ws, [ dim_list ], ws, "]" ;
dim_list       = int_lit, { ws, ",", ws, int_lit }, [ ws, "," ] ;
tensor_data    = "[", ws, tensor_elements, ws, "]" ;
tensor_elements= tensor_row, { ws, ",", ws, tensor_row } ;
tensor_row     = full_expr, { ws, full_expr } ;  (* space-separated within row *)

(* Table: header row + data rows *)
table_expr     = "{|", ws, table_header, table_rows, ws, "|}" ;
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

## Design Notes

1. **Type hints precede values**: `: Type / value` syntax annotates the following expression
2. **Prefix notation for option/result types**: `?T` for Option, `!T` for Result
3. **Keywords for constructors**: `some`, `ok`, `er`, `none`, `data`, `error`, `map`, `set`, `tensor`. Constructor payloads are `full_expr` (may include inline type hints, e.g. `some : u32 / 42`)
4. **Struct fields use `=`** in expressions but `:` in type hints
5. **Tensors**: shape in first brackets, data in second; rows comma-separated, elements within rows space-separated
6. **Tables**: pipe-brace delimiters `{| ... |}`, header then data rows
7. **Trailing commas** allowed everywhere
8. **Block comments** can nest
