# Datalit runtime type representation

Most datalit types are value types and stored inline.

Datalit array sizes and capacities are `index`,
which is `u32` by default for multi-arch compatibility
(`u64` with the `index-64` feature).




## Unit

`()`.
Zero-sized (as are `{}` and atoms).




## Scalars

`bool`,
`u8`, `u16`, `u32`, `u64`,
`i8`, `i16`, `i32`, `i64`,
`index`, `offset`,
`f32`, and `f64`
have their obvious machine representation.
`bool` is one byte.




## Bigints

Int uses a simple gmp-inspired representation:
a pointer to u32 "limb" data plus
size, sign and capacity.

```rust
#[repr(C)]
pub struct Int {
    // "limbs"
    pub data: *const u32,
    // abs(size_and_sign) == number of limbs;
    // sign(size_and_sign) == sign of self
    pub size_and_sign: i32,
    // Limbs allocated.
    pub capacity: Index,
}
```
