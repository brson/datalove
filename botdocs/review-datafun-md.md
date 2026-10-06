# Review: mandocs/datafun.md

A review of the datafun primer for content and flow, as of 2026-10-06. The
intended audience is experienced programmers, likely knowing Rust, who have
already read `datalit.md`.

Every section is written except Scripts and interactive units, which is a
heading only. Every `datalove` code block was run through `datalove script`.
All run except the two meant to fail: the D001 example in Ownership and the
inference example in Generics (F016). Both are labelled as failing in the text.

## Summary

The structure is sound, and the section-level clarity issues from earlier
review passes have been addressed. What remains is topics that have no home
yet. Most are short additions to existing sections. The exception is the
Scripts section, which still has to be written.

## Missing topics

- **Scripts and interactive units.** This is the only stub. The intro promises
  incremental recompilation and reevaluation, and Constants and Collections
  both say a script returns a result. This section should cover what a
  script's top level is, the script's result, how `!` at top level stops it,
  and the interactive and reactive behaviour. Generics points here for the
  REPL story. The note at the end of Modules, about running several scripts in
  one compiler instance, may belong here too.
- **Totality.** The intro says functions "have no exceptional control-flow".
  Collections says there is "no indexing operation that panics", and Numerics
  says integer arithmetic never overflows silently. Neither is stated as the
  general rule. Give it a paragraph near the start: no panics, and every
  partial operation returns an option or result, or returns early through `?`
  and `!`.
- **`error` and `data`.** Data types now says what each holds in one sentence.
  Nothing says how errors are made beyond the `error "..."` constructor used in
  examples, or what can be put into a `data`.
- **Strings.** The most common moved type gets no section. The first script
  shows `to_uppercase` and `len`, and Ownership shows `push_str`. A short tour
  of everyday string operations would help.
- **Writing a module.** Modules describes the hierarchy and the workspace but
  never shows a module's contents. Say that a `.dfm` holds functions, types and
  consts, and that every function is visible to requirers while consts are
  private, as Constants says. Show a `local/` module being required from a
  script.
- **Riders.** These are native modules, `require`d like any other. A paragraph
  under Modules, since an embeddable scripting language is the intro's pitch.

## Smaller points

- **Intro.** "For detail see additional documentation (which does not exist)"
  should eventually link the specification or be cut.
- **Ownership.** "Values are uniquely owned" is followed by copy types; the
  copy list now explains the relationship. "Affine" would be the precise
  word, if the audience is expected to know it.
- **Numerics.** The conversion paragraph names the conventions but does not
  mention conversions between `index`/`offset` and the other integers
  (`index.from_u64`, `index.from_int` and so on), or that `@` only widens.
