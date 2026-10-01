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

Run through `just publish-drip`. It is safe to interrupt and safe to re-run:
state lives in the registry, not here.
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
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        capture_output=True, text=True, check=True,
    ).stdout
    # An empty `publish` list is `publish = false`; those never go up.
    return [
        (p["name"], p["version"])
        for p in json.loads(out)["packages"]
        if p.get("publish") != []
    ]


def retry_delay(output):
    """Seconds until crates.io says the limit resets, from its 429 text."""
    m = re.search(r"Please try again after ([^\n]+?) and see", output)
    if not m:
        return DEFAULT_SLEEP
    when = email.utils.parsedate_to_datetime(m.group(1).strip())
    import datetime
    delay = (when - datetime.datetime.now(datetime.timezone.utc)).total_seconds()
    return max(int(delay) + 10, 10)


def main():
    while True:
        crates = workspace_crates()
        done = [n for n, v in crates if v in published_versions(n)]
        remaining = [n for n, v in crates if n not in done]
        if not remaining:
            print(f"all {len(crates)} crates are up to date on crates.io")
            return 0

        print(f"{len(remaining)} to publish, {len(done)} already up: {' '.join(remaining)}")
        cmd = ["cargo", "publish", "--workspace"]
        for name in done:
            cmd += ["--exclude", name]

        proc = subprocess.run(cmd, capture_output=True, text=True)
        sys.stderr.write(proc.stderr)
        if proc.returncode == 0:
            print("published the rest")
            return 0

        combined = proc.stdout + proc.stderr
        if "429 Too Many Requests" not in combined:
            sys.stderr.write("\npublish failed for a reason that is not the rate limit\n")
            return proc.returncode

        delay = retry_delay(combined)
        print(f"\nrate limited; sleeping {delay}s then resuming", flush=True)
        time.sleep(delay)


if __name__ == "__main__":
    sys.exit(main())
