"""Compare two recordings of the benchmarks, flagging what moved.

    uv run --no-project compare.py [OLD.json NEW.json]

With no arguments, the two latest in `../target/benchvs/history`. For every
benchmark and implementation both recorded, the median wall time old and new
and their ratio, and for the datalove `script` runs the running phase too,
where a change to the runtime or the JIT shows without the front end's noise.
A change past the threshold (5%) is marked, slower with `!`, faster with `+`.
Exits nonzero if anything got slower past it.
"""

import json
import sys
from pathlib import Path

HISTORY = Path(__file__).resolve().parent.parent / "target/benchvs/history"
THRESHOLD = 0.05


def load(path):
    record = json.loads(Path(path).read_text())
    return record, f"{record['commit']}{' (dirty)' if record['dirty'] else ''} {record['time']}"


def main():
    if len(sys.argv) == 3:
        old_path, new_path = sys.argv[1:]
    elif len(sys.argv) == 1:
        recorded = sorted(HISTORY.glob("*.json"))
        if len(recorded) < 2:
            sys.exit(f"fewer than two recordings in {HISTORY}")
        old_path, new_path = recorded[-2:]
    else:
        sys.exit(__doc__)
    old, old_name = load(old_path)
    new, new_name = load(new_path)
    print(f"old: {old_name}\nnew: {new_name}\n")

    slower = False
    for bench, impls in new["results"].items():
        if bench not in old["results"]:
            continue
        print(bench)
        for name, n in impls.items():
            o = old["results"][bench].get(name)
            if o is None:
                continue
            for phase in ["wall", "run"]:
                if phase not in n or phase not in o:
                    continue
                ratio = n[phase] / o[phase]
                mark = "!" if ratio > 1 + THRESHOLD else "+" if ratio < 1 - THRESHOLD else " "
                slower |= mark == "!"
                label = name if phase == "wall" else "  running"
                print(f"  {mark} {label:24} {o[phase]:8.1f} -> {n[phase]:8.1f} ms  {ratio:5.2f}x")
    sys.exit(1 if slower else 0)


if __name__ == "__main__":
    main()
