# Datalove's Lexical Structure

Datalove source is read in three passes,
each of which knows less about the language than the one after it:
a _source map_ that finds strings and comments,
a _lexer_ that splits the rest into words, sigils and whitespace,
and a _bracer_ that matches braces into a tree.
The parsers for datalit and datafun both work on that tree.


## Source maps: strings and comments

The first pass looks only for strings and comments,
so that nothing inside them,
braces in particular,
is ever seen by the later passes.

- `//` starts a line comment, running to the end of the line.
- `/*` starts a block comment, ending at the matching `*/`.
  Block comments nest.
- `"` starts a string, ending at the next unescaped `"`.
  A backslash escapes the character after it,
  and a string may span lines.

```datalove
/* outer /* inner */ still a comment */
let s = "// not a comment, and { is not a brace"
```

An unterminated string or block comment runs to the end of the source
and is reported as an error.

Escape sequences are not interpreted here, only skipped over;
the parser later processes
`\"`, `\\`, `\n`, `\r`, `\t`, `\0`, and `\u{...}` (1 to 6 hex digits).


## Sigils

Everything outside strings and comments is
whitespace, a _word_, or a _sigil_.
A word is a run of alphanumeric characters and `_`,
so keywords, names and the digits of numbers are all words;
numeric literals are assembled from words and `.` sigils by the parser.

A sigil is a punctuation token drawn from a fixed set,
matched longest first,
so `+?` is one sigil rather than `+` and `?`.
All sigils are ASCII.
The set includes:

- Braces: `( )`, `{ }`, `[ ]`, `< >`, the earmuffs below,
  and `%{` and `#{`, which close with a plain `}`.
- Arithmetic: `+ - * /`,
  the optional variants `+? -? *? /?`,
  and the checked variants `+! -! *! /!`.
- Comparison: `.< .> <= >= == !=`.
- Punctuation: `. , ; : = | ? ! @`.

Characters that are neither whitespace, word characters, nor the start of a sigil
are lexed as errors.


## Brace matching

Standard brace types:

```datalove
( )
{ }
[ ]
< >
```

"Earmuff" braces - just a visually unobtrusive way to
unambiguously reuse the standard braces:

```datalove
(| |)
{| |}
[| |]
<| |>
```

Currently `[| |]` is used for tensors and `{| |}` for tables;
`(| |)` and `<| |>` are reserved.

The map and set openers `%{` and `#{` close with a plain `}`.

Braces are matched before parsing,
so the parser sees a tree in which every brace pair is a branch.
Because `<` and `>` are braces,
"less than" and "greater than" are spelled `.<` and `.>`.

An unmatched open or close brace is an error,
but the bracer recovers by inserting the missing close or dropping the stray one,
so the parser can still report errors in the rest of the source.
