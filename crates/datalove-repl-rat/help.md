# The datalove REPL

Every line you enter is a **script unit**: statements, whose bindings the
session keeps, or an expression, whose value it shows. Units stay in the
session in order, and the **Code** panel above shows them all, each with a
number it keeps for as long as the session lasts.

## The panels

- **Code** -- the session's units, as their source, one after another.
- **Input** -- where a new unit is typed, or a unit is edited.
- **The tabs below** -- **Environment**, the bindings the session has;
  **History**, everything entered and what it came to; **Detail**, all about
  the selected unit; and this **Help**.

The bar beside each unit says how it fared the last time it was compiled or
run:

- green -- it ran;
- red -- it did not compile;
- yellow -- it returned early, through `!` or `?`;
- magenta -- it was not run, because it reads from a unit that did not finish;
- cyan -- it is waiting on the engine.

The right half of the bar says what the last change did to the unit: its top
corner if the unit was compiled again, and its bottom corner if it ran again.
A bar with no right half was left alone.

## The selected unit

One unit is always selected, its number in white: the one last entered, or
the one moved to in the Code panel. The **Detail** tab says what there is to
know about it -- how it fared and what the last change did to it, its errors,
what its bindings hold now, the functions it defines, and the names it reads
and writes. A change that leaves the selected unit broken shows the Detail
tab.

## Typing

- **Enter** submits a line.
- A line that opens a block -- `fun`, `if`, `match`, `loop`, `for` -- goes on
  over several lines until its `end`. There **Enter** starts a new line and
  **Alt+Enter** submits.
- **Ctrl+C** twice throws away a multiline input, or an edit.

## Changing the code

With focus on the Code panel, **Up** and **Down** select a unit, and:

- **Enter** edits it in the input;
- **Delete** deletes it;
- **Insert** writes a new unit to go before it.

Units that use what a change touched are compiled and run again, and the
rest are left as they were. The same changes can be typed as commands:

```
/edit N <source>
/delete N
/insert before N <source>
/insert after N <source>
```

## Commands

- `/help` -- this help; **F1** too.
- `/undo` -- take back the last change to the code; what is taken back is
  grayed out in the History tab.
- `/redo` -- make again the change last taken back. Entering anything new
  forgets what was taken back.
- `/reset` -- start the session again, empty.
- `/save` -- save everything entered to `repl-session.world`.
- `/load` -- start the session again as `repl-session.world` has it.
- `/exit` -- leave; **Ctrl+C** twice too.

## Moving around

- **Tab** and **Shift+Tab** move focus between the panels.
- **F2** cycles the tabs, and **Alt+1** to **Alt+4** pick one.
- **Up**, **Down**, **PageUp** and **PageDown** scroll the panel with focus;
  with **Ctrl** they scroll the other one.
