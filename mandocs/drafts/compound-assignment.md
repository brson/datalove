# Compound assignment

Recently I've been proving the performance potential of the language model
with some simple benchmarks and a simple representative demonstration app,
the details of which aren't important.
I have been focusing on the bytecode interpreter and the JIT,
including compilation time and the runtime.
As with writing the high-level docs for the website,
this dogfooding exercise has been highly effective,
finding lots of bugs and semantic gaps.

For bytecode performance I am comparing to Python.
For JIT to Julia.
Both have dynamic typesystems, where Datalove is static,
though Python's bytecode interpreter is amazingly performant
for happy-path synthetic benchmarks,
and Julia of course is 100% jitted via LLVM and is very fast.
Julia unfortunately has slow startup time though.
I am aiming for fast startup time and fast performance.
So far Datalove's startup and compilation time is fast,
and runtime performance is in the ballpark,
enough to give me confidence about the basic data and runtime model
(at least without considering generic functions).
Sometimes performance is still slower than my expectations.
I won't throw out any numbers at this time since they are not rigorous
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
a test of bigint efficiency. Bigints are implemented in the runtime in Rust,
where performance is about representation (are all numbers on the heap? are small numbers
packed into the stack?) and algorithmic efficiency.
But because bigints are non-copyable types whose digits live on the heap
(Datalove has no small-number optimization; every nonzero bigint owns a heap buffer),
the basic semantics of math ops and their interaction with deep-cloning has a significant
impact on bigint performance.

I generally prefer to implement the simplest and clearest surface semantics reasonable to accomplish
a task, and only expand complexity as I better understand the need.
So Datalove only supports a few built-in math operations today:
binary addition, subtraction, multiplication, division, and unary negation (`+`, `-`, `*`, `/`, unary `-`).
Though because one of Datalove's principles insists on strict
[numeric correctness](../principles.md#user-content-numerical-correctness)
the exact formulation of these is
[quirky](../datafun.md#user-content-numerics).

So on the `sum` benchmark,
after squeezing out all the basic performance blunders in the allocator and the bigint implementation
I was finding the bytecode performance still significantly behind Python's bytecode interpreter,
on the order of 2x.

The primary culprit was excessive allocation of bigints.
Let's see our `sum_to` function:

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
Binary ops borrow their operands so they don't themselves require a clone,
but they do produce a fresh value, so a bigint `+` always performs an allocation
(furthermore, because both sides of addition must be the same type,
we have to widen the `u32` counter to `int`, another allocation).
It _seems_ like we could instead do that operation in place directly into `total`
without reallocating.

To address the allocating math operator that immediately assigns to one of its operands
here we _could_ do a fairly simple
peephole optimization on `set total = total + <something>`,
the IR for which looks like

```
v3 = widen s1
v4 = add s0, v3
drop v3
drop.tracked s0
store.move.tracked s0, v4
```

Eminently pattern-matchable.
But one of Datalove's principles is that
[nothing happens that was not written](../principles.md#user-content-nothing-happens-that-was-not-written).
I much prefer the language to have surface constructs that map to
the required underlying performance mechanisms.
That both makes the language's performance characteristics clear,
and lets the compiler avoid accumulating passes that slowly eat at compile times.
And the surface operation for this is obvious: compound assignment.

```datalove
set total += i@
```

This common operation, assign `total + i@` to `total`,
implies exactly the optimization needed,
and lowers to a single IR instruction that updates `total` where it lies:

```
v3 = widen s1
add.assign s0, v3
drop v3
```

Aside: even though optimizing bigint math is our motivator,
having compound assignment should be a win for all types for the bytecode
interpreter too, since it can fuse the math and the assignment into a single opcode.
Further aside: I'll probably end up doing the peephole optimization anyway...


## In-place bigint math

This optimization only matters if we can actually do the bigint math without allocating a new buffer.
Can we?

For addition and subtraction, mostly yes.
A Datalove `int` is a pointer to a buffer of 32-bit limbs,
a signed limb count, and the buffer's capacity.
A sum needs at most one limb more than its longer operand, for the carry,
so addition always allocates that extra limb,
and when the carry doesn't use it, it is left spare.
A running total like `total` almost always fits in the buffer it already has,
so `set total += i@` just adds the limbs of `i@` into it:
no allocation, nothing freed.
Only when the sum outgrows the buffer is a new one allocated,
just as the binary `+` would have.
Subtraction is an addition of the negated operand,
and when the signs make it a true subtraction
the result is no longer than the longer operand,
so it fits unless the right-hand side has more limbs than the place has room for.
The right-hand side may even be the place itself, `set a += a`,
since each limb of the operand is read before the same limb of the place is written.

For multiplication and division, no.
Each limb of a product depends on many limbs of both operands,
which are read all the way to the end,
so the product is built in a separate buffer that then replaces the place's.
Division is the same.
`*=` and `/?=` are still a single instruction,
but they allocate just as the binary forms do.


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
set a +!= b
debuglog (a, b)
```

`b` is evaluated, `a` is read, then assigned `a` plus the right-hand side value.
If an error occurs, the code returns early and `a` is unchanged.
`a` can be both immutably and mutably borrowed on the right-hand side.

```datalove
var a: int = 10_000_000_000
let b: int = 20_000_000_000

fun double(ref n: int): int
  ret n + n
end fun

set a += b + double(ref a) // Immutable borrow and read.
debuglog (a, b)
```

```datalove
var a: int = 10_000_000_000
let b: int = 20_000_000_000

fun sneaky_edit(mut a: int): int
  set a = 1
  ret 5
end fun

set a += b + sneaky_edit(mut a) // Mutable borrow and write.
debuglog (a, b)
```

Because the right-hand side is evaluated first,
the second example adds to the `1` that `sneaky_edit` wrote,
not to the original `a`.

Adding compound assignment operators, lowered through to compound IR and bytecode ops,
immediately reduced time spent on the `sum` benchmark by about 45% on every engine
(IR, bytecode, JIT, AOT), mostly by reducing bigint reallocation in the loop.

But not completely eliminating it:
`i@` still widens the counter into a freshly allocated `int` every iteration.
More on that later.


## Compound assignment to index projections

Writing this explanation and doing the implementation (having an LLM do the implementation...)
brought yet more dogfooding wins:
the actual existing implementation did not have a sensible evaluation order for
the left-hand and right-hand side of an assignment.
In most prior cases this didn't matter since they were simple without potential side-effects.
But that wasn't true for assignment to index projections.

Compound assignment works on any place `set` can write,
including elements reached through `?` and `!` index steps:

```datalove
set counts[w]! += 1
set grid[r]?[c]? *= 2
set rows[i]!.total +!= amount
```

The place is evaluated once, so each key is computed and each lookup done a single time,
and the update happens in the element where it lies.
A bare map index, `set m[k] += 1`, is rejected:
a set on a bare index means upsert,
and it isn't obvious that's desirable nor what upserting means for every compound op
(start from zero? is that useful for `*=`?).

The old order reached the place first, then evaluated the right-hand side.
Reaching an element means checking its index and taking a reference to it,
so a right-hand side that grew the collection reallocated it out from under that reference:

```datalove
set xs[0]?.n += grow(mut xs) // Wrote through a freed buffer.
```

Plain `set xs[0]?.n = grow(mut xs)` had the same use-after-free;
compound assignment just made it common.
Now every `set`, plain or compound, evaluates in one order:

1. the right-hand side,
2. the index keys, left to right,
3. the lookups that reach the place, which run none of the program's code,
4. the write, or the compound op.

All of the program's own code runs before any reference into the place exists,
so the example above writes into the grown list,
and an index the right-hand side pushes into range is in range when it's checked.

Some consequences:
the right-hand side's effects happen even if a lookup then fails;
when both could fail, the right-hand side's error wins;
and what the right-hand side moves is gone by the time the place is reached,
so `set m[k]? = k` is a use-after-move and needs `k@`.


## Datalove's current math ops

For reference, here is the full set of arithmetic operators as of today,
and their compound forms.
Each compound op takes exactly the types its binary form does.

| Type                                             | Binary ops                       | Unary ops   | Compound ops                         |
|--------------------------------------------------|----------------------------------|-------------|--------------------------------------|
| `int`                                            | `+` `-` `*` `/?` `/!`            | `-`         | `+=` `-=` `*=` `/?=` `/!=`           |
| `f32`, `f64`                                     | `+` `-` `*` `/`                  | `-`         | `+=` `-=` `*=` `/=`                  |
| `u8`..`u64`, `i8`..`i64`, `index`, `offset`      | `+?` `-?` `*?` `/?`, `+!` `-!` `*!` `/!` | `-?` `-!` | `+?=` `-?=` `*?=` `/?=`, `+!=` `-!=` `*!=` `/!=` |

Bigints can't overflow, so they get the bare `+`, `-` and `*`,
but division can still divide by zero, so it is checked.
Floats follow IEEE 754 and get the bare operators throughout.
Fixed-width integers get only checked operators.
The `?` forms return `none` from the enclosing function on overflow or division by zero,
and the `!` forms return an error,
so they require the function to return an option or a result, respectively.
A compound op that fails leaves its place unchanged.

Wrapping and saturating arithmetic is available as library functions,
like `u8.add_wrapping` and `u8.add_saturating`.


## Future work

Several other near-term language additions were suggested
by this round of profiling.
As mentioned previously,
math binops require both operands to have the same type,
and when the type is a bigint,
that means they both need to be on the heap.

```datalove
fun sum_to(limit: u32): !int
  var total: int = 0
  var i: u32 = 1
  loop while i <= limit
    set total += i@
    set i +!= 1
  end loop
  ret ok total
end fun
```

Often one side of these ops doesn't need to be large:
the `i` in the above is a `u32` but must be widened to `int`,
an expensive thing to do in a loop.
To make that cheap I could give bigints a small-size optimization,
but I'm disinclined to add the representational complexity and additional branching,
and as always I prefer that performance not rely on optimizations that aren't implied by
the surface language.
I'm instead thinking of having binops support limited mixed-type operations,
where the two sides can have different types, where one losslessly converts to the other
(I suspect Datalove will also eventually get auto-widening of numerics in general,
at least as a mode that is active for interactive/script use).

Secondly, this round of work revealed that the standard library
widely uses a less-than-ideal pattern for loop counters:

```datalove
fun repeated<T>(ref elem: T, n: index): [T]
  var built: [T] = []
  var i: index = : index / 0
  loop while i .< n
    call push(mut built, elem@)
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret built
end fun
```

This maintains an `index`-type loop induction variable `i`
and uses wrapping addition to increment it
with the `add_wrapping_index` intrinsic.
The main problem here is the wrapping math to avoid handling overflow.
It does this to avoid the `+?` operator,
a checked operator that requires the containing function to return an option type.
It's provably not possible for the `index` to overflow _here_,
but needing to use explicit wrapping for loop iteration leaves lots of room for accidental errors,
the reader needing to think hard about the meaning of every instance it occurs.

While I expect to add built-in saturating and wrapping math ops,
`+|` and `+%` a la Zig,
the solution here is a looping construct that iterates collections.
So that's probably coming pretty soon.
