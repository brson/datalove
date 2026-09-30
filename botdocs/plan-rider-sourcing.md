# Plan: sourcing the runtime, sys packages and riders

Implements [mandocs/future-designs.md](../mandocs/future-designs.md),
"2026/09/30 - Locating and loading the runtime, sys packages, and sys riders".
Read that first; this is the order of work, not the design.

The end state is that `cargo install datalove-cli` gives a binary that finds
its standard library, its riders and its runtime without a checkout, and that
a datalove package outside `sys/` gets there by the same rules.

## Where things stand

Already done, and this plan builds on it:

- The runtime is three crates. `datalove-rtdt` declares the data that crosses
  the boundary, `datalove-rti` the calls, `datalove-rt` implements them.
- A rider depends on `rtdt` and `rti` only. It reaches the runtime through the
  table on the handle, so a rider library has no undefined runtime symbols and
  the process loading it exports none.
- `rider_build` synthesizes one native component per rider set, as a dylib for
  the interpreter to load or a staticlib for an AOT program to link.
- The interpreter and the JIT resolve `sys/std`'s natives from the rider
  linked into the binary, so they need no cargo. Only AOT builds a component.

What blocks publishing:

- Internal path dependencies carry no `version`, so every crate fails
  `cargo package`.
- No crate but `bcts` has a `description` or `repository`; crates.io requires
  a description.
- `sys/std/rider/build.rs` reads `../rider.dli`, above its own package root,
  so cargo cannot package the file it needs.
- The component `rider_build` synthesizes names `datalove-rt`, `rtdt` and
  `rti` by absolute path, baked from `env!("CARGO_MANIFEST_DIR")`. An
  installed binary needs its checkout where it was.
- `datalove-stdlib` embeds `sys/` with `include_str!` from outside its own
  package root, the same problem as the rider's interface.

## Phase 1 --- the ABI check

First, because every later phase widens the ways a rider and a runtime can
disagree, and because it is small and useful on its own. Explained in full
under *The ABI check* below.

1. `gen-rti.py` emits `TABLE_SHAPE`, a hash over the generated table's field
   names and types, into `table.rs`.
2. `datalove-rti` gains `ABI_VERSION`, a `const fn` mixing `TABLE_SHAPE`,
   `EXPORTED`, and the sizes and offsets of the `rtdt` types that cross.
3. `rider_build` writes `#[no_mangle] pub static DLR_ABI_VERSION: u64 =
   datalove_rti::ABI_VERSION;` into the component's `lib.rs`, so no rider
   author has to do anything.
4. `rider_load::load_rider_library` reads that symbol before it looks up any
   `dlr_*` and refuses a library whose value differs or is absent.
5. Work dirs are keyed by `ABI_VERSION`, so two datalove binaries sharing a
   cache directory do not build over each other.

Testable now, with no dependency on any other phase.

## Phase 2 --- `manifest.toml`

The rider's crate name and version have to survive the Rust source being
stripped. Today `rider_build::extract_crate_name` reads `rider/Cargo.toml` and
scans for a `name` line, which is both ad hoc and gone once the crate is
stripped out of the package.

1. Define the format. `[rider] name`, `[rider] version` to start. Assume it
   grows: parse with a struct that rejects unknown keys loudly rather than
   ignoring them, so a later field is a clear error on an older datalove.
2. Parse it in `datalove-datafun-pkg`. Module discovery stays by convention;
   only the rider's identity comes from the manifest.
3. `RiderDescriptor` carries name and version alongside its source. Retire
   `extract_crate_name`.
4. Write `sys/std/manifest.toml` naming `datalove-rider-sys-std`.
5. Rename the crate `datalove-rider-std` to `datalove-rider-sys-std`,
   reserving the `-sys-` namespace.

A package with a `rider/` directory and no manifest should fail with a message
saying a manifest is needed, not fall back to guessing.

## Phase 3 --- move the interface inside the rider crate

`sys/std/rider.dli` becomes `sys/std/rider/rider.dli`. Small, and it clears
one of the publishing blockers outright.

1. Move the file.
2. `sys/std/rider/build.rs` reads `rider.dli` in-crate. It is then packageable,
   which it is not today.
3. `package_load` looks for `rider/rider.dli` rather than `rider.dli`.
4. `datalove-stdlib/build.rs` embeds from the new location.
5. Note the deliberate duplication: the published rider crate carries the
   interface for its own codegen, and the published datalove package carries
   it for the compiler. Each is self-contained; neither reads the other.

## Phase 4 --- `BuildInfo`

1. A small crate --- `datalove-buildinfo` --- whose build script emits the
   `BuildInfo` the rest of the tree reads. `Prod { version }` or
   `Local { git_sha, abs_path }`.
2. Deciding which: `Local` when the build is happening inside a git checkout
   of this repository, `Prod` otherwise. Record the decision in the build
   script's output so `datalove --version` can print it; a binary that cannot
   say which kind it is will waste someone's afternoon.
3. Replace `rider_build::workspace_root_dir()`, `aot`'s and
   `datalove-stdlib`'s `env!("CARGO_MANIFEST_DIR")` derivations with
   `BuildInfo`.
4. A `Local` binary whose `abs_path` no longer exists must say exactly that.
   Falling through to cargo produces an error about a missing path dependency
   that names none of the things a user could act on.

**`cargo install --git` is a hole in the two-scenario model.** It builds from a
clone in a temporary directory and deletes it, so a `Local` binary would bake
a path that is already gone. Either detect it and record `Prod` with the
resolved version, or refuse, or accept that `--git` installs are unsupported
and say so. It should not be discovered by a user.

## Phase 5 --- sys packages from a crate

1. New crate `datalove-sys-packages`, embedding every package's `.dfm` modules
   and `rider/rider.dli`, and not the rider's Rust source. This is
   `datalove-stdlib`'s `build.rs` with its own package as the root, so the
   `include_str!` paths stay inside it and it becomes packageable.
2. `datalove-stdlib` keeps what is about *this binary* --- the linked rider
   natives, the work dir --- and takes its package sources from the new crate.
3. `Prod` loads from the embedded crate; `Local` loads from
   `BuildInfo::abs_path`, which `WorkspaceDescriptor::load_sys_dir` already
   does.
4. Keep `embedded_matches_tree`'s guarantee: what is embedded must equal the
   tree it was built from, including the rider crate paths.

## Phase 6 --- the component names riders by version or by path

1. `rider_build` emits registry dependencies with exact versions
   (`=0.1.0`, from the manifest) under `Prod`, and path dependencies under
   `Local`.
2. `datalove-rt`, `rtdt` and `rti` the same way: exact version under `Prod`,
   `BuildInfo::abs_path` under `Local`.
3. Keep naming `rtdt` and `rti` in the generated manifest directly, as now.
   The doc has riders propagating `index-64` by convention, which is right,
   but naming the two crates here means the feature is set by the process that
   has to match rather than by whether every rider author remembered.
4. First run under `Prod` fetches from the network. Decide and document what
   happens with no network: a clear error, a pre-warm at install time, or
   vendoring. It also decides whether CI and air-gapped machines can work.

## Phase 7 --- exercise the unprivileged path

The interpreter short-circuits `sys` riders by linking them, which is worth
keeping and means the general path goes untested for exactly the riders the
test suite uses most.

1. A flag --- `DATALOVE_BUILD_SYS_RIDERS=1` --- that ignores the linked
   natives and drives `sys/std` through discovery, component build and dlopen
   like any other rider.
2. One CI configuration with it set. It belongs beside `test-64` in the
   justfile: same suite, different arrangement underneath.

## Phase 8 --- publish, having tested the production path first

1. Give every internal path dependency a `version` next to its `path`, and
   every crate a `description` and `repository`. `cargo package --workspace
   --no-verify` names each failure in turn.
2. Mark the test and bench crates `publish = false`. Cargo strips path-only
   dev-dependencies, so `datalove-exampletest`, `datalove-tests`,
   `datalove-rt-tests` and `datalove-bench` need no versions and need not
   publish.
3. Build a local registry from `cargo package` output and point
   `[source.crates-io]` at it with `replace-with`. Run the suite with a `Prod`
   `BuildInfo` against it. This is the whole of the answer to "the production
   path cannot be tested until the crates are published": it can, against
   crates that were never published, and it should run in CI rather than once
   by hand before a release.
4. `cargo publish --workspace`, stable since Rust 1.90, orders the publishes
   and strips `path`. It is not atomic: a failure part way leaves some crates
   published. Verification happens before any upload, so the likely causes are
   network and rate limits rather than manifest errors.
5. `bcts` stays on its own version. Everything else goes at `0.1.0`.

## Ordering

Phase 1 stands alone and should land first. Phases 2 and 3 are independent of
each other and of 1. Phase 4 must precede 5 and 6, which both read
`BuildInfo`. Phase 6 needs 2, for the name and version it puts in the
manifest. Phase 7 needs 6, there being no unprivileged path to test before
that. Phase 8 needs all of them.

Nothing before phase 6 changes what an installed binary can do, so phases 1
through 5 are all verifiable against the tree as it is.

## The ABI check

### What can go wrong

A host and a rider are compiled separately and have to agree on three things:

1. The layout of the `rtdt` types that cross --- `TyDesc`, `String`, `Int`,
   `List`, `Index`.
2. `RtiTable`'s field order and each field's signature. The rider indexes into
   the table; field order *is* the ABI.
3. That `RtLocal`'s first field is the table pointer, which is how the rider
   finds the table at all.

Ways they come apart, in rough order of likelihood:

- **Independent version resolution.** Under `Prod` the rider names
  `rtdt = "0.1"` and cargo resolves the component's graph without reference to
  what the host was built against. A different minor version with a changed
  layout is a silent mismatch. Phase 6's exact pinning is the prevention; this
  is the detection.
- **Features, not versions.** `index-64` changes `IndexRepr` between `u32` and
  `u64`, so the same version of `rtdt` has two layouts. Nothing about a
  version number says which one a library holds.
- **A stale library.** Two datalove binaries built from different revisions of
  a checkout, sharing `~/.cache/datalove/work`. Keying the work dir prevents
  it; the check catches what prevention missed.
- **A hand-built rider**, compiled against a different revision of `rti` than
  the datalove that loads it.

### Why nothing catches it now

The table is reached through a pointer at run time, so there is no symbol to
resolve and no link step to fail. That was the point --- resolving a symbol
backward into the executable is what is not portable --- but the cost is
losing the linker as an accidental checker.

Before the table, a changed signature gave either a link error, if a symbol
had gone, or silent undefined behaviour, if only its arguments had changed.
With the table it is always the second. A mismatch does not fail: it calls a
function with the wrong arguments, or reads a field at the wrong offset, and
carries on.

### The check

One value, computed independently on each side from its own headers, compared
once when a library is loaded.

`datalove-rti` gains:

```rust
/// What a rider and a runtime must agree on, as one number.
pub const ABI_VERSION: u64 = abi_version();
```

computed at compile time from:

- `table::TABLE_SHAPE`, a hash `gen-rti.py` writes over the generated field
  names and types. Field order and every signature are in it, so any
  regeneration that changes the table changes this.
- `table::EXPORTED`, the field count.
- `size_of` and the `offset_of!`s of the `rtdt` types that cross. Sizes alone
  would miss two fields of equal width being swapped, which is why the offsets
  are in it too.

Mixed with something that actually mixes --- FNV-1a over the bytes is enough.
This detects difference; it is not a security boundary, and nobody is trying
to forge it.

Note what is deliberately *not* in it: the version numbers of `rtdt` and
`rti`. A patch release that changes no layout should not invalidate every
rider in the wild, and a version number does not say whether `index-64` is on.
Layout facts trigger exactly when the layout differs, which is the question
being asked.

The component exports it. `rider_build` already generates the component's
`lib.rs`, so this is one more generated line and no burden on a rider author:

```rust
#[no_mangle]
pub static DLR_ABI_VERSION: u64 = datalove_rti::ABI_VERSION;
```

Because the component is compiled against whatever `rti` the riders resolved,
the value it exports describes the rider side. The host compares against its
own.

The host checks it in `rider_load::load_rider_library`, before it looks up a
single `dlr_*`:

```rust
let theirs: libloading::Symbol<*const u64> = lib.get(b"DLR_ABI_VERSION")
    .context("rider library exports no ABI version; it was built by \
              a datalove too old to say, or is not a rider library")?;
if unsafe { **theirs } != datalove_rti::ABI_VERSION {
    bail!("rider library was built against a different runtime interface \
           (library {:#x}, this datalove {:#x})", unsafe { **theirs },
          datalove_rti::ABI_VERSION);
}
```

A missing symbol is a failure in its own right, not something to shrug at: an
older library, or a library that is not one of ours.

### What it costs and what it does not cover

One `dlsym` and one comparison per library load. Nothing per call, and nothing
in a rider's own code.

It is a whole-interface check. Equal means compatible and different means
incompatible, with nothing in between --- no partial compatibility, no
per-function negotiation, no optional fields. That is the right granularity
while the interface is one table and the only consumers are built from the
same tree. A versioned table with appendable fields is the thing to do if
riders ever ship on their own schedule, and this check is what would tell us
we need it.

### The AOT side

An AOT program links the runtime by name, so the linker checks signatures
there, and the component carries one runtime built together with the riders.
The residual risk is different: the emitted code has struct offsets in it,
from `tydesc_emit`'s `size_of` and `offset_of!` at *compiler* build time, and
the component's `datalove-rt` might be a different version than the compiler
was built against. Exact pinning in phase 6 prevents it, and the linker
catches a signature that moved.

If that turns out to be worth belt and braces, the emitted program can call
the runtime for its `ABI_VERSION` at startup and compare against one the
compiler baked in, aborting on mismatch. It costs a call per program start and
is worth doing only if pinning proves not to hold in practice.
