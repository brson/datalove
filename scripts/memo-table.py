#!/usr/bin/env python3
"""Generate markdown tables from module memoization test results."""

import json
import sys
from pathlib import Path

FIXTURES_DIR = Path(__file__).parent.parent / "crates/datalove-datafun/tests/fixtures/module_memo"

def yn(val):
    """Convert bool to y/n."""
    return "y" if val else "n"

def load_test_results():
    """Load all .out.expected files and extract final step results."""
    results = []
    for f in sorted(FIXTURES_DIR.glob("*.out.expected")):
        name = f.stem.replace(".out", "")
        with open(f) as fp:
            data = json.load(fp)
        if not data.get("steps"):
            continue
        last_step = data["steps"][-1]
        action = last_step.get("action", "unknown")
        for mod_path, mod_result in last_step.get("results", {}).items():
            results.append({
                "test": name,
                "action": action,
                "module": mod_path,
                **mod_result
            })
    return results

def print_table(title, rows, columns):
    """Print a markdown table with equal-width columns."""
    print(f"\n**{title}**\n")

    # Convert all cells to strings first.
    str_rows = []
    for row in rows:
        cells = []
        for col in columns:
            val = row.get(col, "")
            if col == "correct" and val == False:
                cells.append("**n**")
            elif isinstance(val, bool):
                cells.append(yn(val))
            else:
                cells.append(str(val))
        str_rows.append(cells)

    # Calculate max width for each column.
    widths = [len(col) for col in columns]
    for cells in str_rows:
        for i, cell in enumerate(cells):
            widths[i] = max(widths[i], len(cell))

    # Print header.
    header = "| " + " | ".join(col.ljust(widths[i]) for i, col in enumerate(columns)) + " |"
    separator = "| " + " | ".join("-" * w for w in widths) + " |"
    print(header)
    print(separator)

    # Print rows.
    for cells in str_rows:
        line = "| " + " | ".join(cell.ljust(widths[i]) for i, cell in enumerate(cells)) + " |"
        print(line)

def main():
    results = load_test_results()

    # Separate direct and dependent results from 1xx and 2xx tests.
    direct_results = []
    dependent_results = []

    for r in results:
        test = r["test"]
        # expected can be None for cases where memoization doesn't apply
        exp = r.get("expected") or {}
        if test.startswith("1") and r.get("is_direct"):
            direct_results.append({
                "test": test[:3],
                "action": r["action"],
                "parsed": r.get("parsed"),
                "typechecked": r.get("typechecked"),
                "hash_changed": r.get("hash_changed"),
                "exp_p": exp.get("parsed") if exp else "-",
                "exp_t": exp.get("typechecked") if exp else "-",
                "exp_h": exp.get("hash_changed") if exp else "-",
                "correct": r.get("correct"),
            })
        elif test.startswith("2") and r.get("is_dependent"):
            dependent_results.append({
                "test": test[:3],
                "action": r["action"],
                "parsed": r.get("parsed"),
                "typechecked": r.get("typechecked"),
                "hash_changed": r.get("hash_changed"),
                "exp_p": exp.get("parsed") if exp else "-",
                "exp_t": exp.get("typechecked") if exp else "-",
                "exp_h": exp.get("hash_changed") if exp else "-",
                "correct": r.get("correct"),
            })

    columns = ["test", "action", "parsed", "typechecked", "hash_changed", "exp_p", "exp_t", "exp_h", "correct"]

    print_table("Direct Module Results (1xx tests)", direct_results, columns)
    print_table("Dependent Module Results (2xx tests)", dependent_results, columns)

    # Summary
    all_results = direct_results + dependent_results
    total = len(all_results)
    correct = sum(1 for r in all_results if r.get("correct"))
    print(f"\n**Summary:** {correct}/{total} correct")

    if correct < total:
        sys.exit(1)

if __name__ == "__main__":
    main()
