#!/usr/bin/env python3
"""Plant each reach fault in turn, and say what the tests catch of it.

The faults are in scripts/reach-faults.toml, one per reach rule. For each,
`measure` puts the fault in its file, runs the generative reactive tests over
a range of seeds without shrinking, with their kept sessions, and the
hand-written suites, and puts the file back. What each fault failed is printed
as a row of the planted-fault table in
botdocs/script-reactivity-architecture.md, and written to
target/faults/<id>.json.

    faults.py check                     every fault's text matches once
    faults.py measure [IDS...]          seeds 0 to 199
    faults.py measure --start 200 --count 1000 F15

The planted file is put back however the run ends, from what it held before,
so a measurement can be stopped at any point. Edit no other source while one
runs: the build it makes would carry the fault.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FAULTS = ROOT / "scripts/reach-faults.toml"
OUT = ROOT / "target/faults"
GENERATIVE = ROOT / "crates/datalove-tests/tests/generative_reactive_tests.rs"

# The hand-written suites a reach fault can fail: the datafun script suites
# and engine fixtures, the script engine's own tests and the step tables.
HAND = [
    ["cargo", "test", "-p", "datalove-datafun",
     "--test", "script_reactivity_tests", "--test", "script_scenario_tests",
     "--test", "script_splice_tests", "--test", "script_exec_reactivity_tests",
     "--test", "edit_reach_tests", "--test", "script_graph_tests", "--test", "engine_tests"],
    ["cargo", "test", "-p", "datalove-script-engine"],
    ["cargo", "test", "-p", "datalove-doc-fixtures"],
]


def load():
    with open(FAULTS, "rb") as f:
        return tomllib.load(f)["fault"]


def matches(fault):
    return (ROOT / fault["file"]).read_text().count(fault["find"])


def check(faults):
    broken = [f for f in faults if matches(f) != 1]
    for f in broken:
        print(f"{f['id']} ({f['name']}): its text appears {matches(f)} times in {f['file']}")
    if broken:
        sys.exit(1)
    print(f"all {len(faults)} faults match once")


def default_seeds():
    """The seeds `just test` runs, as the generative test's constants say."""
    text = GENERATIVE.read_text()
    start = int(re.search(r"const DEFAULT_SEED: u64 = (\d+);", text).group(1))
    count = int(re.search(r"const DEFAULT_COUNT: u64 = (\d+);", text).group(1))
    return range(start, start + count)


def run(command, env=None):
    full = dict(os.environ, **(env or {}))
    done = subprocess.run(command, cwd=ROOT, env=full, capture_output=True, text=True)
    return done.stdout + done.stderr


def generative(start, count):
    """The failing sessions, generated and dense, and the failing kept ones."""
    output = run(
        ["cargo", "test", "-q", "--release", "-p", "datalove-tests", "--test", "generative_reactive_tests"],
        {"GENERATIVE_REACTIVE_SEED": str(start), "GENERATIVE_REACTIVE_COUNT": str(count),
         "GENERATIVE_REACTIVE_NO_SHRINK": "1"},
    )
    seeds = re.search(r"failing seeds: \[(.*)\]", output)
    labels = re.findall(r'"seed (\d+)( dense)?"', seeds.group(1)) if seeds else []
    kept = re.search(r"failing kept sessions: (.*)", output)
    return (
        [int(s) for s, dense in labels if not dense],
        [int(s) for s, dense in labels if dense],
        kept.group(1).split(", ") if kept else [],
    )


def hand():
    """The hand-written tests and engine fixtures that fail, and the test
    binaries that died without saying which."""
    failed = []
    for command in HAND:
        output = re.sub(r"\x1b\[[0-9;]*m", "", run(command))
        named = re.findall(r"^test (\S+) \.\.\. FAILED", output, re.M)
        named += re.findall(r"^\s*FAIL\s+(\S+)", output, re.M)
        if not named and re.search(r"^error: test failed", output, re.M):
            named.append(f"a {command[3]} test binary, which died")
        failed += named
    return sorted(set(failed))


def measure(fault, start, count):
    path = ROOT / fault["file"]
    original = path.read_text()
    assert original.count(fault["find"]) == 1, f"{fault['id']} does not match once; run `faults.py check`"
    path.write_text(original.replace(fault["find"], fault["replace"]))
    try:
        generated, dense, kept = generative(start, count)
        failed = hand()
    finally:
        path.write_text(original)
    defaults = default_seeds()
    result = {
        "id": fault["id"], "name": fault["name"], "start": start, "count": count,
        "default": sum(s in defaults for s in generated) + sum(s in defaults for s in dense),
        "generated": generated, "dense": dense, "kept": kept, "hand": failed,
    }
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / f"{fault['id']}.json").write_text(json.dumps(result, indent=2))
    return result


def row(result):
    hand = result["hand"]
    named = ", ".join(f"`{h}`" for h in hand) if len(hand) <= 2 else f"{len(hand)} tests"
    return (f"| {result['name']} | {result['default']} | {len(result['generated'])} | "
            f"{len(result['dense'])} | {len(result['kept'])} | {named or 'none'} |")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("check")
    m = sub.add_parser("measure")
    m.add_argument("--start", type=int, default=0)
    m.add_argument("--count", type=int, default=200)
    m.add_argument("ids", nargs="*")
    args = parser.parse_args()

    faults = load()
    if args.command == "check":
        check(faults)
        return
    chosen = [f for f in faults if not args.ids or f["id"] in args.ids]
    unknown = set(args.ids) - {f["id"] for f in faults}
    if unknown:
        sys.exit(f"no such faults: {', '.join(sorted(unknown))}")
    defaults = default_seeds()
    print(f"| fault | of {2 * len(defaults)} | generated, of {args.count} | dense, of {args.count} | kept | hand-written |")
    print("|---|---|---|---|---|---|")
    for fault in chosen:
        print(row(measure(fault, args.start, args.count)), flush=True)


if __name__ == "__main__":
    main()
