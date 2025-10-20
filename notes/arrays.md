## Heap-allocated strided nd-arrays

Heap allocated strided multidimensional arrays are called tensors.

Major influence is Julia.

Their rank (dimension) is static,
but shape, layout, and stride are dynamic.

- shape - rows x columns
- layout - row-major, column-major, RM transposed, CM transposed, maybe exotic layouts
- stride - rows/columns to skip in a particular view

```
: tensor<u32, 2> / tensor [
  1 2 3,
  4 5 6
]
```

```
: [|u32, 2|] / [|
  1 2 3,
  4 5 6
|]
```

```
|[ ]|

|( )|

|{ }|

|< >|

[| |]

(| |)

{| |}

<| |>

```


Compare to lists:

```
: [int] / [1]
```

## Alternative brace modifiers for ndarray syntax

Single-character modifiers:

### Hash/Pound `#`
```
#[ ]#    #( )#    #{ }#
[# #]    (# #)    {# #}
```
Pros: Grid-like appearance, keyboard accessible, not heavily overloaded in most languages
Cons: Used for comments/directives in some contexts

### Colon `:`
```
:[ ]:    :( ):    :{ }:
[: :]    (: :)    {: :}
```
Pros: Already associated with slicing/indexing, clean appearance
Cons: Might conflict with type annotations, range syntax

### Tilde `~`
```
~[ ]~    ~( )~    ~{ }~
[~ ~]    (~ ~)    {~ ~}
```
Pros: Wavy/undulating suggests multi-dimensionality, rarely used for core syntax
Cons: Less familiar, might look decorative rather than structural

### At sign `@`
```
@[ ]@    @( )@    @{ }@
[@ @]    (@ @)    {@ @}
```
Pros: Round, matrix-like appearance, distinct
Cons: Heavily used for decorators/annotations in Python/Java/etc

### Asterisk/Star `*`
```
*[ ]*    *( )*    *{ }*
[* *]    (* *)    {* *}
```
Pros: Suggests multiplication/cartesian products, pointer-like (memory-backed)
Cons: Very overloaded (multiplication, pointers, wildcards, unpacking)

### Percent `%`
```
%[ ]%    %( )%    %{ }%
[% %]    (% %)    {% %}
```
Pros: Two circles suggest dimensionality, distinct
Cons: Modulo operator, string formatting in some languages

### Equals `=`
```
=[ ]=    =( )=    ={ }=
[= =]    (= =)    {= =}
```
Pros: Horizontal lines suggest rows/grids
Cons: Too strongly suggests assignment/equality

### Dot `.`
```
.[ ].    .( ).    .{ }.
[. .]    (. .)    {. .}
```
Pros: Minimal, unobtrusive, suggests elements
Cons: Might be too subtle, conflicts with member access

### Caret `^`
```
^[ ]^    ^( )^    ^{ }^
[^ ^]    (^ ^)    {^ ^}
```
Pros: Suggests exponentiation/higher dimensions
Cons: XOR operator, sometimes line-start anchor in regex

Double-character modifiers:

### Double colon `::`
```
::[ ]::    ::[  ]::
```
Pros: Namespace separator feel, suggests "different kind of"
Cons: More verbose, might be too heavy

### Double hash `##`
```
##[ ]##
```
Pros: Very distinct, strong visual signal
Cons: Quite heavy visually

### Recommendations

Based on visual distinctness and avoiding overloaded symbols:

1. Hash `#` - `[# #]` or `#[ ]#`: Best balance of distinctness, non-overloaded meaning, and grid-like appearance
2. Tilde `~` - `[~ ~]` or `~[ ]~`: Unique, wavy suggests multi-dimensional, rarely conflicts
3. Colon `:` - `[: :]` or `:[ ]:`: Already has array-indexing connotations, clean

The postfix style `[# #]` might read better since the opening delimiter immediately signals "this is a bracket of some kind" before showing it's specialized.
