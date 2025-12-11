// int.dfm - bigint utilities
//
// NOTE: Most int functions cannot be implemented without runtime support
// because int is a linear type (move semantics). Operations like comparison
// consume the value, preventing reuse in the return statement.
//
// Functions like abs, signum, max, min, clamp would require either:
// - Borrow/reference semantics for comparisons
// - A clone/copy operation for int
// - Runtime intrinsics
