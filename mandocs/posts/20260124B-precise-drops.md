---
title: "Precise drops"
category: news
summary: "Lowering now tracks most drops precisely"
---

Until now both interpreter and AOT
used some runtime tracking method to determine
if linear types needed to be dropped.

Now most bindings and all temporaries are tracked precisely
and the IR describes exactly when to drop them.

All `let` statements are precise.
All `var` statements are dynamically tracked.

`out` parameters use dynamic tracking to avoid
dropping before initialization.

More analysis could improve precise tracking,
at the cost of more analysis.
This is a nice and simple baseline for correctness and efficiency.
