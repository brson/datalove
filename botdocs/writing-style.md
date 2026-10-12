# Writing Style

How to write prose for this project: botdocs, and blog post drafts for
`mandocs/posts/`. Read this before writing either. It is a distillation of a
few well-known sources, listed at the end; when this guide is silent, they are
the tiebreaker.

## Contents

- [The one rule](#user-content-the-one-rule)
- [Know which kind of document you are writing](#user-content-know-which-kind-of-document-you-are-writing)
- [Give it a spine](#user-content-give-it-a-spine)
- [Openings](#user-content-openings)
- [Sentences](#user-content-sentences)
- [Cutting](#user-content-cutting)
- [Botdocs conventions](#user-content-botdocs-conventions)
- [Blog posts](#user-content-blog-posts)
- [Checklist](#user-content-checklist)
- [Sources](#user-content-sources)

## The one rule

Write for a reader with a problem, not to record what you know.

The reader does not care that a thing was done or how much effort it took.
They care about something that is costing them: a slow loop, a confusing
rule, a design that will not scale. A document earns its length by changing
how the reader understands that problem. Before writing, name the reader and
the problem in one sentence each. If you cannot, you are not ready to write.

## Know which kind of document you are writing

Most muddled documents are two or three kinds of document mixed together.
Pick one; link to the others.

- **Reference** -- what is true. Specs, typing rules, ABIs, grammars.
  Complete, terse, organized for lookup. No narrative, no history.
  Examples: `botspec.md`, `native-abi.md`.
- **Explanation** -- why it is this way. Architecture docs, "how it works"
  docs. Organized for reading front to back. Examples: `generics.md`,
  `salsa-architecture.md`.
- **Plan / design** -- what we intend to do and why. Starts from a problem,
  argues for one design, admits what is unresolved. Examples: the `plan-*.md`
  files.
- **How-to** -- steps to accomplish a task. Example: `release.md`.
- **Blog post** -- an explanation with a narrator and an opinion. See
  [Blog posts](#user-content-blog-posts).

A plan that becomes implemented should eventually be distilled into an
explanation or reference doc. Do not keep patching a plan into a spec.

## Give it a spine

A document that proceeds "and then, and then, and then" loses the reader.
Connect sections with *but* and *therefore* instead:

> We have X, and it does Y. **But** Z goes wrong. **Therefore** we try W.
> **But** W costs V. **Therefore** ...

Each section should exist because the previous one left a problem open. If a
section does not advance that chain, it is a digression: cut it, shrink it to
a sentence, or move it to its own document.

This applies to blog posts most of all. "What I did on the way" is not a
spine. Dead ends belong in the story when they teach something -- when the
*but* is the interesting part -- not because they happened.

## Openings

The first paragraph does four jobs, in this order (Minto's SCQA):

1. **Situation** -- what the reader already knows or accepts.
2. **Complication** -- what is wrong, missing, or costly about it.
3. **Question** -- usually implicit: so what do we do?
4. **Answer** -- the thesis, stated plainly.

State the answer early. A reader who knows where a document is going can read
the evidence as evidence; a reader who does not is just accumulating facts.
Mysteries are for fiction.

Do not open with throat-clearing ("This document describes...", "In this post
I will..."), background the reader already has, or the history of how the
document came to be.

## Sentences

From Williams and Gopen & Swan; these do most of the work of clarity.

- **Characters are subjects, actions are verbs.** "The checker rejects the
  call" not "Rejection of the call is performed by the checker." Hunt
  nominalizations (-tion, -ment, -ance) and turn them back into verbs.
- **Old before new.** Begin a sentence with something the reader has already
  seen; end it with the new information. This is what makes prose feel like
  it flows.
- **The end of a sentence is the stress position.** Put the important thing
  there. Do not end on a qualifier or a citation.
- **Get to the verb fast.** Do not make the reader hold a long subject in
  their head.
- **One idea per sentence.** Split sentences that join two claims with
  "and" or a semicolon unless the claims are genuinely one thought.
- **Concrete over abstract.** Name the function, the type, the number. "The
  bytecode loop is 30% slower" beats "performance is impacted."
- **Code is evidence.** Show the real snippet, the real error, the real
  benchmark output, not a paraphrase of it.

## Cutting

Draft long, then cut a third. Things to cut first:

- Sentences that restate the heading or the previous paragraph.
- Summaries at the end of short sections.
- Hedges with no information: "fairly", "somewhat", "it should be noted".
  Keep a hedge only when the uncertainty is real, and then say what it is.
- Hype: "powerful", "seamless", "robust", "blazingly fast", "elegant".
- Inline lists of things that are not used anywhere else in the document.
- Transitional filler: "Now let's look at...", "With that out of the way...".

## Botdocs conventions

Observed from the existing docs; follow them.

- `# Title` as a noun phrase that names the subject: "Immutable Cursors for
  the Collections", "The Native Rider ABI".
- Directly under the title, one short paragraph: what this is, its status, and
  when it was written. For example: "Written October 2026 alongside the `for`
  draft; not implemented."
- A `## Contents` list for anything longer than a few screens. Anchors use the
  `#user-content-` prefix.
- Prose is hard-wrapped at about 80 columns, in ordinary paragraphs.
- Plans end with what exists, what is missing, phases, and open questions.
  Open questions are real questions, not tasks in disguise.
- Link to code by path (`crates/datalove-rtdt/src/layout.rs`) and to other
  docs by relative link.
- New docs get an entry in both `README.md` and `nav.md`, in the right
  section.
- Plain ASCII punctuation. No emoji.

## Blog posts

Posts live in `mandocs/posts/` and are Brian's. Bots draft them only when
asked, in `mandocs/drafts/`. The `claude-skills:interview` skill has the full
voice rules; the essentials:

- **Frontmatter**: `title`, `category` (`dev` or `news`), `summary` (one short
  line). Files are named `YYYYMMDDX-slug.md`.
- **First person**, conversational, peer to peer. Opinionated where Brian has
  an opinion; do not sand it down into "it depends".
- **Line-per-clause formatting**: break lines at clause boundaries, not at a
  column. This is distinct from botdocs.
- **Declarative first sentence.** Jump straight in: "I've decided to close out
  Checkpoint 1."
- **Understated ending.** A quiet observation, a callback, or just stop. No
  "In conclusion", no calls to action.
- **Separate what was done from what was learned.** Sections like "Some things
  I like" / "Some things I don't love" or "Learnings" work well.
- **Receipts.** Every opinion is backed by code, numbers, or a specific
  experience. Link to commits and docs.
- **Length follows type.** `news` posts can be a few paragraphs. `dev` posts
  that argue something need the spine described above.

The usual shape of a `dev` post -- problem, exploration, results, lessons,
future work -- is fine, provided the exploration has a *but/therefore* spine
and the main takeaway shows up early, not only in the lessons at the end.

## Checklist

Before calling a document done:

- Can you state the reader, their problem, and your answer in three sentences?
- Is the answer in the first paragraph or two?
- Does every section follow from a *but* or *therefore* in the one before it?
- Is it one kind of document?
- Read the first sentence of each paragraph alone. Do they tell the story?
- Did you cut a third?

## Sources

- Larry McEnerney, "The Craft of Writing Effectively" (UChicago lecture,
  on YouTube). Writing for the reader's problem.
- Randy Olson, *Houston, We Have a Narrative*. And / But / Therefore.
- Barbara Minto, *The Pyramid Principle*. SCQA, answer first.
- Joseph Williams, *Style: Lessons in Clarity and Grace*. Sentences.
- George Gopen and Judith Swan, "The Science of Scientific Writing" (1990).
  Reader expectations, stress position.
- William Zinsser, *On Writing Well*. First-person nonfiction, cutting
  clutter.
- Daniele Procida, Diataxis (diataxis.fr). Kinds of documentation.
