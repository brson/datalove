# A Virtual I/O Layer

A plan for sending every file the compiler reads or writes through one
trait-based interface, in a new `datalove-vio` crate, so that later the
interpreter and compiled programs can do their I/O through it too. Written
November 2026. Phases 1 to 3, the crate and the compiler's own I/O, are
implemented, and so is the embedded system library as a filesystem of its
own; the runtime's I/O is not.

## Contents

- [Why](#user-content-why)
- [What already holds](#user-content-what-already-holds)
- [Where the I/O is](#user-content-where-the-io-is)
- [The interface](#user-content-the-interface)
- [Where the handle goes](#user-content-where-the-handle-goes)
- [The native toolchain stays outside](#user-content-the-native-toolchain-stays-outside)
- [The embedded system library](#user-content-the-embedded-system-library)
- [Megaspace-iostack](#user-content-megaspace-iostack)
- [Phases](#user-content-phases)
- [Open questions](#user-content-open-questions)

## Why

Scripts are going to read and write files, and the debug log may go the same
way. That I/O has to behave the same in the interpreter and in a compiled
binary, and has to be replaceable: by memory in tests, by nothing on wasm, by
something sandboxed when a script is not trusted. So it needs one interface
that every caller goes through, not `std::fs` called where convenient.

The compiler is where to start, because it is the I/O that exists today. But
it calls `std::fs` from five crates, each with its own idea of how to ask
whether a path is a package or a directory. There is no point at which a
caller could hand the compiler a different filesystem. Therefore: a
`datalove-vio` crate holding a synchronous `Fs` trait, a host implementation
and a memory one, with a handle to it passed to every place the compiler
reads sources or writes outputs.

## What already holds

The compiler proper does no file I/O. Parsing, resolution, typechecking,
ownership, lowering, CTFE and the script engine all take source text as
`Arc<str>` through a `WorkspaceDescriptor`
(`crates/datalove-datafun/src/driver/workspace.rs`). Diagnostic rendering
uses paths as labels and never reads the files they name. So the layer has
to reach only the loaders and the drivers, not the salsa phases.

Two pieces are already abstracted. The worldfile parsers in
`crates/datalove-datafun-pkg/src/package_load_worldfile.rs` take `impl Read`.
The runtime's debug log writes to a sink chosen by `DebugOutputMode`
(`Stderr`, `Buffer` or `Disabled`, held in `RtLocal`), which is the shape the
runtime side of this layer will eventually take.

## Where the I/O is

Excluding build scripts, test harnesses and the `doc` command, whose site
builder does its own I/O in `megaspace_pipeliner`.

**Source reads.** These are what the layer is for first.

| Site | What it does |
|---|---|
| `crates/datalove-datafun-pkg/src/package_load.rs` | Lists a library directory, decides what is a package, reads `rider/rider.dli`, checks for `rider/Cargo.toml`, reads every `.dfm` and `.dlt` |
| `crates/datalove-pkg-manifest/src/lib.rs` `load` | Reads `manifest.toml` |
| `crates/datalove-datafun/src/driver/workspace.rs` `find_local_library` | Checks with `is_dir` for `local/` beside a script, or above a `scripts/` directory |
| `crates/datalove-cli/src/compile.rs` `compile_script` | Reads the script |
| `crates/datalove-cli/src/main.rs` | Reads datalit files (`lit-*` commands) and worldfiles (`script-world`) |
| `crates/datalove-repl/src/lib.rs` `read_world`, `src/batch.rs` `run_script` | Reads worldfiles, saved sessions and batch scripts |

**Outputs.**

| Site | What it does |
|---|---|
| `crates/datalove-cli/src/main.rs` | Writes the object file of `aot` without linking, and the `repl --record` trace |
| `crates/datalove-repl/src/app.rs` `/save` | Writes the session worldfile |

**The working directory.** The CLI calls `current_dir()` in
`compile.rs` and five places in `main.rs`: to find `local/`, to show paths
relative to it in diagnostics, and to make a `--run` path absolute. A
virtual filesystem has to say what a relative path means, so the working
directory belongs to the layer rather than to its callers.

**The native toolchain.** Rider builds, linking and loading. See
[The native toolchain stays outside](#user-content-the-native-toolchain-stays-outside).

**Debug output.** Later, not now.

| Site | What it does |
|---|---|
| `crates/datalove-rt/src/c/impls/debuglog.rs` | The debug log, by `DebugOutputMode` |
| `crates/datalove-rt/src/c/impls/alloc.rs`, `src/c/mod.rs` | Leak reports and the stack overflow message, by `eprintln!` |
| `crates/datalove-datafun-interp/src/call.rs`, `src/bytecode/stats.rs` | `DATALOVE_BC_DUMP` and `DATALOVE_BC_STATS` output to stderr |
| `crates/datalove-datafun-cranelift-jit/src/compiler.rs` | The perf map, which must be a real file at the path `perf` expects |
| `crates/datalove-repl-rat/src/term.rs` | Points file descriptor 2 at a temp file with `dup2`, below anything a trait can intercept |

## The interface

Synchronous, behind `Arc`, with the names and signatures of
[megaspace-fs](#user-content-megaspace-iostack) wherever the two overlap:

```rust
pub type DynFs = Arc<dyn Fs>;

pub enum EntryKind { File, Dir, Other }

pub trait Fs: Send + Sync + std::fmt::Debug + 'static {
    fn current_dir(&self) -> &Path;
    fn kind(&self, path: &Path) -> io::Result<Option<EntryKind>>;
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>>;
    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()>;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
}
```

`kind` stands in for `exists`, `is_file` and `is_dir` together, since
package loading needs to tell a file from a directory and `exists` cannot.
`is_file` and `is_dir` are provided on top of it, answering as `Path`'s do, a
path that cannot be looked at being neither. `Other` is a device, socket or
fifo, which a host directory can hold. `current_dir` is what a relative path
is relative to.
`rename` and the durability operations are left out until something needs
them; nothing in the compiler's own I/O does.

Synchronous because every caller is. `package_load` is `async` in name only:
it spawns a thread and a channel per package to do blocking reads, then
waits on it all with `block_on`. With a synchronous `Fs` that machinery has
nothing to do and goes.

Two implementations to begin with:

- `HostFs`, over `std::fs`, with a working directory of its own, fixed when it
  is made, that it resolves relative paths against.
- `MemFs`, a tree in memory, for tests and wasm. Most of the tests that now
  lay out a `tempfile` directory to exercise package loading or
  `find_local_library` can use it instead; `HostFs` keeps a few of its own.

And a third, for the system library: `StaticFs`, read-only, over a table of
paths and contents compiled into the binary.

The crate depends on `std` alone, as megaspace-fs does, so that it can sit
below `datalove-pkg-manifest` now and below `datalove-rt` later without
pulling anything in with it.

## Where the handle goes

The CLI and the REPL each make one `HostFs` at startup and hand it down. As
`&dyn Fs` it goes to:

- `datalove_pkg_manifest::load`
- `package_load::load_world`, `load_library_dir` and `package_from_dir`
- `find_local_library`, `load_local_library`, `local_library_for` and
  `WorkspaceDescriptor::for_command`, which stop taking `cwd` and use the
  filesystem's
- `compile_script` and the CLI commands that read or write a file
- `read_world`, `load_session`, `run_script`, and the app's `/save`

Nothing below the driver changes, since nothing below it does I/O. The
system library brings a filesystem of its own; see
[The embedded system library](#user-content-the-embedded-system-library).

For the runtime, later, a handle sits in `RtLocal` beside
`debug_output_mode`, and a compiled binary installs a `HostFs` when its
runtime starts. How a program reaches it across the native ABI is left for
then. The debug log, leak reports and bytecode dumps would move onto a
`Stdio` part of the same layer, of which `DebugOutputMode::Buffer` is
already a small instance.

## The native toolchain stays outside

Building a rider writes a Cargo crate into the work dir and runs `cargo` on
it. Linking writes `.o` or `.c` files into a temp dir and runs `cc`, which
writes the executable. Loading a rider is `dlopen`. In each case another
program reads the files, so they have to be real files at real paths, and a
virtual filesystem changes nothing about that except to add a way to fail.

So the toolchain is its own boundary, and does not go through `Fs`:

- `crates/datalove-datafun/src/driver/rider_build.rs`: the synthesized crate,
  `write_if_changed` and `write_atomic`, the copied `Cargo.lock`,
  `canonicalize`, looking for the built library, `default_work_dir`
- `crates/datalove-datafun/src/driver/rider_load.rs`: `dlopen`
- `crates/datalove-datafun/src/driver/aot.rs` and `c_aot.rs`: temp dirs,
  object and C sources, `cc`, `ld.lld`, running the executable
- `crates/datalove-paths/src/lib.rs`: the cache directory under
  `XDG_CACHE_HOME` or `HOME`
- `crates/datalove-buildinfo/src/lib.rs`: whether the checkout is still there

It may become a `Toolchain` trait of its own, if a sandboxed or remote build
ever wants one. For now it is enough that this list is where host paths are
used and that nothing outside it adds to the list.

Where the two boundaries meet is the rider crate directory. Package loading
finds `rider/Cargo.toml` through `Fs` and records the directory in
`RiderDescriptor::crate_dir`, and the toolchain hands that path to cargo. That
works for `HostFs`. For a package in a `MemFs` there is no such directory,
and the build has to refuse, as it does now for a workspace with no work
dir. Today it does not know to: a `MemFs` package holding a `rider/Cargo.toml`
is given a crate directory cargo will not find.

## The embedded system library

The binary carries `sys/` inside it, and used to turn a table that
`sys/build.rs` generated straight into package descriptors, with a loader and
a rule for what counts as a package of its own. Now the table is a
`StaticFs`: every module, data file and manifest under `sys/`, at its path,
and each rider interface at the path its file would have, the interface
arriving as the rider crate's `INTERFACE` constant because cargo leaves
`rider/` out of the sys package. `datalove_sys_packages::system_library()`
loads it with `load_library`, through the same `package_load` as a library on
disk.

**Each library brings its own filesystem.** The workspace's `local/` comes
from the driver's `HostFs` and `sys/` from the `StaticFs`, and nothing merges
the two into one namespace. The compiler does not need one: a module's
`origin` is shown in diagnostics and never read. Rooting the embedded files at
`sys/` makes an embedded module's origin `sys/std/list.dfm`, which is what
diagnostics showed when it had none.

**The rider crate directory is the library's to supply.** Where a rider's crate
is is a fact about the host, for the toolchain, and the embedded files cannot
answer it: probing them for `rider/Cargo.toml` finds nothing, which would
quietly move a checkout build's riders from path dependencies to the registry.
So `build.rs` writes the directories into a table of their own,
`RIDER_CRATE_DIRS`, and `system_library()` sets them on the loaded riders.

`sys/tests/embedded_matches_tree.rs` loads `sys/` off disk with the same
loader and holds the two libraries equal, crate directories included.

## Megaspace-iostack

`megaspace-fs`, in brson/megaspace-iostack, is the part that applies; the rest
of `megaspace-io` is network, clocks and task spawning. It is the same idea:
an interface crate with no implementation in it, `Arc<dyn Fs>` passed down,
a real and a simulated filesystem beside it.

Its trait is built for a storage engine, though: `sync_all`, `sync_dir`,
positional reads and writes on an open file, `direct_io`, `fallocate`. The
compiler needs none of that, and does need the file-or-directory question
that megaspace-fs answers only with `exists`. So datalove gets its own trait,
with method signatures kept identical to megaspace-fs for the methods both
have, so that moving onto it later is a change of import rather than of every
caller.

## Phases

What exists: phases 1 to 3. `crates/datalove-vio` holds the interface and
both implementations, and every source read and output write of the compiler's
drivers goes through it. What is missing: the runtime's I/O, and the deferred
work in the open questions.

1. **The crate.** `datalove-vio` with `Fs`, `EntryKind`, `HostFs` and `MemFs`,
   and tests of the two against each other.
2. **Package loading.** `datalove-pkg-manifest`, `package_load` and the
   `local/` lookup take `&dyn Fs`; `package_load` loses its threads and
   channels. The CLI and the REPL make a `HostFs` and pass it. Tests that
   build directories only to load them move to `MemFs`.
3. **Driver reads and writes.** Scripts, datalit files, worldfiles, saved
   sessions, the object file and the record trace go through the handle, and
   `current_dir()` calls go with them.
4. **The runtime, later.** A handle in `RtLocal`, `HostFs` installed by a
   compiled binary's runtime, and a `Stdio` part that takes over from
   `DebugOutputMode`.

After phase 3, `std::fs` outside `datalove-vio` appears only in the toolchain
modules listed above, the debug output listed above, build scripts, test
harnesses and the `doc` command. `sys/build.rs` reads manifests through a
`HostFs`, since `datalove_pkg_manifest::load` takes an `Fs` wherever it is
called from.

## Open questions

- **One namespace.** A `MountFs`, sending each call to the filesystem
  mounted at the longest matching prefix, would put the workspace and the
  system library in one path space, which script I/O may want. Where `sys`
  goes in it is the hard part: `/sys` shadows Linux's, any absolute path may
  be real, and a relative `sys` shadows a checkout's own `sys/`. It is the same
  question as which paths a script sees. Once there is one, a mount could
  also answer the rider crate directory, by forwarding a `host_path` query to
  a host directory mounted inside the embedded tree.
- **Worldfiles as a filesystem.** A worldfile could be laid out in a `MemFs`
  and loaded like a directory. But `from_worldfile_sections` gives a rider
  with no matching package a package of its own, which loading from disk
  never does, so the two are not yet the same thing.
- **Script-visible paths.** When scripts get file I/O, is their filesystem
  the compiler's, rooted at the workspace, or a separate one the driver
  grants them? The answer decides whether one `Fs` handle is enough or the
  layer needs a notion of capability.
- **Environment variables.** The `DATALOVE_*` switches, `XDG_CACHE_HOME` and
  `HOME` are ambient inputs of the same kind as the working directory. They
  are not file I/O, but a hermetic run would need them behind the layer too.
