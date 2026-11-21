# Lexical Grammar

The tokens of datalit and datafun, as EBNF over characters. Both languages
share one lexer, in `crates/bcts/src/lexer.rs`, and both grammars,
[Datalit Grammar](datalit-ebnf.md) and [Datafun Grammar](datafun-ebnf.md),
are written over the tokens it makes. This states what those tokens are, and
a test holds the lexer to it (see
[Checking the grammar](#user-content-checking-the-grammar)).

## Contents

- [Notation](#user-content-notation)
- [Grammar](#user-content-grammar)
- [What the grammars read](#user-content-what-the-grammars-read)
- [Checking the grammar](#user-content-checking-the-grammar)

## Notation

The EBNF is as in the [datalit grammar](datalit-ebnf.md#user-content-notation),
over characters. One rule is not in it: **the lexer takes the longest lexeme
that matches**, starting where the last one ended. So a word is never two
words, `->` is one sigil and not `-` and `>`, and `//` begins a comment, not
two `/`s. Whitespace, newlines and comments are lexemes but not tokens: the
lexer drops them.

## Grammar

```ebnf
(* ===== A source  (lexer.rs: lex) ===== *)
source        = { lexeme } ;
lexeme        = token | whitespace | newline | comment ;
token         = word | sigil | string | error ;

word          = word_char, { word_char } ;
word_char     = ? a Unicode alphanumeric character ? | "_" ;
string        = '"', { string_char | escaped }, '"' ;
string_char   = ? any character except '"' and '\' ? ;
escaped       = "\", ? any character ? ;   (* what it means is the parser's: D039 *)

whitespace    = space, { space } ;
space         = ? a Unicode whitespace character but "\n" ? ;
newline       = "\n" ;
comment       = line_comment | block_comment ;
line_comment  = "//", { ? any character except "\n" ? } ;
block_comment = "/*", { block_comment | comment_char }, "*/" ;   (* nests *)
comment_char  = ? any character, not starting "/*" or "*/" ? ;

(* ===== Sigils  (lexer.rs: Sigil) ===== *)
sigil         = "+?=" | "-?=" | "*?=" | "/?=" | "+!=" | "-!=" | "*!=" | "/!="
              | "(|" | "|)" | "{|" | "|}" | "[|" | "|]" | "<|" | "|>"
              | "%{" | "#{"
              | ":-" | "->" | "+?" | "-?" | "*?" | "/?" | "+!" | "-!" | "*!" | "/!"
              | "+=" | "-=" | "*=" | "/=" | ".." | ".<" | ".>" | "<=" | ">=" | "==" | "!="
              | "." | "," | "+" | "-" | "*" | "/" | "=" | "<" | ">" | "|" | "?" | "!"
              | ":" | "#" | "@" | "%" | "(" | ")" | "{" | "}" | "[" | "]" | "$" | "~"
              | ";" ;

(* ===== Errors ===== *)
error         = stray, { stray }
              | '"', { string_char | escaped }, [ "\" ], END        (* a string never closed *)
              | unclosed ;
unclosed      = "/*", { block_comment | comment_char }, ( unclosed | END ) ;
                (* a block comment never closed *)
stray         = ? a character that begins no other lexeme ? ;
END           = ? the end of the source ? ;
```

Longest match decides what the grammar alone leaves open:

- **Words are whole.** Two word characters written against each other are in
  one word: `truefalse` is one word, and `12` one, not `1` and `2`.
- **Comments are whole.** A `/` followed by `/` or `*` begins a comment, never
  a sigil, so `: u8 //1` has no `/` token after `u8`. A line comment runs to
  the end of its line. A block comment nests:
  `/* /* */ */` is one comment.
- **A string or comment never closed is an error token** running to the end of
  the source, `error`'s second and third forms; a closed one is longer than
  any error it begins.

## What the grammars read

The datalit and datafun grammars are over these tokens, and match each
terminal against a token's text. Whitespace and comments between tokens are
not written in them. Three things are read from what the lexer drops:

- **Glue.** Two tokens are glued when nothing is between them: no
  whitespace, newline or comment. Datafun's `~` says a token is glued to the
  one before it, and spacing decides what is glued in a tensor's row in both
  languages.
- **`EOL`**, in datafun, is a newline lexeme before a token outside brackets:
  a statement break. A newline inside a block comment is not one.
- **`newline`** is a newline lexeme before a token in a `{|` and `|}` pair,
  outside the brackets nested in it: a table's row break. A type reads it as
  whitespace, so a table *type* may be written over several lines, while a
  table *literal* starts a row at each.

`NAME` in both grammars is a word of ASCII letters, digits and `_` not
beginning with a digit. A word with a character outside ASCII is a name to
the lexer and refused by the parser (D043).

## Checking the grammar

`crates/datalove-doc-fixtures/tests/lexical_grammar_tests.rs` reads the
grammar block above and lexes with it: from where the last lexeme ended, the
longest match of `lexeme`, by an Earley recognizer over characters. It holds
the lexer to the same tokens, each with the same kind, span, and whether a
newline lexeme came before it since the token before. The `sigil` rule must
list the lexer's sigils, every one.

The inputs are every datalit and datafun source the grammar tests read, each
with characters changed, strings generated from the grammar, and strings of
characters drawn from those the rules single out. The meaning of each
`? x ?` is in `crates/datalove-doc-fixtures/src/lexical.rs`.

The generated strings must go everywhere in the grammar the start rule
reaches: every rule, every alternative of every `|`, and every option and
repetition taken, or the test fails, naming what was never written. The
generator favours what it has written least, so a rare corner is reached
without many more strings.
