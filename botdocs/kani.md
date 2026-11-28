# Bounded Proofs Under Kani

What the runtime's arithmetic is proved of under Kani, how to run and write
the harnesses, and what they cannot say. Written November 2026, when the
first harnesses went in: the layout functions, the byte copies, the
allocator's size classes and the big integer's limb arithmetic. The value
layouts, shared since with the IR, came after.

## Contents

- [The problem](#the-problem)
- [What Kani checks](#what-kani-checks)
- [What is proved](#what-is-proved)
- [Running it](#running-it)
- [Writing a harness](#writing-a-harness)
- [What is not covered](#what-is-not-covered)

## The problem

The runtime's layout functions are the ABI between compiled code and the
runtime: generated code writes a key at the offset `map_leaf_node_layout`
gives and the runtime reads it back at the same. The unit tests try a few
sizes, the proptests sample a few hundred, Miri checks the paths a test
happens to run, and `layout_conformance_tests` holds the backends to the
runtime over a corpus of types. None of them says anything about a key size
nobody wrote down. A value offset that is misaligned for one key size, or a
copy that moves eight bytes of twelve, passes all four if the size is not in
the corpus.

Kani checks every size. A harness looks like a unit test, but its inputs are
`kani::any()`, which stands for every value of the type at once, and the
verifier either proves every assertion for every input within a bound or
prints the input that breaks one.

## What Kani checks

Kani is a bounded model checker for Rust. It compiles a harness and the code
it calls to a formula through CBMC and asks a SAT solver whether any input
reaches a failing check. The checks are the ones the code already carries:
every `assert!` and `debug_assert!`, arithmetic overflow, an index out of
bounds, and the validity of every dereference in unsafe code, of a null,
dangling, misaligned or out-of-bounds pointer. So a harness that only calls
a function and asserts nothing still proves the function has no overflow or
invalid access on any input it admits.

A loop has to be bounded with `#[kani::unwind(n)]`, one more than the
iterations it can take, so a proof is over inputs up to a size, not over all
sizes. Within the bound it is exhaustive, which is what makes it different
from a proptest.

## What is proved

Each harness, where it lives, what it proves and over what, and how long
it takes in a debug build on four CPUs. Every harness also proves the code
it calls free of overflow and invalid access on those inputs.

| Harness | File | Proves | Bound | Time |
|---------|------|--------|-------|------|
| `align_up_rounds_up` | `crates/datalove-rtdt/src/layout.rs` | `align_up` lands on a multiple of the alignment, at or past the value, less than one alignment past | any power of two to a page; values short of overflow | under a second |
| `payload_offsets_follow_their_tags` | same | the option, enum and result payload offsets follow their tags, aligned, with no spare padding | any alignment to a page | under a second |
| `sequence_pads_minimally` | same | a tuple's or struct's parts each follow the last, aligned to itself, with less than one alignment of padding; the whole is aligned to its most aligned part and padded only to it | up to four parts, sizes to a mebibyte, any alignment to a page | 3 seconds |
| `sequence_fits_exactly_when_its_true_size_does` | same | the checked form lays a sequence out exactly when its true size, worked out in `u64`, fits a `u32`, and then at that size; past it, it returns `None` rather than wrapping | up to three parts of any size | 4 seconds |
| `enum_pads_minimally` | same | the same of an enum's discriminant and largest payload, and every variant's payload, at its own offset, fits | up to three payloads | 2 seconds |
| `option_pads_minimally`, `result_pads_minimally` | same | the same of a tag and its payload; a result holds the ok value and the error both, each aligned | any payload | under a second |
| `map_internal_node_pads_minimally`, `map_leaf_node_pads_minimally`, `set_internal_node_pads_minimally`, `set_leaf_node_pads_minimally` | same | each node part follows the last, aligned to itself, with less than one alignment of padding; the node's alignment is the largest part's and its size a multiple of it | key and value sizes to a mebibyte, any alignment to a page | about a second each |
| `table_columns_follow_one_another` | same | a table's columns follow one another in its data allocation, each aligned, and the allocation holds the last and is a multiple of the table's alignment; a capacity of zero allocates nothing | three columns to a kilobyte each, capacity to a thousand | 12 seconds |
| `copy_value_copies_the_value` | `crates/datalove-rtdt/src/bytes.rs` | the copy writes the source's bytes to the destination and changes nothing else, with source and destination overlapping by any amount in either direction | sizes to 32, in a 64-byte buffer | 4 minutes |
| `swap_value_swaps_the_values` | same | the swap exchanges the two values' bytes and leaves the rest | sizes to 40 | 35 seconds |
| `size_class_is_the_smallest_that_fits` | `crates/datalove-rt/src/c/impls/alloc.rs` | every small size goes to the smallest class that holds it | every size to the small limit | under a second |
| `align_up_ptr_moves_less_than_the_alignment` | same | aligning a pointer up lands on a multiple, moving less than the alignment | any power of two to a mebibyte; addresses short of overflow | under a second |
| `compare_magnitude_orders_values` | `crates/datalove-rt/src/c/impls/int_math.rs` | the magnitude comparison orders as the values do | magnitudes to three limbs | a second |
| `add_magnitude_into_adds`, `sub_magnitude_into_subtracts` | same | the sum and difference of two magnitudes are the `u128` sum and difference, fit the capacity given, and carry no leading zero | magnitudes to three limbs | 3 seconds each |
| `add_assign_magnitude_adds_in_place`, `sub_assign_magnitude_subtracts_in_place` | same | the same for the in-place forms `set a += b` and `set a -= b` run, in either order of magnitude | magnitudes to three limbs | 3 and 10 seconds |
| `mul_magnitude_into_multiplies_limbs` | same | a one-limb product is the `u128` product, with no leading zero | one limb by one | 6 seconds |
| `mul_magnitude_into_stays_in_bounds` | same | a wider product writes in bounds, overflows nothing and panics nowhere | two limbs by two | 6 seconds |
| `div_magnitude_into_by_a_limb_stays_in_bounds` | same | division by a limb writes in bounds, overflows nothing, panics nowhere, and leaves no leading zero | dividends to three limbs | 3 seconds |

"Pads minimally" is not "packed". Every part is aligned, and a `u8`
followed by a `u64` still has seven bytes between them. What the harnesses
rule out is more padding than alignment needs: each part starts at the first
aligned offset past the last, which is the C rule. The order is the declared
one, never rearranged to save space. With each part following the last by
less than one alignment, the layout is the unique one the order and the
alignments allow, so a second computation of it, a backend's or a writer of
descriptors', has one answer to agree with.

The value layouts are the ones both sides use. `datalove_datafun_ir::layout`
used to carry its own copy of the tuple, struct, enum, option and result
arithmetic, held to the runtime's only by `layout_conformance_tests` over a
corpus. Now it calls the pure functions in `datalove_rtdt::layout`
(`sequence_layout`, `enum_layout_of`, `option_layout_of`,
`result_layout_of`), which take sizes and alignments rather than
descriptors, and the runtime's descriptor walk calls them too. Proving them
covers both, and the conformance tests are left checking the walk from a
type to its parts.

Every addition and multiplication in the runtime's layouts is checked, and a
layout past `u32::MAX` panics. A size or offset is a `u32`, so such a value
or node cannot be laid out at all, and wrapping would lay it out short and
let a write to its last part land past its end. The other harnesses bound
sizes to a mebibyte and so never reach the check; the sequence harness
above takes any size, and proves the check fires exactly when it should.
The frame layout is a sequence too, of values, slots and tracking bytes,
and calls the same function. So does the interpreter's row layout for a
table constant, and the Cranelift backend's constant buffers, of a list's,
a tensor's or a table's elements end to end, are sized by `array_layout`,
a checked product of the element's size and the count. Cranelift's own
`TypeLayout` is gone too: there is one, in `rtdt::layout`, re-exported by
the IR.

The big integer's arithmetic was refactored to make it provable: each
operation now writes into a limb slice the caller allocated and returns how
many limbs it used, with the allocation in one place,
`magnitude_in_runtime`. Before, each allocated from the runtime as it went,
and Kani cannot model `mmap`. The in-place forms gained the same shape, so
the borrow and carry logic that `set a += b` runs is proved too.

The harnesses were held to planted faults before they went in: a 12-byte
swap changed to move 8 bytes, the value alignment dropped from the leaf's
layout, and an in-place subtraction that forgets to borrow. Each failed the
harness that covers it. The value layout harnesses were held to an
aggregate not padded at the end, an enum sized to its last payload rather
than its largest, and a result sized without the error. The first draft of
the result harness passed the last of these: its expected payload was built
the way the code builds it, so it agreed with a wrong one too. It now checks
that each arm fits.

### Where the arithmetic proofs stop

A product or quotient wider than one limb is proved to stay in bounds and
panic nowhere, and not to be the right value. Holding the schoolbook product
to a `u128` product asks the solver to show two multipliers equivalent,
which is the problem SAT solvers are worst at: it ran for two hours at two
limbs by two and ten minutes at two by one without finishing, and the
one-limb quotient against `u32` division, a 64-bit divider against a 32-bit
one, ran ten minutes too. Knuth's algorithm D could not be proved even to
stay in bounds: with a divisor of two limbs and a dividend of two or three,
CBMC ran out of memory or past fifteen minutes under both solvers, because
the quotient-digit estimate multiplies and divides 64-bit values and the
inner assertions need that arithmetic reasoned about. The values of
multi-limb products and quotients, and the long division's safety, rest on
`int_math_tests` and the fixtures.

## Running it

Kani installs beside cargo and brings its own pinned nightly:

```
cargo install --locked kani-verifier --version 0.68.0
cargo kani setup
```

The bundle goes under `~/.kani` and the nightly into rustup. Then:

```
just verify-kani
```

runs every harness of both crates, and the table harness again under
`index-64`, where a capacity is a `u64`. One harness alone is

```
cargo kani -p datalove-rtdt --harness copy_value_copies_the_value
```

Kani replaces `assert!`, `panic!` and their relatives with its own, to
report the condition and the message of a failure. A crate whose prelude
glob-imports the standard ones fails to compile under it with E0659, every
macro ambiguous, and `--no-assert-overrides` is the escape hatch, at the
cost of failures reported as plain panics without the condition. rustmax
0.0.13 re-exported std's prelude and needed the flag; 0.0.14 does not, and
the harnesses run with the overrides in every crate.

CI runs the recipe as its own job, since it alone needs Kani.

## Writing a harness

A harness is a `#[kani::proof]` function in a `#[cfg(kani)] mod kani_proofs`
beside the code, as the unit tests are beside it, so that it reaches private
functions. The crate's `Cargo.toml` tells rustc the cfg is expected. The
inputs are `kani::any()` narrowed by `kani::assume`, and the assertions are
plain `assert!`.

What was learned writing the first ones:

- **Masks, not remainders.** `offset % align == 0` with both symbolic is a
  64-bit divider circuit. `offset & (align - 1) == 0` is the same statement
  for a power of two and costs nothing. The first draft of the table harness
  used remainders and the `Vec`-building entry point and ran CBMC out of
  memory on a 16 GB machine; the second uses masks and the slice entry
  point and takes 12 seconds. Which of the two mattered more was not
  separated.
- **Keep multiplications few and narrow.** A symbolic size times a symbolic
  capacity is a multiplier circuit whatever the assumed bounds, because the
  bounds constrain the solution and not the circuit. The table harness has
  six.
- **Bound every loop, and write why.** `#[kani::unwind(n)]` is the loop's
  iteration count plus one. A harness without it that reaches a loop fails
  with an unwinding assertion, which looks like a bug and is not.
- **Prefer the slice entry point.** A function that collects into a `Vec`
  before its loop brings the allocator and `RawVec` into the formula.
  `table_column_offset_of` takes the pairs directly and proves the same.
- **State the bound in the doc comment**, and why it is where it is: the
  largest fixed case of a copy, the page, the small-allocation limit. A
  reader of a passing harness should know what it does not say.
- **Hold a new harness to a planted fault** before trusting it. A harness
  whose assertions are all implied by its assumptions passes everything, and
  so does one whose expected value is computed the way the code computes
  it. State what the result must satisfy, not how to build it.
- **`should_panic` proves that something panics, not that everything
  does.** It passes when any path panics, and a `kani::assert(false)` after
  the call does not help, because Kani counts that failure as a panic too.
  To prove an operation refuses every input past a limit, give it a form
  that returns `None` and prove that one, as `checked_sequence_layout` is.
- **No second multiplier or divider in the spec.** A check of the form
  `value(out) == value(a) * value(b)` is a multiplier equivalence, and the
  solver will not finish it past 32 bits. Prove the value where the operands
  are a limb, and the bounds and absence of panics where they are wider.
- **Run a new harness alone under `timeout`** before adding it to the
  recipe, and read the tail of a failure: "Verification failed" with "CBMC
  appears to have run out of memory" above it is the solver giving up, not a
  counterexample. A real failure lists checks with `Status: FAILURE` and
  their lines.

## What is not covered

In rough order of what to do next:

- **The allocator's free lists.** Three or four `alloc` and `free` calls
  with `map` and `unmap` stubbed to Rust's global allocator by
  `#[kani::stub]`, proving blocks aligned and inside their page and the
  list last-in first-out. A proof with the stub says nothing about the real
  mappings.
- **The big integer's values, at a narrower limb.** Made generic over the
  limb, the schoolbook product and Knuth's division could be held to their
  values at `u8` limbs, where the spec's multiplier and divider are small
  enough to solve. That proves the algorithm rather than its `u64`
  instantiation, but the algorithm is most of what can go wrong.
- **Function contracts.** Kani's `#[kani::requires]` and `#[kani::ensures]`
  put a harness's assumptions on the function itself, where they document
  it and let callers be checked against it. They are behind
  `-Z function-contracts` in 0.68, so the harnesses carry the assumptions
  for now.

Out of reach, and not worth a harness: whole B-tree insertions and
removals, anything through `RtLocal::new` and its thread setup, and
anything that reaches salsa. Not worth one either: the node search in
`btreemap.rs`, which is a scan through a comparator the map type supplies,
with nothing in it a proof would say more about than the fixtures do.
