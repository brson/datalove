# Datalove REPL UI

The Datalove REPL has a rich UI model that
can be implemented in the console or elsewhere
(particularly via html/css + wasm).

It is fully reactive and introduces aspects of Jupyter-style cell-oriented notebooks.

We're going to try something radical.
There are three panels, from top to bottom:

- repl history
- text input
- debugging pane

Probably the middle panel has a border,
and the other two don't directly,
though each history entry probably has a visual divider between.

## Repl history panel

Similar to the scrollback of a typical repl,
but more interactive. New entries at bottom,
scrollable.
Each entry is an input and eval.
Every input has an entry, even if it's a repl command or blank,
and different type of entries may have unique uis.
Think of the history entries as interactive "cards" in material design terms.

## Text input

Starts as a full-width single line text entry.
When the repl switches to multi-line mode,
it becomes a multi-line scrollable text entry widget,
where enter inserts newlines, and _shift+enter_ executes
(probably need visual indicator).
Switches back after entry.

## Bottom panel

Scrollable panel containing a table of:
local variables; serialized values.

## Other notes

Remember that the repl UI doesn't have direct access
to the engine and needs to communicate to it through messages.
For now we can maybe just query the entire UI model state from the engine
at once after each eval.

