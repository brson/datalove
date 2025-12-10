# Task: make the new interpreter shine

We've finished reimplementing the interpreter (
see
oldplans/plan-new-interpreter.md,
oldplans/plan-interp-temp-slots.md etc.
oldplans/plan-split-parser.md etc.
)

It's beyond feature parity with the old interpreter,
but its implementation is still pretty shaky.

We ultimately want the interpreter to not make any
decisions on its own, but to be entirely driven
by the function analysis.

We also know its doing extra clones that it shouldn't be:
the current datalove language only includes moves,
no clones yet;
but there are some in the interpreter.
These are bugs.

Every value and temporary should be accounted for
by the function analysis;
anywhere where we are creating ad-hoc values on the heap
instead of storing them in a slot in the stack fram
should be considered a bug.

Every expression and statement type should be exercised in three
different contexts:
in script unit statements,
in functions in script units,
in functions in modules.

Function in script units
and functions in modules should share the same code path.
script statements and expressions should share as much
code with functions as reasonable.

Review the datafun interpreter
and look for opportunities to simplify
and cleanup along these design guidelines.