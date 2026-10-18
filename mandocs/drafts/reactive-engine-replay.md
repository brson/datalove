<!--

Title: 
Summary:

Audience: practicioners

Problem:
I want to show Datalove's script reactivity in blog posts.

Thesis:
Antithesis:
Synthesis:

-->


## The situation with Datalove's deterministic record and replay

I'm trying to practice "blog-driven development",
where I write down the problem I'm trying to solve,
and write down a solution before I start having an LLM write the code.
Then as the solution develops update the writing and distill it down to a simple narrative.
I think this approach has multiple promises,
not least of which is that it will keep me thinking hard about the
technical aspects of coding in the age of agentic programming.

<!-- problem statement -->

The next [checkpoint](../roadmap.md) in this project
is about the interactive and reactive scripting experience,
which is currently centered around the command line REPL.
I've been putting off starting on it because this is an opportunity
to include a strong visual component in the blog,
showing how the environment behaves interactively.
Currently I don't have a way to present Datalove's behavior over time on the web.

<!-- antithesis -->

I could embed Datalove as a wasm blob and drive it directly.
I don't want to do this right now because every time I write about it
I will need to upload a new large binary somewhere.
Definitely don't want to include that in the repo.
And don't want to deal with CORS in an S3 bucket.
I could record video of the terminal and upload it somewhere.
Again big binaries, or the need to deal with a video hosting service.
Could use a terminal capture and replay program.
I like this better, but it is limited to exactly what shows in the terminal.
Has pros and cons.

<!-- thesis -->

What I intend to do is capture the inputs and outputs to the scripting engine,
serialize them,
and replay them on the website with a custom player that renders to HTML.
I like this solution because it plays directly to the design strengths of Datalove,
which is specifically intended to be deterministic and reproducible.
Replaying commands and their recorded output on the web
needs the some machinery as replaying commands and reinterpriting them fresh.
Big synergy.



## Background

Datalove divides its world of code into modules and script units.
A single script unit on disk is just "a script", and has a `.dfs` extension.
In most respects script units are like a function,
except that they accept no arguments and are not required to return a result.
At run time they have the same layout as a function's stack frame.
Script units can be chained together in a list dynamically at run time.
New script units inherit the type checking and execution environment of previous script units.

In this example of a self-contained Datalove _worldfile_ we have two sequential script units.
They share a type checking context and execution environment.

```datalove
---
script-unit
---

todo

---
script-unit
---

todo

```

> Worlfiles are chunks of text delimited by headers bound between `---` lines.
> Each chunk is interpreted as determined by the header.

Each time code is entered in the REPL it becomes a script unit.
The REPL also allows bare expressions to be entered,
in which case they are lightly ensugared to be valid `let` statements.
For example, `1 + 2` becomes `let _out = 1 + 2`.
A worldfile has a special section type for this:

```datalove
---
expression
---
1 + 2
```

Worldfiles are used for a variety of testing purposes within Datalove,
but already provide much of the mechanism needed for expressing playback of an interactive session.

The REPL engine also supports its own commands,
which are not part of the language.
These are prefix with a slash, `/`.
An example is `/exit`,
which terminates the engine and exits the CLI.




## Requirements

Today the REPL UI can only append script units.
It can't undo the previous.
It can't edit or delete units.
The engine supports these but not the UI.
For this reproducible playback system we need every possible action to be describable serializable to text.

And since we already have a text-based syntax for REPL commands, `/command ...`,
we'll extend that.
Some commands may not be ergonomic for a human to actually enter,
but it will be good to have every possible REPL action described in one format.

We can think of the script unit chain as a stack,
so lets describe the commands we need in terms of a stack.

- `/push <source>`.
  Pushes the source of a script unit onto the stack.
  Can span multiple lines.
  This is conceptually what the three source-based input forms to the REPL desugar to.
  Not clear whether this will ultimately be useful to have as its own command and make
  that desugaring real or not.
- `/pop`.
  Pops the last script unit.
- `/edit N <source>`.
  Changes the source of the Nth prior unit, 0-based.
  Like N pops then a push.
- `/delete N`.
  Deletes the Nth prior unit, 0-based.
  Like N pops, then pushing all but the last back.




Worldfiles are a good-enough base for describing a series of inputs to the REPL.
They don't describe the outputs of the REPL engine though.
And for replay on the web we're going to want the sequence of inputs and outputs to be in JSON.

