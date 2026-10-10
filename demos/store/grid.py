"""Time the report under each engine, at several sizes of data.

    uv run --no-project grid.py [ORDERS ...]

For each order count (by default 2,000, 20,000 and 200,000): generates the
data, builds the AOT executable, then runs every engine round after round,
alternating so that a drift in the machine lands on all of them alike, and
reports medians. `script` runs are split into compiling and running by
`--time`. One more run per JIT configuration with `--jit-stats` gives what
was compiled and how often calls crossed into it, kept out of the timed runs
since counting calls costs time.

Prints a markdown table per size, and writes everything to
`target/store-grid/grid.json`. Leaves the data at 20,000 orders, as `just gen`
does by default.
"""

import json
import re
import statistics
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
DATALOVE = ROOT / "target/release/datalove"
OUT = ROOT / "target/store-grid"

# Each JIT configuration: its name and the flags beside `--jit`. Compiling
# every function on its first call, as `--jit` alone does, then tiering at a
# call threshold of 100 -- which no longer matters much, since loops are
# entered by OSR -- across OSR thresholds and Cranelift's opt levels.
JIT_CONFIGS = [("jit t=1", ["--jit-threshold", "1"])] + [
    (f"t=100 osr={osr} {opt}",
     ["--jit-threshold", "100", "--jit-osr-threshold", str(osr), "--jit-opt-level", opt])
    for opt in ["speed", "none"]
    for osr in [100, 1000, 10000]
]
TIME_LINE = re.compile(r"^time: ([\d.]+) ms compiling, ([\d.]+) ms running$", re.M)
COMPILED_LINE = re.compile(
    r"^jit: compiled (\d+) functions \((\d+) refused\), ([\d.]+) ms codegen, (\d+) bytes$", re.M)
CALLS_LINE = re.compile(
    r"^jit: calls through the dispatcher: (\d+) interpreted, (\d+) interpreter -> native "
    r"\((\d+) planned\), (\d+) native -> interpreter$", re.M)


def rounds_for(orders):
    """Fewer rounds for the big sizes, which are slow and steadier."""
    return 7 if orders <= 20000 else 3


def engines(orders):
    """Each engine as (name, argv, whether it is a `script` run)."""
    script = [str(DATALOVE), "script", "--time"]
    yield "interp", script + ["report.dfs"], True
    for name, flags in JIT_CONFIGS:
        yield name, script + ["--jit"] + flags + ["report.dfs"], True
    yield "aot", [str(OUT / f"report-{orders}")], False


def run(argv):
    """Run once; the wall time in ms and stderr, failing loudly on an error."""
    start = time.perf_counter()
    proc = subprocess.run(argv, cwd=HERE, stdout=subprocess.DEVNULL,
                          stderr=subprocess.PIPE, text=True)
    wall = (time.perf_counter() - start) * 1e3
    if proc.returncode != 0:
        sys.exit(f"{' '.join(argv)} failed:\n{proc.stderr[-2000:]}")
    return wall, proc.stderr


def jit_stats(flags):
    """What one run with `flags` compiled, and the calls through the dispatcher."""
    _, err = run([str(DATALOVE), "script", "--jit"] + flags + ["--jit-stats", "report.dfs"])
    compiled = COMPILED_LINE.search(err)
    calls = CALLS_LINE.search(err)
    return {
        "compiled": int(compiled[1]),
        "refused": int(compiled[2]),
        "codegen_ms": float(compiled[3]),
        "code_bytes": int(compiled[4]),
        "interpreted": int(calls[1]),
        "interp_to_jit": int(calls[2]),
        "planned": int(calls[3]),
        "jit_to_interp": int(calls[4]),
    }


def measure(orders):
    subprocess.run(["uv", "run", "--no-project", "gen.py", str(orders)], cwd=HERE,
                   check=True, stdout=subprocess.DEVNULL)
    subprocess.run([str(DATALOVE), "aot-compile", "--link", "-o", str(OUT / f"report-{orders}"),
                    "report.dfs"], cwd=HERE, check=True, stdout=subprocess.DEVNULL)

    configs = list(engines(orders))
    # A round, not counted, that warms the page cache and finds what fails. An
    # engine that fails is reported as failing rather than timed, so that one
    # broken backend does not cost the rest of the grid.
    failed = {}
    for name, argv, _ in configs:
        proc = subprocess.run(argv, cwd=HERE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if proc.returncode != 0:
            failed[name] = proc.returncode
    configs = [c for c in configs if c[0] not in failed]
    samples = {name: [] for name, _, _ in configs}
    for _ in range(rounds_for(orders)):
        for name, argv, is_script in configs:
            wall, err = run(argv)
            sample = {"wall": wall}
            if is_script:
                phases = TIME_LINE.search(err)
                sample["compile"] = float(phases[1])
                sample["run"] = float(phases[2])
            samples[name].append(sample)

    results = {}
    for name, _, _ in configs:
        result = {key: statistics.median(s[key] for s in samples[name])
                  for key in samples[name][0]}
        result["samples"] = samples[name]
        results[name] = result
    for name, flags in JIT_CONFIGS:
        results[name]["stats"] = jit_stats(flags)
    for name, code in failed.items():
        results[name] = {"failed": code}
    return results


def table(orders, results):
    interp = results["interp"]["wall"]
    lines = [
        f"### {orders:,} orders",
        "",
        "| Engine | Wall | Compile | Run | vs interp | Compiled | Codegen | Interp->JIT calls |",
        "|---|---|---|---|---|---|---|---|",
    ]
    for name, r in results.items():
        if "failed" in r:
            lines.append(f"| {name} | failed, exit {r['failed']} | | | | | | |")
            continue
        compile_ms = f"{r['compile']:.0f}" if "compile" in r else "-"
        run_ms = f"{r['run']:.0f}" if "run" in r else "-"
        s = r.get("stats")
        compiled = str(s["compiled"]) if s else "-"
        codegen = f"{s['codegen_ms']:.0f}" if s else "-"
        crossings = f"{s['interp_to_jit']:,}" if s else "-"
        lines.append(f"| {name} | {r['wall']:.0f} | {compile_ms} | {run_ms} | "
                     f"{interp / r['wall']:.2f}x | {compiled} | {codegen} | {crossings} |")
    return "\n".join(lines)


def main():
    sizes = [int(a) for a in sys.argv[1:]] or [2000, 20000, 200000]
    OUT.mkdir(parents=True, exist_ok=True)
    subprocess.run(["cargo", "build", "-p", "datalove-cli", "--release"], cwd=ROOT, check=True)
    everything = {}
    try:
        for orders in sizes:
            print(f"measuring {orders} orders", file=sys.stderr)
            everything[orders] = measure(orders)
            print(table(orders, everything[orders]) + "\n", flush=True)
    finally:
        subprocess.run(["uv", "run", "--no-project", "gen.py", "20000"], cwd=HERE,
                       check=True, stdout=subprocess.DEVNULL)
    (OUT / "grid.json").write_text(json.dumps(everything, indent=1))


if __name__ == "__main__":
    main()
