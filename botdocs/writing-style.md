# Writing Style

How to write prose for this project: botdocs, and how to review and edit blog
posts in `mandocs/posts/`. Read this before writing or reviewing either. It is
a distillation of a few well-known sources, listed at the end; when this guide
is silent, they are the tiebreaker.

## Contents

- [The one rule](#user-content-the-one-rule)
- [Know which kind of document you are writing](#user-content-know-which-kind-of-document-you-are-writing)
- [Give it a spine](#user-content-give-it-a-spine)
- [Openings](#user-content-openings)
- [Endings](#user-content-endings)
- [Write the bookends first](#user-content-write-the-bookends-first)
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

## Endings

The opening makes a promise and the ending shows it was kept. A reader who got
the answer in the first paragraph does not need it again. They need to see
what it amounts to now that they have the evidence.

- **Answer the opening's complication.** The last *therefore* resolves the
  first *but*. If the ending is about something the opening never raised,
  one of the two is wrong, and it is usually the opening.
- **Say what is different for the reader.** Name what they can now do, or
  see, that they could not before. What was done, or how hard it was, does
  not belong here.
- **Restate the point, then go one step past it.** Williams puts a
  document's main point in two places: the end of the introduction and the
  conclusion. The conclusion's copy should add something the opening could
  not: a consequence, a cost, or what the point changes.
- **Name what is still open.** A fix almost always exposes the next problem.
  Ending on it is honest, and it is the opening of the next document.
- **Echo the opening.** Come back to the opening's example, code or
  question, changed by everything in between. Zinsser's test for an ending
  is that it takes the reader slightly by surprise and yet seems exactly
  right. Coming full circle is the most reliable way to pass it.
- **Then stop.** No summary of a short document, no "in conclusion", no call
  to action. Zinsser again: when you are ready to stop, stop.

The ending is the document's stress position, by analogy with Gopen and
Swan's rule for sentences: what is there is what the reader leaves with.
Do not end on a hedge, an acknowledgment, or a list of minor future work.

## Write the bookends first

Before the middle, draft the opening and the ending: the problem and the
answer you expect, and where that answer leaves the reader. The middle is
then the spine from one to the other. A section that does not move the
reader from the first toward the second is a digression.

Expect the ending to move. Drafting is how you find out what you think, so
the claim that surfaces at the end of a first draft is often the real thesis.
McEnerney and Williams both make this point. When that happens, move it to
the opening, and write a new ending to go with it.

A post started before the work is done gets the same treatment: bookends
holding the expected answer. When the results come in, check them against
the bookends. If they disagree, the disagreement is usually the better post.

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

Posts live in `mandocs/posts/` and Brian writes them. Bots do not draft posts;
they review and edit them when asked. The job is to make the post more itself,
not to make it sound like a bot wrote it.

### Audience

Nobody uses Datalove, so the reader is not a Datalove user. The primary reader
is a practitioner who has the same problem the post is about: language
implementers, people building interpreters, JITs, and runtimes in Rust, and PL
enthusiasts. Potential customers, employers etc read the same posts and judge them the same
way, by whether peers find them worth reading; do not write at them.

Make the problem general and Datalove the worked example. "How I made drops
precise in Datalove" is a devlog; "Static drops for linear types, and where
dynamic tracking still wins" is a post someone building a different language
will keep.

The test: would someone who will never run Datalove learn something they can
use? If yes, it is a `design` post. If no, it is a `devlog` post -- a short
note on what landed, which is fine, but should not be dressed up as more.
Checkpoints and releases are `milestone` posts. The builder rejects any other
category; new ones are declared in `crates/datalove-cli/src/main.rs`.

Two other audiences, served differently:

- **The future Datalove user** is served by the docs, not the blog. A post's
  reasoning feeds the explanation docs once the feature lands.
- **Users of a future product** built on Datalove (visual, spreadsheet-like
  data transformation) care about their data problems, not compiler
  internals. Occasional vision posts are written for them; do not mix the two
  audiences in one post.

Posts are often started before the feature is built: the problem and the
design options first, then the dead ends, then results. A draft in that state
is expected; review it for the parts that exist.

When reviewing, look for:

- **Audience.** Is the problem framed so a non-user cares? Is there Datalove
  jargon or context a practitioner from outside the project would not have?
- **Spine.** Does the middle hold together with *but* and *therefore*, or does
  it wander? Point out sections that do not advance the argument. The usual
  shape of a `design` post -- problem, exploration, results, lessons, future work
  -- is fine if the exploration has a spine.
- **Buried takeaway.** If the main point first appears in the lessons at the
  end, suggest moving it up.
- **Bookends.** Read the opening and the ending together, skipping the
  middle. Does the ending answer the question the opening raised? If the
  post was started before the work was done, do the results still agree with
  the opening?
- **Unbacked claims.** Opinions should rest on code, numbers, or a specific
  experience. Flag claims that need a receipt and places where a snippet,
  benchmark, or link to a commit would help.
- **Unclear sentences.** Apply the sentence rules above. Flag jargon a reader
  outside the project would not know.
- **Factual errors.** Check claims about the code and the language against
  the code and the specs.
- **Mechanics.** Frontmatter (`title`, `category` of `design`, `devlog`,
  or `milestone`, `summary`), file name `YYYYMMDDX-slug.md`, broken relative links, typos.

When editing, preserve the voice:

- First person, conversational, opinionated. Do not sand opinions down into
  "it depends", and do not add hedges.
- Line-per-clause formatting: lines break at clause boundaries, not at a
  column. This is distinct from botdocs.
- Direct first sentence, understated ending. Do not add an introduction, a
  conclusion, or a call to action. Understated does not mean absent: the
  ending still pays off the opening, quietly.
- Rough edges, fragments, and casual phrasing are the voice, not mistakes.
- Prefer suggesting changes over rewriting. Make direct edits only to
  mechanics and clear errors unless asked for more.

## Checklist

Before calling a document done:

- Can you state the reader, their problem, and your answer in three sentences?
- Is the answer in the first paragraph or two?
- Does the ending resolve the opening's complication, add a consequence, and
  name what is still open?
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
