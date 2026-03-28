## Heap-allocated strided nd-arrays

Heap allocated strided multidimensional arrays are called tensors.

Major influence is Julia.

Their rank (dimension) is static,
but shape, layout, and stride are dynamic.

- shape - rows x columns
- layout - row-major, column-major, RM transposed, CM transposed, maybe exotic layouts
- stride - rows/columns to skip in a particular view

```datalove
: ⟪u32, 2⟫ / ⟪
  1 2 3,
  4 5 6
⟫
```

I think outer hash may be best but currently used for heap sigil.
Hash suggests matrixes. Could _also_ save this far actual matrix type.

```
: #[u32, 2]# / #[
  1 2 3,
  4 5 6
]#
```

Outer percent also good:

```datalove
: %[u32, 2]% / %[
  1 2 3,
  4 5 6
]%
```


## Bracket-modifier combination examples

### `|[ ]|` (pipe prefix)

```datalove
: |[u32, 2]| / |[
  1 2 3,
  4 5 6
]|

fun foo(a: !u32): !u32
  if a |val: |[u32, 2]|| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `⟪ ⟫` (pipe postfix)

```datalove
: ⟪u32, 2⟫ / ⟪
  1 2 3,
  4 5 6
⟫

fun foo(a: !u32): !u32
  if a |val: ⟪u32, 2⟫| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `#[ ]#` (hash prefix)

```
: #[u32, 2]# / #[
  1 2 3,
  4 5 6
]#

fun foo(a: !u32): !u32
  if a |val: #[u32, 2]#| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `[# #]` (hash postfix)

```datalove
: [#u32, 2#] / [#
  1 2 3,
  4 5 6
#]

fun foo(a: !u32): !u32
  if a |val: [#u32, 2#]| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `:[ ]:` (colon prefix)

```datalove
: :[u32, 2]: / :[
  1 2 3,
  4 5 6
]:

fun foo(a: !u32): !u32
  if a |val: :[u32, 2]:| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `[: :]` (colon postfix)

```datalove
: [:u32, 2:] / [:
  1 2 3,
  4 5 6
:]

fun foo(a: !u32): !u32
  if a |val: [:u32, 2:]| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `~[ ]~` (tilde prefix)

```datalove
: ~[u32, 2]~ / ~[
  1 2 3,
  4 5 6
]~

fun foo(a: !u32): !u32
  if a |val: ~[u32, 2]~| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `[~ ~]` (tilde postfix)

```datalove
: [~u32, 2~] / [~
  1 2 3,
  4 5 6
~]

fun foo(a: !u32): !u32
  if a |val: [~u32, 2~]| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `@[ ]@` (at prefix)

```datalove
: @[u32, 2]@ / @[
  1 2 3,
  4 5 6
]@

fun foo(a: !u32): !u32
  if a |val: @[u32, 2]@| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `[@ @]` (at postfix)

```datalove
: [@u32, 2@] / [@
  1 2 3,
  4 5 6
@]

fun foo(a: !u32): !u32
  if a |val: [@u32, 2@]| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `*[ ]*` (star prefix)

```datalove
: *[u32, 2]* / *[
  1 2 3,
  4 5 6
]*

fun foo(a: !u32): !u32
  if a |val: *[u32, 2]*| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `[* *]` (star postfix)

```datalove
: [*u32, 2*] / [*
  1 2 3,
  4 5 6
*]

fun foo(a: !u32): !u32
  if a |val: [*u32, 2*]| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `%[ ]%` (percent prefix)

```datalove
: %[u32, 2]% / %[
  1 2 3,
  4 5 6
]%

fun foo(a: !u32): !u32
  if a |val: %[u32, 2]%| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `[% %]` (percent postfix)

```datalove
: [%u32, 2%] / [%
  1 2 3,
  4 5 6
%]

fun foo(a: !u32): !u32
  if a |val: [%u32, 2%]| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `=[ ]=` (equals prefix)

```datalove
: =[u32, 2]= / =[
  1 2 3,
  4 5 6
]=

fun foo(a: !u32): !u32
  if a |val: =[u32, 2]=| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `[= =]` (equals postfix)

```datalove
: [=u32, 2=] / [=
  1 2 3,
  4 5 6
=]

fun foo(a: !u32): !u32
  if a |val: [=u32, 2=]| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `.[ ].` (dot prefix)

```datalove
: .[u32, 2]. / .[
  1 2 3,
  4 5 6
].

fun foo(a: !u32): !u32
  if a |val: .[u32, 2].| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `[. .]` (dot postfix)

```datalove
: [.u32, 2.] / [.
  1 2 3,
  4 5 6
.]

fun foo(a: !u32): !u32
  if a |val: [.u32, 2.]| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `^[ ]^` (caret prefix)

```datalove
: ^[u32, 2]^ / ^[
  1 2 3,
  4 5 6
]^

fun foo(a: !u32): !u32
  if a |val: ^[u32, 2]^| {
    ret val
  } else |err| {
    ret err
  }
end fun
```

### `[^ ^]` (caret postfix)

```datalove
: [^u32, 2^] / [^
  1 2 3,
  4 5 6
^]

fun foo(a: !u32): !u32
  if a |val: [^u32, 2^]| {
    ret val
  } else |err| {
    ret err
  }
end fun
```


Compare to lists:

```datalove
: [int] / [1]
```

## Alternative brace modifiers for ndarray syntax

Single-character modifiers:

### Hash/Pound `#`
```datalove
#[ ]#    #( )#    ⦃ ⦄#
[# #]    (# #)    {# #}
```
Pros: Grid-like appearance, keyboard accessible, not heavily overloaded in most languages
Cons: Used for comments/directives in some contexts

### Colon `:`
```datalove
:[ ]:    :( ):    :{ }:
[: :]    (: :)    {: :}
```
Pros: Already associated with slicing/indexing, clean appearance
Cons: Might conflict with type annotations, range syntax

### Tilde `~`
```datalove
~[ ]~    ~( )~    ~{ }~
[~ ~]    (~ ~)    {~ ~}
```
Pros: Wavy/undulating suggests multi-dimensionality, rarely used for core syntax
Cons: Less familiar, might look decorative rather than structural

### At sign `@`
```datalove
@[ ]@    @( )@    @{ }@
[@ @]    (@ @)    {@ @}
```
Pros: Round, matrix-like appearance, distinct
Cons: Heavily used for decorators/annotations in Python/Java/etc

### Asterisk/Star `*`
```datalove
*[ ]*    *( )*    *{ }*
[* *]    (* *)    {* *}
```
Pros: Suggests multiplication/cartesian products, pointer-like (memory-backed)
Cons: Very overloaded (multiplication, pointers, wildcards, unpacking)

### Percent `%`
```datalove
%[ ]%    %( )%    ⦇ ⦈%
[% %]    (% %)    {% %}
```
Pros: Two circles suggest dimensionality, distinct
Cons: Modulo operator, string formatting in some languages

### Equals `=`
```datalove
=[ ]=    =( )=    ={ }=
[= =]    (= =)    {= =}
```
Pros: Horizontal lines suggest rows/grids
Cons: Too strongly suggests assignment/equality

### Dot `.`
```datalove
.[ ].    .( ).    .{ }.
[. .]    (. .)    {. .}
```
Pros: Minimal, unobtrusive, suggests elements
Cons: Might be too subtle, conflicts with member access

### Caret `^`
```datalove
^[ ]^    ^( )^    ^{ }^
[^ ^]    (^ ^)    {^ ^}
```
Pros: Suggests exponentiation/higher dimensions
Cons: XOR operator, sometimes line-start anchor in regex

Double-character modifiers:

### Double colon `::`
```datalove
::[ ]::    ::[  ]::
```
Pros: Namespace separator feel, suggests "different kind of"
Cons: More verbose, might be too heavy

### Double hash `##`
```datalove
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
