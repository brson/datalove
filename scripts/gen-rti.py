#!/usr/bin/env python3
"""Generate the runtime interface from the runtime's definitions.

`crates/datalove-rt/src/c.rs` defines every `dtlv_rti_*` function the ABI
exports. Three files are generated from it:

- `crates/datalove-rti/src/table.rs`, the table of function pointers a rider
  reaches the runtime through.
- `crates/datalove-rti/src/call.rs`, one wrapper per function that finds the
  table on the handle and calls through it, so rider code reads as a call.
- `crates/datalove-rt/src/abi_table.rs`, the table itself, filled with this
  runtime's functions. Building it is what checks the two agree: a field whose
  type does not match its function is a compile error, with no assertions
  needed.

Run from the repository root:

    just gen-rti
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SOURCE = ROOT / "crates" / "datalove-rt" / "src" / "c.rs"
TABLE = ROOT / "crates" / "datalove-rti" / "src" / "table.rs"
CALL = ROOT / "crates" / "datalove-rti" / "src" / "call.rs"
FILLED = ROOT / "crates" / "datalove-rt" / "src" / "abi_table.rs"

MARKER = "#[unsafe(no_mangle)]"
SIGNATURE = re.compile(r"pub\s+(unsafe\s+)?extern\s+\"C-unwind\"\s+fn\s+(\w+)\s*\(")


def signatures(source):
    """Every exported function, as (name, is_unsafe, params, return type)."""
    lines = source.splitlines()
    for index, line in enumerate(lines):
        if line.strip() != MARKER:
            continue

        # The signature runs from the `fn` to the brace that opens its body,
        # over however many lines the parameters take.
        text = "\n".join(lines[index + 1:])
        match = SIGNATURE.search(text)
        if not match or match.start() > text.find("\n") + 1:
            sys.exit(f"{SOURCE}:{index + 1}: no signature follows {MARKER}")

        depth = 1
        position = match.end()
        while depth:
            if text[position] == "(":
                depth += 1
            elif text[position] == ")":
                depth -= 1
            position += 1

        params = text[match.end():position - 1]
        rest = text[position:text.index("{", position)].strip()
        returns = rest[2:].strip() if rest.startswith("->") else None

        yield match.group(2), bool(match.group(1)), parameters(params), returns


def parameters(params):
    """The parameter list as (name, type) pairs.

    A comment explaining an argument is written for whoever reads the
    definition and is dropped here. A leading underscore says a definition
    ignores an argument, which is no business of an interface.
    """
    flat = " ".join(
        part for part in
        (re.sub(r"//.*$", "", line).strip() for line in params.split("\n"))
        if part
    )

    pairs = []
    depth = 0
    current = ""
    for char in flat + ",":
        if char in "(<[":
            depth += 1
        elif char in ")>]":
            depth -= 1
        if char == "," and depth == 0:
            if current.strip():
                name, _, ty = current.partition(":")
                pairs.append((name.strip().lstrip("_"), re.sub(r"\s+", " ", ty.strip())))
            current = ""
        else:
            current += char
    return pairs


def pointer_type(is_unsafe, params, returns):
    """The function pointer type a table field holds."""
    args = ", ".join(ty for _, ty in params)
    arrow = f" -> {returns}" if returns else ""
    prefix = "unsafe " if is_unsafe else ""
    return f'{prefix}extern "C-unwind" fn({args}){arrow}'


def main():
    source = SOURCE.read_text()
    found = list(signatures(source))
    exported = source.count(MARKER)
    if len(found) != exported:
        sys.exit(f"parsed {len(found)} signatures but {exported} are exported")

    fields = "\n".join(
        f"    pub {name}: {pointer_type(is_unsafe, params, returns)},"
        for name, is_unsafe, params, returns in found
    )
    TABLE.write_text(f'''//! The table of runtime functions a rider calls.
//!
//! Generated from `datalove-rt`'s `c.rs` by `scripts/gen-rti.py`. Do not edit.
//!
//! A rider does not link the runtime, so it cannot call these by name. It
//! gets them from the handle it is passed, which carries a pointer to one of
//! these as its first word. [`call`](crate::call) is the readable way to do
//! that; this is the shape being pointed at.
//!
//! The field order is the ABI. A runtime and a rider built from different
//! revisions of this file would disagree about which function is which, so
//! `EXPORTED` is checked where the table is filled in.

use datalove_rtdt as rtdt;

use crate::{{DebugOutputMode, LocalRtHandle, RtEq, RtOrdering, RtStatus}};

/// How many functions the runtime exports.
pub const EXPORTED: usize = {len(found)};

/// Every function the runtime exports, by pointer.
#[repr(C)]
pub struct RtiTable {{
{fields}
}}
''')

    # A wrapper needs a handle to find the table on, so the few functions that
    # take no handle have none. They ask nothing of the runtime's state --
    # they read a descriptor or unpack a value -- and a caller that has a
    # handle anyway can reach them through the table directly.
    wrappers = []
    stateless = []
    for name, is_unsafe, params, returns in found:
        if not params or params[0] != ("rt", "LocalRtHandle"):
            stateless.append(name)
            continue
        args = ", ".join(f"{argname}: {ty}" for argname, ty in params)
        forward = ", ".join(argname for argname, _ in params)
        arrow = f" -> {returns}" if returns else ""
        wrappers.append(
            f"/// Calls [`RtiTable::{name}`](crate::table::RtiTable::{name}).\n"
            f"///\n"
            f"/// # Safety\n"
            f"///\n"
            f"/// `rt` must be a handle the runtime gave out, and the remaining\n"
            f"/// arguments must be what that function requires.\n"
            f"#[inline]\n"
            f"pub unsafe fn {name}({args}){arrow} {{\n"
            f"    unsafe {{ (crate::table(rt).{name})({forward}) }}\n"
            f"}}"
        )

    listed = "\n".join(f"//! - `{name}`" for name in stateless)
    CALL.write_text(f'''//! Calling the runtime through the table on the handle.
//!
//! Generated from `datalove-rt`'s `c.rs` by `scripts/gen-rti.py`. Do not edit.
//!
//! Each of these finds the table on the handle it is given and calls through
//! it, so a rider writes `call::dtlv_rti_string_from_bytes(rt, ..)` and does
//! not have to hold the indirection in mind. The handle is passed on as well
//! as being read, the runtime needing its own state to do the work.
//!
//! These functions take no handle, so there is nothing here to find a table
//! on. They ask nothing of the runtime's state, reading a descriptor or
//! unpacking a value, and a caller holding a handle for other reasons can
//! reach them through [`table`](crate::table) directly:
//!
{listed}

use datalove_rtdt as rtdt;

use crate::{{DebugOutputMode, LocalRtHandle, RtEq, RtOrdering, RtStatus}};

{chr(10).join(chr(10).join([w, ""]) for w in wrappers).rstrip()}
''')

    filled = "\n".join(
        f"    {name}: c::{name}," for name, _, _, _ in found
    )
    FILLED.write_text(f'''//! This runtime's functions, as the table a rider is given.
//!
//! Generated by `scripts/gen-rti.py`. Do not edit.
//!
//! A rider is built against `datalove-rti` and run against these definitions,
//! so the two disagreeing is a rider calling a function whose arguments are
//! not what it passed. Filling the table in is what rules that out: a field
//! whose type does not match the function assigned to it does not compile, so
//! there is nothing to assert separately.
//!
//! [`RtLocal`](crate::impls::rt_local::RtLocal) holds a reference to this as
//! its first field, which is how a rider reaches it from a handle.

use datalove_rti::table::RtiTable;

use crate::c;

/// The table every handle this runtime gives out points to.
pub static TABLE: RtiTable = RtiTable {{
{filled}
}};

/// The declarations must cover every export, not merely agree with the ones
/// they name. A function added to `c.rs` without regenerating leaves the
/// counts apart.
const _: () = assert!(
    datalove_rti::table::EXPORTED == {len(found)},
    "datalove-rti describes a different number of functions than datalove-rt exports",
);
''')

    print(f"wrote {len(found)} fields to {TABLE.relative_to(ROOT)}")
    print(f"wrote {len(wrappers)} wrappers to {CALL.relative_to(ROOT)}"
          f" ({len(stateless)} take no handle)")
    print(f"wrote {len(found)} entries to {FILLED.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
