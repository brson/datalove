# Report: `.fui` on bcts, as bcts now stands

Date: 2026-09-14

`brson/fairlightui:botdocs/proposal-datalove-syntax.md` proposes moving `.fui`
onto datalove's syntax and bcts's brace tree. It was written against bcts
0.5.0 and measured what that version did. Several of the things it asks for
have since been built, and two of its open questions are now answered by the
lexer rather than by a decision. This is what is left to do, and how the part
it spends the most words on - deciding where one property ends and the next
begins - is done with what bcts already has.

## 1. What the proposal asked for, and what exists now

| the proposal's ask | now |
| --- | --- |
| `%` added to the sigil set upstream (§7 open question) | done. `%` is `Sigil::Percent`, and `%{` still wins as the longer match |
| `0 .. 8` "unchanged" | done. `..` is `Sigil::DotDot`; it was two `Dot` tokens before, which the proposal did not notice |
| `1.5px`, `50pct`, the unit on the fractional part (§3) | done. `parser_util::eat_number` returns the digits, fraction, exponent and a `suffix`, which is where a unit arrives |
| `#3f3f3f` "joined on adjacency" (§3) | done. `TreeToken::span`, `TokenStream::prev_end`/`peek_next`, and `glued_left`/`glued_right` |
| "spans and diagnostics get rebuilt" (§5) | mostly done. `bct::render` prints a `Diagnostic` over one source or several; `bct::diagnostics!` writes the salsa accumulators for whatever phases a language has |
| `TokenStreamExt` (§2) | unchanged, plus the above |
| brace recovery (§2) | unchanged; still the reason to do this |

So the remaining bcts-side work for `.fui` is **none**. What is left is on the
`.fui` side, and one of those items is larger than the proposal says.

## 2. What `.fui` still has to change about itself

These are forced by the lexer and the bracer, not by taste.

1. **`<` and `>` are braces.** This is the big one, and the proposal
   understates it as a table row. Measured: `if a < b { x } else { y }` gives
   `Branch(< closed=false [...])` - the `<` opens a branch that swallows the
   rest of the document. `.<` and `.>` have to land in the same step as
   `:` → `=`, not in the later parser swap, because until they do nothing
   parses at all.
2. **`&&` and `||`.** `&&` lexes as an `Error` token and `||` as two `Pipe`
   sigils. `fairlightui-lang/src/parse.rs:427-428` has both, so the
   proposal's "`and or not` - already matches" row is wrong: only `not` is a
   word today. Related trap: `(|`, `{|`, `[|` and `<|` are earmuff-brace
   opens, so a `(` written against a `|` is a bracket.
3. **Hyphens in names.** `matte-plastic` is three tokens. The proposal's
   recommendation - rename the material table to snake_case - stands.
4. **`:` → `=`**, as proposed.

`%` versus `pct` is now a free choice rather than a question: `50%` lexes, and
`50pct` arrives as digits `50` with the suffix `pct`. Both work; pick on
looks.

## 3. The pipeline

```
Source
  -> source_map::basic_source_map     -> Chunk          (comments and strings scanned out)
  -> lexer::lex_chunk                 -> ChunkLex       (Word | Sigil | String | Comment | Whitespace | Error)
  -> bracer::bracer                   -> Bracer         (a brace tree, and what it had to repair)
  -> bracer.iter(db)                  -> TreeToken::{ Token, Branch { sigil, open, close, inner } }
  -> split::split_lines               -> Vec<TokenGroup>  (one per line, at this brace level)
  -> your parser over parser_util::{TokenStream, TokenStreamExt}
  -> diagnostic::DiagnosticBuilder -> bct::diagnostics! -> bct::render
```

The step the proposal does not mention is `split`, and it is the one that
decides the shape of a `.fui` document.

## 4. Line-oriented parsing of properties in braces

### What `split_lines` does

```rust
pub fn split_lines<'db>(
    db: &'db dyn salsa::Database,
    tokens: impl IntoIterator<Item = TreeToken<'db>>,
) -> Vec<TokenGroup<'db>>
```

(bcts names that `crate::Db` internally and does not re-export the alias, so
from outside it is `salsa::Database`. This repo renames the dependency to
`bct`, which is why the paths below read `bct::` where a fresh consumer would
write `bcts::`.)

It cuts a run of tokens at a newline inside a whitespace token, or at a `;`,
and hands back every group including the empty ones. Each group records what
closed it, so a blank line - which separates nothing and is nobody's mistake -
is distinguishable from a `;` written with nothing before it. Whitespace and
comments are dropped from the groups themselves, so a formatter that wants
the comments back reads the bracer's own iterator instead.

Three properties matter for `.fui`:

- **A newline inside a branch belongs to the branch.** The bracer has already
  nested `{ ... }`, `( ... )` and `[ ... ]`, so `split_lines` at one level never
  sees the newlines inside them. `stroke = (1.5px,` newline `pointer)` is one
  line, with no continuation rule needed.
- **It is per brace level.** To read the inside of an element you call it
  again on that branch's `inner`.
- **The complaint is written for you.** `stray_delimiters` plus
  `stray_delimiter_error` give the same message both datalove parsers give, so
  a reader moving between the languages meets one error rather than three
  spellings of it.

### The consequence for the proposal's §4

The proposal's Option A and Option B both require commas between properties,
because of this, quoted from the `.fui` language doc:

> Inside an element's property run a bare identifier is otherwise ambiguous
> with the start of a sibling.

That is a limit of the hand-written parser, not of bcts. With `split_lines`
inside each branch, a bare identifier alone on a line is a flag and a name
followed by a brace is a sibling, with no ambiguity and no commas. The closed
flag list dies either way.

So there is a third option beside A and B: **keep the newline as the
separator**, and accept `;` where two properties share a line. That is exactly
what datalove statements do, and it changes the §5 cost estimate - the four
documents are not rewritten a second time, only their `:` becomes `=`.

If you want `,` to work as well, split each line group again on commas:
`split_commas(group.tokens, 1)`. Then `size = (30, 30), radius = 15px` on one
line means what it looks like, and so does the same pair on two lines. Both
datalove parsers already do this for tensor rows and columns.

### The shape of the parser

```rust
/// Parse the members of one brace, each line a property, a flag or a child.
fn parse_members<'db>(
    db: &'db dyn salsa::Database,
    text: Text<'db>,
    tokens: impl IntoIterator<Item = TreeToken<'db>>,
) -> Vec<Member<'db>> {
    let groups = split::split_lines(db, tokens);

    // A `;` with nothing before it separates nothing.
    for written in split::stray_delimiters(&groups) {
        split::stray_delimiter_error(db, text, &written, "properties")
            .code("U010")
            .emit_parse();
    }

    split::nonempty_groups(groups)
        .into_iter()
        .map(|line| parse_member(db, text, line))
        .collect()
}

fn parse_member<'db>(
    db: &'db dyn salsa::Database,
    text: Text<'db>,
    line: Vec<TreeToken<'db>>,
) -> Member<'db> {
    let mut stream = Stream::new(db, line, text);
    let Some(name) = stream.eat_name() else { ... };

    // `name = value`
    if stream.eat_sigil(Sigil::Equals) {
        return Member::Property { name, value: parse_value(&mut stream) };
    }

    // `name { ... }` - an element, read the same way one level down
    if let Some(inner) = stream.eat_branch(Sigil::BraceOpen) {
        return Member::Element { name, members: parse_members(db, text, inner) };
    }

    // `name` alone - a flag
    if stream.peek().is_none() {
        return Member::Flag { name };
    }
    ...
}
```

`eat_name`, `eat_sigil` and `eat_branch` are `TokenStreamExt`; the `Stream` is
whatever type implements `TokenStream` over a `Vec<TreeToken>`, which is six
methods (`db`, `peek`, `peek_next`, `next`, `prev_end`, `source_text`). Both
datalove parsers' `state.rs` are worth copying for it.

### If you take Option A after all

Option A spreads one element's properties over several unbraced lines:

```
path pin = 0,
     d = "M {x},{y} L {x2},{y2}",
     stroke = (1.5px, pointer)
```

`split_lines` makes that three groups. bcts has no line-continuation notion,
deliberately - datalove has none either - but joining is a few lines at the
caller, since the comma is the last token of the group it ends:

```rust
/// Join the lines a trailing `,` continues onto the next.
fn join_continuations<'db>(groups: Vec<Vec<TreeToken<'db>>>) -> Vec<Vec<TreeToken<'db>>> {
    let mut joined: Vec<Vec<TreeToken>> = vec![];
    for line in groups {
        let continues = matches!(
            joined.last().and_then(|prev| prev.last()),
            Some(TreeToken::Token(t)) if t.kind == TokenKind::Sigil(Sigil::Comma)
        );
        match continues {
            true => joined.last_mut().X().extend(line),
            false => joined.push(line),
        }
    }
    joined
}
```

Under Option B every element has braces and the question does not arise, which
is one more argument for B if the parser is being written from scratch anyway.

## 5. Numbers, units and colours

`eat_number` reads one literal as far as it was written without spaces and
returns the pieces:

```rust
pub struct Number {
    pub negative: bool,
    pub radix: Radix,          // Dec | Hex
    pub float: bool,
    pub digits: String,        // as written, separators and radix prefix included
    pub fraction: Option<String>,
    pub exponent_tail: Option<String>,
    pub suffix: Option<String>,   // `px`, `pct`, `deg`, `db` - or an error, in datalove
    pub span: Range<usize>,
    pub error: Option<NumberError>,
}
```

- `1.5px` arrives as digits `1`, fraction `5`, suffix `px`.
- `50pct` arrives as digits `50`, suffix `pct`.
- `-25deg` - the sign is a token in prefix position, as the proposal says, and
  `eat_number` takes it only when it is written against the digits.
- `number.complaint()` gives the message and label for a literal written
  wrongly, quoting the number that was meant: `1 . 5` is answered with
  ``write it as `1.5` ``. Use it, so that both languages complain alike.

A `#rrggbb` colour is `Sigil(Hash)` then a `Word`, joined when
`stream.glued_right()` says they were written together. `# 3f3f3f` is then a
`#` and a number, and can be told so.

If `.fui` wants spacing to decide an operator's fixity as datalove now does -
`a - b` and `a-b` infix, `a -b` prefix - `TokenStreamExt::is_infix_spacing`
is the predicate, and `botdocs/design-token-gluing.md` is the rule.

## 6. Diagnostics

```rust
bcts::diagnostics! {
    ParseDiagnostic => emit_parse,
}
```

That writes the salsa accumulator and the `emit_parse` method on
`DiagnosticBuilder`. `.fui` wants one phase where datalove has five.

Printing is `bct::render`:

```rust
render::render_diagnostics(db, diagnostics.iter().map(|d| d.to_diagnostic(db)), file_path, cwd);
```

with `Renderer` for a run that has other things to print between diagnostics,
and `insertion_suggestion` for the `+`-marker form. A `Diagnostic` carries
labels, notes, helps and suggestions; `did_you_mean` and `available:` are
notes.

## 7. Staging, revised

1. **Rename the materials** to snake_case. One commit, no syntax change.
2. **`:` → `=`, `<`/`>` → `.<`/`.>`, `&&`/`||` → `and`/`or`.** All three are
   forced by the lexer, so they belong together, and they are the whole of the
   family-resemblance change. Documents and docs move once.
3. **Swap the lexer and parser for bcts**, no syntax change. The tests written
   in step 2 hold it honest. This is where `split_lines` decides whether the
   document keeps newline-separated properties or goes comma-separated.
4. **Option B**, if it is wanted, once the parser walks a brace tree.

Step 3 is now a smaller job than the proposal costs it at: the numeric
reassembly, the adjacency checks, the line splitting and the rendering are all
written.

## 8. What is still worth arguing about

- Whether a `.fui` document should *be* a datalit value (commas, Option B) or
  merely parse with datalove's tools (newlines, no rewrite). The second is
  now cheap, which it was not when the proposal was written.
- Whether to draw a partially-typed document at all, which the proposal's §7
  raises and nothing here answers.
- `%` or `pct`. Both lex.
