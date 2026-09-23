# The Runtime Allocator, and What It Puts on the Rust Heap

Asked after a global-allocator experiment showed up in a crash inside
`AllocLocal::alloc`: does the runtime allocator wrap Rust's allocator, or Vecs, instead of
hitting mmap directly?

**Short answer: the memory it hands to programs is mmap, directly. Its own bookkeeping is
on the Rust heap, and one piece of that bookkeeping is on by default in release and costs
17%.**

## The payload path is mmap, as advertised

`crates/datalove-rt/src/impls/alloc.rs`, the non-wasm implementation. Nothing here goes
near Rust's allocator:

- **Small** (up to 4096 bytes): `allocate_page_for_size_class` mmaps one 4096-byte page,
  carves it into `PAGE_SIZE / block_size` blocks and pushes each onto an intrusive free
  list. `alloc_small` pops the free list and only mmaps when it is empty. Ten size classes,
  8 through 4096.
- **Large** (over 4096 bytes): `alloc_large` mmaps per allocation, `free_large` munmaps it.

So the answer to the question as asked is no: a datalove program's data is not coming out
of `malloc`. The wasm implementation does use Rust's global allocator, but that is stated
at the top of the file and is a different target.

## Three things are on the Rust heap

```rust
pub struct AllocLocal {
    free_lists: [*mut FreeListNode; NUM_SIZE_CLASSES],
    small_pages: Vec<Page>,
    large_pages: Vec<Page>,
    active_allocations: HashMap<*mut u8, AllocationInfo>,
    leak_check_mode: LeakCheckMode,
}
```

`free_lists` is inline and fine. The other three are not:

**`active_allocations` is one hash-map insert per allocation and one remove per free.** It
exists for leak detection. It is a `std::collections::HashMap`, so the key -- a raw pointer,
already uniformly distributed -- is run through SipHash, and the value is an
`AllocationInfo` of size, align, count and an `Option<Backtrace>`. This is what appeared in
the crash backtrace that prompted the question: `AllocLocal::alloc` -> hashbrown rehash ->
`free_buckets` -> the Rust global allocator.

**`large_pages` is pushed per large allocation, and `free_large` scans it linearly:**

```rust
if let Some(idx) = self.large_pages.iter().position(|page| {
    let page_start = page.ptr as usize;
    ...
```

So freeing one of *n* live large allocations is O(n), and a workload holding many of them
is quadratic.

**`small_pages` is pushed per 4096-byte page**, which is the mild one -- once per 512
allocations in the 8-byte class.

## Leak tracking is on in release, by default

```rust
fn from_env() -> Self {
    match std::env::var("DATALOVE_LEAK_CHECK").as_deref() {
        Ok("warn") => LeakCheckMode::Warn,
        Ok("panic") => LeakCheckMode::Panic,
        Ok("panic-backtrace") => LeakCheckMode::PanicWithBacktrace,
        Ok("ignore") => LeakCheckMode::Ignore,
        _ => LeakCheckMode::Panic,
    }
}
```

An unset `DATALOVE_LEAK_CHECK` gives **`Panic`**, not `Ignore`. So every release `datalove`
run pays a hash insert on every allocation and a hash remove plus a parameter cross-check
on every free.

Measured on a program that builds and drops a 64-element list of 16-byte strings twenty
thousand times -- same binary, only the environment variable changed, three alternating
rounds:

| | min | mean |
|---|---|---|
| default (`Panic`) | 593ms | 610ms |
| `DATALOVE_LEAK_CHECK=ignore` | 506ms | 517ms |

**17% of total wall time**, and about 19% of the work once the ~55ms of startup is taken
off both sides. `Ignore` mode skips the bookkeeping entirely rather than recording and
discarding, which is why the saving is the whole of it.

The default is presumably deliberate: several fixtures exist only to be checked by the leak
detector -- `135_nested_early_return_drops.dfs` says "There is nothing to check but the
leak detector and the answer" -- and defaulting to `Panic` means the suite catches leaks
without anyone having to set anything. The cost is that production runs are paying for the
test suite's assertions.

`botdocs/plan-leak-check-default.md` works out what to do, and the next section is what
was done.

## What was changed

**The default is `Ignore`**, and that is the whole of it. One arm of `from_env`.

The interesting part is what was tried first and undone. The three `free` checks --
double free, untracked pointer, and a size/align/count that disagrees with the allocation --
look like things a release build should keep, so they were moved out from behind the mode.
Then it became clear they cannot be: **they are built on the same record the leak report is
built on**, so keeping them keeps the hash map, and the hash map is the entire cost. Measured
that way, the new default and `panic` were identical, and production still paid 13%.

So the checks went back under the mode, which is where they belong for a reason that is about
the language rather than about the allocator: **datalove is safe, and a program cannot reach
`free` with the wrong arguments by being written badly.** Only the compiler can do that, by
lowering a drop wrongly, and the test suite is what watches for it -- which is why the
justfile turns the checks on and a shipped binary leaves them off.

`LeakCheckMode` is therefore one switch rather than two by design, and its doc comment now
says so, because "no leak detection" undersells what `Ignore` gives up.

Interleaved, three rounds, on a noisier machine than the table above -- read the pattern, not
the values:

| | min, three rounds |
|---|---|
| before, default (tracked) | 651 / 724 / 736ms |
| before, `ignore` (untracked) | 550 / 552 / 619ms |
| **after, default** | **528 / 558 / 575ms** |
| after, `panic` | 642 / 708 / 650ms |

The new default lands on the untracked path and `panic` lands on the old default, which is
what it should do. The saving is the 17% from the table above, now not paid by anything that
ships.

The justfile sets `DATALOVE_LEAK_CHECK=panic` on `test`, `test-64`, `test-slow` and
`test-parallel`, covering the three recipes CI runs. Children inherit, so it also reaches the
executables the AOT backends build and the `datalove` binary the cli tests spawn.

## Two more things worth naming

**The largest size class gets one block per syscall.** `SIZE_CLASSES` tops out at 4096 and
`PAGE_SIZE` is 4096, so `num_blocks = PAGE_SIZE / block_size` is 1 for that class. And
`size_to_class_index` rounds up, so **every allocation from 2049 to 4096 bytes is its own
mmap**, as is every allocation above 4096 via the large path. Two syscalls per
allocate-free pair, with no page cache in front of them.

This was not what cost anything in the workload above -- its system time is 5ms of 593ms,
because 16-byte strings and small lists live in the reused free lists -- so it is a
structural observation rather than a measured one. A program working in buffers of a few
kilobytes would find it.

**Small pages are never returned.** `free_small` pushes back onto the free list and the
page stays mapped until shutdown. That is a reasonable choice for a short-lived process and
worth knowing about for a long-lived one.

## Not related to startup

Worth saying, because the two got discussed together: the runtime allocator is **not** why
a cold `datalove` start spends 40% of its time in allocation. That 40% is the compiler's
own Rust allocations -- salsa's tables, the parser's vectors, the AST and IR. `datalove_rt`
is 3 samples out of a full profile of compiling the standard library. Comptime evaluation
runs the interpreter, so the runtime is reachable during a compile, but the library does
not lean on it.
