# Datalit runtime type representation

Most datalit types are value types and stored inline.

Datalit array sizes and capacities are limited to `u32`
for multi-arch compatibility.




## Unit

`()`.
Byte-sized.




## Scalars

`bool`,
`u8`, `u32`, `u64`, `u128`,
`i8`, `i32`, `i64`, `i128`,
and `f32`
have their obvious machine representation.




## Bigints

Int uses a simple gmp-inspired representation:
a pointer to u32 "limb" data plus
size, sign and capacity

```rust
#[repr(C)]
pub struct Int {
    // "limbs"
    pub data: *const u32,
    // abs(size_and_sign) == number of limbs;
    // sign(size_and_sign) == sign of self
    pub size_and_sign: i32,
    // Limbs allocated.
    pub capacity: u32,
}
```
