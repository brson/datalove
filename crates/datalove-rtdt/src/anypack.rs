/// Bitpacking experiments for @data/@error.
///
/// Since we don't expect dynamic types to be instantiated a lot,
/// let's be extravagant and allocate 128 bits to them.
///
/// We need to encode the type and value of every type in this.
/// Some less important cases will likely have to punt to a pointer to a tydesc
/// and/or pointer to an intermediate allocation.
///
/// This does not have to be super efficient -
/// the biggest requirement is that it is easy to understand and explain,
/// as our ABI is well-specified and interoperable.

#![allow(unused)]

struct Data(u128);
