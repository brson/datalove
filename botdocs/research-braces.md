# Brace Style Research

Brainstorming matched brace/bracket styles for the language.

## ASCII Combinations

**Earmuff variants** (inner decoration):
- `(| |)` `{| |}` `[| |]` `<| |>` - pipes
- `(: :)` `{: :}` `[: :]` `<: :>` - colons
- `(# #)` `{# #}` `[# #]` `<# #>` - hashes
- `(! !)` `{! !}` `[! !]` `<! !>` - bangs
- `(? ?)` `{? ?}` `[? ?]` `<? ?>` - questions
- `(@ @)` `{@ @}` `[@ @]` `<@ @>` - ats
- `(* *)` `{* *}` `[* *]` `<* *>` - stars
- `(% %)` `{% %}` `[% %]` `<% %>` - percents
- `(= =)` `{= =}` `[= =]` `<= =>` - equals (careful with `<=`)

**Doubled brackets:**
- `(( ))` `{{ }}` `[[ ]]` `<< >>`

**Prefix/suffix sigils:**
- `#( )` `#{ }` `#[ ]` - hash prefix
- `@( )` `@{ }` `@[ ]` - at prefix
- `$( )` `${ }` `$[ ]` - dollar prefix
- `&( )` `&{ }` `&[ ]` - ampersand prefix

## Language Precedents

| Language | Syntax | Meaning |
|----------|--------|---------|
| F# | `[| |]` | arrays |
| F# | `<@ @>` `<@@ @@>` | code quotations |
| OCaml | `[| |]` | arrays |
| Haskell | `[| |]` | Template Haskell quasi-quotes |
| Haskell | `{- -}` | block comments |
| Clojure | `#{ }` | sets |
| Clojure | `#( )` | anonymous functions |
| Perl | `qw( )` `qw{ }` `qw[ ]` | word lists (any bracket works) |
| Ruby | `%w[ ]` `%i{ }` `%r( )` | special literals |
| Mathematica | `(* *)` | comments |
| Mathematica | `[[ ]]` | part extraction |
| Mathematica | `<< >>` | contexts/packages |
| Agda/Lean | `{ }` | instance arguments |
| Lean | `< >` | anonymous constructors |
| Swift | `<# #>` | editor placeholders |
| Lua | `[[ ]]` `[=[ ]=]` | raw strings (with = for nesting levels) |
| Shell | `$(( ))` | arithmetic |
| Shell | `${ }` | parameter expansion |
| JSX/XML | `{/* */}` | embedded comments |
| Embedded langs | `<% %>` `<%= %>` | ERB, EJS, ASP templates |

## Unicode Brackets

**Mathematical:**
- `< >` - angle brackets (U+27E8/9)
- `<< >>` - double angle brackets
- `[[ ]]` - double square / semantic brackets
- white tortoise shell
- flattened parenthesis

**Floor/Ceiling:**
- ceiling
- floor

**CJK:**
- corner brackets
- white corner brackets
- lenticular brackets
- tortoise shell
- CJK angle brackets
- double angle (guillemets)

**Ornamental:**
- parenthesis ornaments
- angle ornaments
- curly ornaments

**Z-notation / formal methods:**
- white curly brackets
- white parenthesis
- image brackets
- binding brackets

**Half brackets:**
- top corners
- bottom corners

**Exotic:**
- double parentheses
- S-shaped bag delimiters
- square with quill

## Semantic Ideas

Potential meanings for different styles:

| Style | Possible semantics |
|-------|-------------------|
| `(| |)` | banana brackets - idioms/applicative |
| `[| |]` | envelope - quotation/reification |
| `{| |}` | record/struct literal |
| `<| |>` | template/generic instantiation |
| `(: :)` | type annotation context |
| `#{ }` | set literal |
| `#[ ]` | attribute/annotation |
| `@( )` | macro invocation |
| `[[ ]]` | denotational semantics |
| ceiling/floor | ceiling/floor (actual math) |
| `< >` | tuples, sequences, or kets |
