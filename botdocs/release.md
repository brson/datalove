# Releasing

Two things ship from this workspace on two schedules: `bcts`, which has been
published since before the rest existed and moves on its own, and the
`datalove-*` crates, which all sit at one version and go together.

## Releasing bcts

`bcts` is the compiler infrastructure crate, versioned independently of the
workspace. Its version appears in three files and all three move together:

- `crates/bcts/Cargo.toml`, the `version` field.
- `Cargo.toml`, the `bct.version` workspace dependency, and the comment above
  it that names the current version.
- `Cargo.lock`, which a `cargo check --workspace` rewrites.

Then commit as `Release bcts X.Y.Z`, tag the commit `bcts-X.Y.Z` as a
lightweight tag, and publish.

### Choosing the number

`bcts` is pre-1.0, so cargo reads the minor as the breaking position. The
history follows that:

- A change to the shape of anything public takes the minor. 0.7.0 did,
  because `TreeToken::Branch`'s `inner` became a `Box<BracerIter>`.
- Additions alone take the patch. 0.6.2 did, adding four public items and
  changing nothing that existed.
- Behaviour changes under unchanged signatures take the patch. 0.6.1 and
  0.6.3 did.

A new method on a public trait is an addition when it carries a default body
and a breaking change when it does not, since without one every implementor
outside the workspace stops compiling.

The commit message says how many commits went in, what changed, and why that
picked the position it picked. The existing ones are the model.

## Publishing the workspace

Four recipes in the justfile, each with a comment saying when it is the right
one:

- `just publish-check` is `cargo publish --dry-run --workspace --no-verify`.
  Manifests only. Use it rather than `cargo package --workspace`, which does
  not honour `publish = false` and so reports failures for the test and bench
  crates that never publish.
- `just local-registry` tries the release path without a release. See below.
- `just publish` is `cargo publish --workspace`, in one shot. Right only when
  every crate is already on crates.io and this is a new version of each.
- `just publish-drip` runs `scripts/publish-drip.py`, which publishes a crate
  at a time and waits out the rate limit. Right for a first publish and for
  resuming a run that stopped part way.

Two properties of `cargo publish --workspace` are why there are two.

**Cargo errors rather than skips.** Before it uploads anything it queries the
index for every crate in the set, and if any of them is already there at the
version in its manifest it fails the whole run. There is a `--dry-run` that
downgrades the same condition to a warning, so a dry run will tell you a crate
is already up and then cheerfully go on listing it as an upload; that is the
dry run being lenient, not cargo planning to skip it. Anything already
published therefore has to be named in `--exclude`, and leaving one out means
nothing publishes at all.

**crates.io rate limits a new crate name hard.** The registry allows a burst
of five new names and then refills one slot every ten minutes; a new version
of a name that already exists is a separate and much looser bucket, thirty
burst and one a minute. Publishing a workspace of thirty-odd crates for the
first time therefore gets five through and then takes a 429, and the rest come
at six an hour. The two buckets are why a `bcts` version bump rides along
freely while its neighbours queue.

`publish-drip` handles both. It reads the index to see what is already up,
excludes that, publishes what is left, and sleeps when the registry says to.
It verifies the whole workspace once at the start and passes `--no-verify`
thereafter, so the waiting is not spent rebuilding. State lives in the
registry, so it is safe to interrupt and re-run.

If the whole set is still ahead of you, it is worth writing to
help@crates.io first and asking for the new-crate limit to be lifted for the
account; they do grant it, and it turns five hours of dripping into one run.

### Trying the release path first

`just local-registry` vendors the third-party crates, packages ours, and puts
both in `target/local-registry`. Point a `CARGO_HOME` at a `config.toml` that
replaces `crates-io` with that directory and
`cargo install --offline --locked datalove-cli --version X.Y.Z` builds what a
user would get: `BuildInfo` saying `Prod`, `sys/` out of a crate, native
components naming dependencies by version. `CARGO_HOME` rather than a project
`.cargo/config.toml` because the compiler shells out to cargo again to build a
native component, in a work dir with no project above it, and that invocation
needs the replacement too. `scripts/local-registry.py` has the details.

### Dev-dependency cycles

A dev-dependency cycle is unpublishable, since neither end can go first, and
cargo accepts one silently: it builds and tests happily, and only publishing
complains. Three turned up on the first release attempt, all into the
pipeline from crates the pipeline is built on: `datalove-datafun-compiler`
and `datalove-datafun-cranelift-aot` each had tests using `datalove-datafun`,
and `datalove-datafun-cranelift-jit` declared the same dev-dependency without
using it.

The rule that came out of it: a test that needs the whole pipeline to
exercise one crate cannot live in that crate. It goes in `datalove-tests`,
which exists for suites that span the workspace and is `publish = false`.
