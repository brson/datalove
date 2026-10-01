"""Add this workspace's packaged crates to a vendor directory.

A directory source stands in for crates.io entirely, so the production code
path can be exercised against crates that were never published: `BuildInfo`
saying `Prod`, the component naming dependencies by version, `sys/` arriving
out of a crate rather than off disk. None of that runs from a checkout, and
none of it was tested before a release until this existed.

Run through `just local-registry`, which packages first. Then:

    cargo install --offline --locked --root <somewhere> \
        datalove-cli --version 0.1.0

with a `CARGO_HOME` whose `config.toml` replaces `crates-io` with this
directory. `CARGO_HOME` rather than `.cargo/config.toml` because the compiler
shells out to cargo again to build a native component, in a work dir with no
project above it, and that invocation has to see the replacement too.
"""
import hashlib, json, pathlib, subprocess, sys, tarfile

PACKED = pathlib.Path("target/package/tmp-crate")
VENDOR = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "target/local-registry")

# Not published, so not in the registry either.
SKIP = {"datalove-bench", "datalove-exampletest", "datalove-rt-tests", "datalove-tests"}

added = 0
for crate in sorted(PACKED.glob("*.crate")):
    stem = crate.name[: -len(".crate")]
    name = stem.rsplit("-", 1)[0]
    if name in SKIP:
        continue

    dest = VENDOR / stem
    if dest.exists():
        subprocess.run(["rm", "-rf", str(dest)], check=True)

    with tarfile.open(crate) as tar:
        tar.extractall(VENDOR, filter="data")

    files = {}
    for path in sorted(dest.rglob("*")):
        if path.is_file() and path.name != ".cargo-checksum.json":
            rel = path.relative_to(dest).as_posix()
            files[rel] = hashlib.sha256(path.read_bytes()).hexdigest()

    (dest / ".cargo-checksum.json").write_text(json.dumps({
        "files": files,
        "package": hashlib.sha256(crate.read_bytes()).hexdigest(),
    }))
    added += 1

print(f"added {added} crates to {VENDOR}")
