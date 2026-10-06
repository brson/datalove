//! A function a script unit defines, edited and run again, runs as edited.
//!
//! Re-executing a unit replaces its functions' bodies under the same identity
//! -- the same unit, the same function id -- and a body is a new allocation
//! that may land where a body it replaced was freed. The interpreter keeps a
//! frame layout per function identity, and the bytecode keeps its lowering of
//! a body, and each call site its callee, by the body's address; none of
//! those may survive the body they were made for, or an edit runs the code it
//! replaced. Run under `DATALOVE_INTERP=bc` by `just test-bc`, which is where
//! the addresses matter.

use rmx::prelude::*;

mod scriptsession;
use scriptsession::Session;

/// Edit a function's body over and over, and read what calling it gives.
#[test]
fn an_edited_function_runs_as_edited() {
    let mut session = Session::new();
    session.append("fun f(): u32\n  ret 1\nend fun\n");
    session.append("let r = f()\n");
    assert_eq!(session.binding("r"), "1");
    for k in 2..=8 {
        session.edit(0, &format!("fun f(): u32\n  ret {k}\nend fun\n"));
        session.rederive(0);
        assert_eq!(session.binding("r"), k.to_string(), "after editing `f` to return {k}");
    }
}

/// The same, through a second function, so that a call site in a body the
/// edit did not touch is the one that has to notice.
#[test]
fn a_caller_of_an_edited_function_calls_the_edited_body() {
    let mut session = Session::new();
    session.append("fun f(): u32\n  ret 1\nend fun\nfun g(): u32\n  ret f()\nend fun\n");
    session.append("let r = g()\n");
    assert_eq!(session.binding("r"), "1");
    for k in 2..=8 {
        session.edit(0, &format!("fun f(): u32\n  ret {k}\nend fun\nfun g(): u32\n  ret f()\nend fun\n"));
        session.rederive(0);
        assert_eq!(session.binding("r"), k.to_string(), "after editing `f` to return {k}");
    }
}
