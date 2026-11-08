# Rust Style: the Prelude and Powerletters

The conventions for Rust in this workspace that an editor will not enforce.
Written November 2026, when the tree was converted to powerletters.

## The rustmax prelude

Every crate that depends on `rmx` (rustmax) imports its prelude in each file
that needs anything from it:

```rust
use rmx::prelude::*;
```

Ten crates do not depend on `rmx` at all, by design: `datalove`,
`datalove-buildinfo`, `datalove-datafun-intrinsics`, `datalove-diagnostic`,
`datalove-diagnostic-codes`, `datalove-pkg-manifest`, `datalove-rtdt`,
`datalove-rti`, `datalove-vio` and `sys/std/rider`. They spell everything the
std way. Build scripts do not have `rmx` either.

Where a file glob-imports another module that also exports `String`, `Option`
or `Result` (the runtime's C types in `datalove_rtdt`, for instance), the two
globs collide. Name the local ones explicitly; an explicit import shadows a
glob:

```rust
use rmx::prelude::*;
use datalove_rtdt::*;
use datalove_rtdt::{Option, Result, String};
```

## Powerletters

The prelude brings in [powerletters](https://docs.rs/powerletters): one-letter
methods for the most common calls. Use them in place of the std spelling.

| Write    | Instead of      | Notes |
|----------|-----------------|-------|
| `x.C()`  | `x.clone()`     | Resolves exactly as `clone` does. |
| `x.S()`  | `x.to_string()` | |
| `x.O()`  | `x.to_owned()`  | |
| `x.X()`  | `x.unwrap()`    | On a `Result`, the error must be `Display`. |
| `r.I()`  | `let _ = r;`    | `Result` only. |

Where the std spelling stays:

- **`unwrap` with an error that is only `Debug`.** `X` formats the error with
  `Display`, so it does not compile there. `unwrap` is right for those, and
  for `Option::unwrap` in a `const` context, which `X` is not.
- **`expect("...")`.** The message says why the value is there; keep it. `X`
  is for the case so plain that a message would only repeat the code.
- **`let _ = ...` on anything but a `Result`.** Discarding a salsa query's
  value to force it, silencing an unused binding, or dropping a tuple all keep
  `let _`. `I` takes only a `Result`, which is its point: it cannot quietly
  swallow a future or a value that should have been used.
- **`Rc::clone(&x)` and other fully qualified calls.** The qualified form is
  there to say what is being cloned.
- **Inside a `macro_rules!` body that expands in another crate,** unless that
  crate imports the prelude too.
