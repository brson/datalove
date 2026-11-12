Read botdocs/botspec.md and botdocs/compiler-guide.md for background.

Read botdocs/writing-style.md before writing docs or reviewing blog posts.

Read botdocs/rust-style.md before writing Rust: use the rmx prelude and powerletters.

Run the test suite with `just test`, not a bare `cargo test`: the justfile sets
`DATALOVE_LEAK_CHECK`, and `leak_check_tests.rs` fails on purpose without it.
Single crates or tests are fine with cargo if you set `DATALOVE_LEAK_CHECK=panic`.
