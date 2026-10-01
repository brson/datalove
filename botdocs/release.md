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

`just publish` runs `cargo publish --workspace`. Two properties of that
command govern everything awkward about it.

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

So the first publish of this workspace is not one command. It is that command
run repeatedly, each time with the crates that made it last time added to the
exclusions, until the exclusion list covers the workspace. The exclusions in
the `publish` recipe are a record of where the last run stopped.

If the whole set is still ahead of you, it is worth writing to
help@crates.io first and asking for the new-crate limit to be lifted for the
account; they do grant it, and it turns five hours of dripping into one run.
