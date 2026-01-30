Read botdocs/botspec.md and botdocs/compiler-guide.md for background.

Before reporting a task is complete, run `just test`;
If there are errors, either fix them, or tell me.

Unless it's just a documentation or research task:
then you don't need to run the full test suite.

But usually you should run `just test` before claiming
a task is done.

Don't leave fallback and compatibility code.
Don't write defensive code.

Before sealing test results with BLESS
be sure that the environment doesn't contain RUST_BACKTRACE -
that will corrupt the test output.
Unset the env var before running to fix.
