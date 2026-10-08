# New and optimizable math ops

Recently I've been proving the performance potential of language model
with some simple benchmarks and a simple representative demonstration app
(the details of which aren't super important).
I have been focusing on the bytecode interpreter and the JIT,
including compilation time and the runtime.
As with writing the high-level docs for the website,
this dogfooding exercised has been highly effective,
finding lots of bugs and semantic gaps.

For bytecode performance I am comparing to Python.
For JIT to Julia.
Both have dynamic typesystems, where Datalove is static,
though Python's bytecode interpreter is amazingly performant
for happy-path synthetic benchmarks,
and Julia of course is 100% jitted via LLVM and is very fast.
Julia's has unfortunately slow startup time though,
in the range of 100ms for simple benchmarks.
I am aiming for fast startup time and fast performance.
So far Datalove's startup and compilation time is fast,
runtime performance is in the ballpark,
enough to give me confidence about the basic data and runtime model
(at least without considering generic functions),
but sometimes considerably slower than my expectations.
I wont throw out any numbers at this time since they are not rigorous
and the implementation and language model are changing rapidly.

Today I was looking at
(having an LLM look at, under my close expert supervision)
the performance of the `sum` benchmark,
the Datalove implementation being as follows.

```datalove
fun sum_to(limit: u32): !int
  var total: int = 0
  var i: u32 = 1
  loop while i <= limit
    set total = total + i@
    set i = i +! 1
  end loop
  ret ok total
end fun

let result = sum_to(30000000)
if result |value|
  debuglog(value)
else |err|
  debuglog(false)
end if
```

This is a bigint benchmark that simply sums the numbers from 1 to 30 million,
so once the interpreter is reasonably tuned generally it becomes primarily
a test of bigint efficiency. Bigints are primarily implement in the runtime in Rust,
where performance is about representation (are all numbers on the heap? are small numbers
packed into the stack?) and algorithmic efficiency.
But because bigints are non-copyable types that live on the heap (Datalove bigints are always on the heap),
the basic semantics of math ops and their interaction with deep-cloning has a significant
impact on bigint performance.

I generally prefer to implement the simplest and clearest surface semantics reasonable to accomplish
a task, only expand complexity as I better understand the need.
So Datalove only supports a few built-in math operations today:
binary addition, subtraction, multiplication, division, and unary negation (`+`, `-`, `*`, `/`, unary `-`).
Though because one of Datalove's principles insists on strict
[numeric correctness](principles.md#user-content-numerical-correctness)
the exact formulation of these is
[pretty quirky](datafun.md#user-content-numerics).

So on the `sum` benchmark,
after squeezing out all the basic performance blunders in the allocator and the bigint implementation
I was finding the bytecode performance still significantly behind Python's bytecode interpreter,
on the order of 2x.

The primary culpret was excessive cloning of bigints.
Lets see our `sum_to` function:

```datalove
fun sum_to(limit: u32): !int
  var total: int = 0
  var i: u32 = 1
  loop while i <= limit
    set total = total + i@
    set i = i +! 1
  end loop
  ret ok total
end fun
```

That `set total = total + i@` is the whole story.
Binary ops borrow their operands so themselves don't require a clone,
but they do produce a fresh value, a bigint `+` always performs an allocation.
It _seems_ like we could instead do that operation in place directly into `total`
without reallocating.
Furthermore, because both sides of addition require bigints,
we have to promote the `u32` counter to `int`, another allocation.

Two seperate issues.
To address the allocating math operator that immediately assigns to one of its operands
here we _could_ do a fairly simple
peephole optimization on `set total = total + <something>`,
the IR for which looks like

```
todo
```

Imminently pattern-matchable.
But one of Datalove's principles is that
[nothing happens that is not written](principles.md#user-content-nothing-happens-that-is-not-written).
I much prefer the language to have surface constructs that map to
the required underlying performance mechanisms.
That both makes the language's performance characteristics clear,
and lets the compiler avoid accumulating passes that slowly eat at compile times.
and the surface operation for this is obvious: compound assignment.

```datalove
set total += i@
```

This common operation, assign `total + i@` to `total`,
implies exactly the optimization needed.

Aside: one might think "well, even if we _have_ this construct
it would be _nice_ if the compiler optimized `set total + total + i@` _anyway_.
Maybe it would, but I'm kinda thinking no:
let's instead teach the compiler to analyze the possibility of doing this optimization,
but emit a note suggesting to do it by hand.
I strongly value implementation simplicity and a straightforward mapping from source to executable.

Another aside! Even though optimizing bigint math is our motivator,
having compound assignment should be a win for all types for the bytecode
interpreter too, since it can fuse the math and the assigment into a single opcode.


## In-place bigint math

This optimization only matters if can actually do the bigint math without allocating a new buffer.
Can we?

todo yes for + -, no for * /?


## Compound assignment semantics

The semantics of compound assignment ops are exactly as if they had been written as
an assignment of the binary operator,
so the following are observably the same in all respects, including error handling.

```datalove
var a: int = 10_000_000_000
let b: int = 20_000_000_000
set a = a + b
debuglog (a, b)
```

```datalove
var a: int = 10_000_000_000
let b: int = 20_000_000_000
set a += b
debuglog (a, b)
```

And fixed ints with checked ops:

```datalove
var a: u8 = 10
let b: u8 = 20
set a +?= b
debuglog (a, b)
```

`b` is evaluated, `a` is read, then assigned `a` plus the right-hand side value.
If an error occurs, the code returns early and `a` is unchanged.
`a` can be both immutable and mutably borrowed on the rhs.

```datalove
require module sys/std/int

var a: int = 10_000_000_000
let b: int = 20_000_000_000
set a += b + int.sqrt(ref a)? // Immutable borrow and read.
debuglog (a, b)
```

```datalove
var a: int = 10_000_000_000
let b: int = 20_000_000_000

fun sneaky_edit(mut a: int)
  set a = 1
end fun

set a += b + sneaky_edit(mut a)? // Mutable borrow and write.
debuglog (a, b)
```

Adding compound addition operators, lowered through to compound IR and bytecode ops
immediately improved reduced time spent on the `sum` benchmark by about 45% on every engine
(IR, bytecode, JIT, AOT), mostly by reducing bigint reallocation in the loop.

But not completely eliminating them.


## Mixed-type math ops




## Compound assignment to index projections

Writing this explanation and doing the implementation (having an LLM do the implementation...)
brought yet more dogfooding wins:
the actual existing implementation did not have a sensible evaluation order for
the left-hand and right-hand side of an assigment.
In most prior cases this didn't matter since they were simple without many potential side-effects.
But that wasn't true for assignment to index projections.

todo



---

todo

how does bigint do the compound math in place?
compound assignment to index projection places
todo mixed-type math ops
saturating and wrapping ops
full list of current math ops
