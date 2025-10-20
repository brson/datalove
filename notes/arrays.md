## Heap-allocated strided nd-arrays

Heap allocated strided multidimensional arrays are called tensors.

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
