# A spec for bots

I'm falling behind on keeping canonical docs up to date:

- README.md
- demo-datalit.dlt
- demo-datafun-script.dfs
- demo-datafun-module.dfm
- notes/anytype.md
- notes/arrays.md
- notes/bitmanip.md
- notes/module-system.md
- notes/panicking.md
- notes/repl-ui.md
- notes/script-semantics.md
- notes/sigil-assignments.md
- notes/typing-rules.md
- notes/zipper-heaps.md

We should keep a bot-owned spec that actually is up to date
for bot purposes.

Put it in notes/botspec.md

Read my existing partial specs linked above,
but be aware that some may be out of date or represent future designs.

Make a plan to incrementally implement this document,
starting with datalit syntax, typechecking etc.,
datafun etc.
Use an organization that is similar to what my own docs suggest.

At each step compare the spec to the implementation and make note.
Only include features that have some implementation progress.
Use an appendix to note documented features that don't exist at all
or are completely wrong compared to the implementation.
