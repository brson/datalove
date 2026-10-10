"""Time every implementation of every benchmark, and keep the result.

    uv run --no-project record.py [BENCH ...]

Runs each benchmark (by default all of them) under every implementation round
after round, alternating so that a drift in the machine lands on all of them
alike, and reports medians. The datalove `script` runs are split into
compiling and running by `--time`, so that a change to the front end, the
runtime or the JIT shows up where it belongs.

Writes `../target/benchvs/history/<time>-<commit>.json`, which `compare.py`
reads. Expects `just build` to have built the CLI and the AOT executables;
`just record` does both. `PYTHON` names the Python interpreter; Julia, Java and
V8 (`v8`, its d8 shell) are the ones on `PATH`.
"""

import datetime
import json
import os
import re
import statistics
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
DATALOVE = ROOT / "target/release/datalove"
AOT_DIR = ROOT / "target/benchvs"
HISTORY = AOT_DIR / "history"

BENCHES = ["fib", "sum", "primes", "wordfreq"]
ROUNDS = 3
TIME_LINE = re.compile(r"^time: ([\d.]+) ms compiling, ([\d.]+) ms running$", re.M)


def implementations(bench):
    """Each implementation as (name, argv, whether `--time` splits it)."""
    script = [str(DATALOVE), "script", "--time"]
    yield "datalove-interp", script + [f"{bench}.dfs"], True
    yield "datalove-jit-t1", script + ["--jit", "--jit-threshold", "1", f"{bench}.dfs"], True
    yield "datalove-jit", script + ["--jit", f"{bench}.dfs"], True
    yield "datalove-aot", [str(AOT_DIR / bench)], False
    yield "python", [os.environ.get("PYTHON", "python3"), f"{bench}.py"], False
    yield "julia", ["julia", "--startup-file=no", f"{bench}.jl"], False
    yield "java", ["java", "-cp", str(AOT_DIR / "java"), bench.capitalize()], False
    yield "v8", ["v8", f"{bench}.js"], False


def run(argv):
    """Run once; the wall time in ms and stderr, failing loudly on an error."""
    start = time.perf_counter()
    proc = subprocess.run(argv, cwd=HERE, stdout=subprocess.DEVNULL,
                          stderr=subprocess.PIPE, text=True)
    wall = (time.perf_counter() - start) * 1e3
    if proc.returncode != 0:
        sys.exit(f"{' '.join(argv)} failed:\n{proc.stderr[-2000:]}")
    return wall, proc.stderr


def measure(bench):
    impls = list(implementations(bench))
    samples = {name: [] for name, _, _ in impls}
    for _, argv, _ in impls:
        run(argv)  # A round to warm the caches, not counted.
    for _ in range(ROUNDS):
        for name, argv, split in impls:
            wall, err = run(argv)
            sample = {"wall": wall}
            if split:
                phases = TIME_LINE.search(err)
                sample["compile"] = float(phases[1])
                sample["run"] = float(phases[2])
            samples[name].append(sample)
    return {
        name: {key: statistics.median(s[key] for s in samples[name]) for key in samples[name][0]}
        | {"samples": samples[name]}
        for name, _, _ in impls
    }


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True,
                          check=True).stdout.strip()


def main():
    benches = sys.argv[1:] or BENCHES
    commit = git("rev-parse", "--short", "HEAD")
    dirty = bool(git("status", "--porcelain"))
    results = {}
    for bench in benches:
        print(f"measuring {bench}", file=sys.stderr)
        results[bench] = measure(bench)

    for bench, impls in results.items():
        print(f"\n{bench}")
        for name, r in impls.items():
            split = f"  ({r['compile']:.0f} compiling, {r['run']:.0f} running)" if "run" in r else ""
            print(f"  {name:16} {r['wall']:8.1f} ms{split}")

    now = datetime.datetime.now(datetime.timezone.utc)
    record = {
        "commit": commit,
        "dirty": dirty,
        "time": now.isoformat(timespec="seconds"),
        "load": os.getloadavg()[0],
        "rounds": ROUNDS,
        "results": results,
    }
    HISTORY.mkdir(parents=True, exist_ok=True)
    path = HISTORY / f"{now:%Y%m%dT%H%M%S}-{commit}{'-dirty' if dirty else ''}.json"
    path.write_text(json.dumps(record, indent=1))
    print(f"\nwrote {path.relative_to(ROOT)}", file=sys.stderr)


if __name__ == "__main__":
    main()
