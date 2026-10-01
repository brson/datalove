"""Publish the workspace a crate at a time, waiting out the rate limit.

crates.io allows a burst of five new crate names and then refills one slot
every ten minutes, so a first publish of this workspace stops after five and
takes a 429. Cargo has nothing to pace itself with: there is no delay option,
and the only 429 handling it has is the generic network retry, which caps a
sleep at ten seconds, defaults to three attempts, and is not wrapped around
the upload anyway. So the waiting happens out here.

Cargo also checks every crate in the set against the index before it uploads
anything and fails the run on the first one already up, which means a resumed
publish has to name all the finished crates in `--exclude`. This reads the
index and writes that list itself, so a run picks up wherever the last one
stopped with no bookkeeping in the justfile.

Cargo packages and build-verifies every crate in the set before it uploads
any of them, so a loop that left verification on would rebuild the whole
workspace once per crate it managed to land. Verification happens here once
up front, against everything, and the publishes that follow pass
`--no-verify`. The tarballs cargo builds from are the same either way.

Run through `just publish-drip`. It is safe to interrupt and safe to re-run:
state lives in the registry, not here. Re-running repeats the one
verification, which is cheap after the first since the build is cached.
"""
import email.utils, json, re, subprocess, sys, time, urllib.error, urllib.request

# How long to wait when crates.io does not say. One refill plus slack.
DEFAULT_SLEEP = 610


def index_path(name):
    """The sparse index lays crates out by the length and head of the name."""
    n = len(name)
    if n <= 2:
        return f"{n}/{name}"
    if n == 3:
        return f"3/{name[0]}/{name}"
    return f"{name[:2]}/{name[2:4]}/{name}"


def published_versions(name):
    url = f"https://index.crates.io/{index_path(name)}"
    try:
        with urllib.request.urlopen(url) as resp:
            body = resp.read().decode()
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return set()
        raise
    return {json.loads(line)["vers"] for line in body.splitlines() if line.strip()}


def workspace_crates():
    """The crates that go to the registry, and the names of those that do not.

    An empty `publish` list is `publish = false`. `cargo publish --workspace`
    drops those itself, but `cargo package --workspace` does not: it tries to
    package them and dies on `datalove-rt-tests`, whose dependency on
    `datalove-exampletest` is by path with no version because neither is ever
    published. So the verification pass has to name them to skip them.
    """
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        capture_output=True, text=True, check=True,
    ).stdout
    packages = json.loads(out)["packages"]
    publishable = [(p["name"], p["version"]) for p in packages if p.get("publish") != []]
    unpublishable = [p["name"] for p in packages if p.get("publish") == []]
    return publishable, unpublishable


def retry_delay(output):
    """Seconds until crates.io says the limit resets, from its 429 text."""
    m = re.search(r"Please try again after ([^\n]+?) and see", output)
    if not m:
        return DEFAULT_SLEEP
    when = email.utils.parsedate_to_datetime(m.group(1).strip())
    import datetime
    delay = (when - datetime.datetime.now(datetime.timezone.utc)).total_seconds()
    return max(int(delay) + 10, 10)


def run_cargo(*args):
    """Run cargo, echoing it live, and keep a copy of what it said.

    Verifying thirty-odd crates takes long enough that a silent wait is
    indistinguishable from a hang, so the output goes straight through rather
    than being held until the run ends. The copy is only here because the 429
    and the time it resets at have to be read back out afterwards. Cargo puts
    its status on stderr, so the two streams merge to keep them in order.
    """
    cmd = ["cargo", *args]
    # Piping turns cargo's colors off, and this is meant to be watched.
    if sys.stdout.isatty():
        cmd += ["--color", "always"]
    proc = subprocess.Popen(
        cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1,
    )
    seen = []
    for line in proc.stdout:
        sys.stdout.write(line)
        sys.stdout.flush()
        seen.append(line)
    return proc.wait(), "".join(seen)


def main():
    print("verifying the workspace once; the publishes after this skip it")
    verify = ["package", "--workspace"]
    for name in workspace_crates()[1]:
        verify += ["--exclude", name]
    code, _ = run_cargo(*verify)
    if code != 0:
        sys.stderr.write("\nverification failed, so nothing was uploaded\n")
        return code

    while True:
        crates, _ = workspace_crates()
        done = [n for n, v in crates if v in published_versions(n)]
        remaining = [n for n, v in crates if n not in done]
        if not remaining:
            print(f"all {len(crates)} crates are up to date on crates.io")
            return 0

        print(f"{len(remaining)} to publish, {len(done)} already up: {' '.join(remaining)}")
        args = ["publish", "--workspace", "--no-verify"]
        for name in done:
            args += ["--exclude", name]

        code, combined = run_cargo(*args)
        if code == 0:
            print("published the rest")
            return 0

        if "429 Too Many Requests" not in combined:
            sys.stderr.write("\npublish failed for a reason that is not the rate limit\n")
            return code

        delay = retry_delay(combined)
        print(f"\nrate limited; sleeping {delay}s then resuming", flush=True)
        time.sleep(delay)


if __name__ == "__main__":
    sys.exit(main())
