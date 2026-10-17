//! Compile the system library and run a script on the bytecode interpreter,
//! on whatever thread this is called on.
//!
//! Built for `wasm32-unknown-unknown`, which has no threads, and run under
//! wasmi by `tests/wasm_smoke.rs`. Rayon's pool falls back to the calling
//! thread there, so `ParallelMode::Parallel` takes every parallel path the
//! compiler has on one thread. That is the test: the parallel compile may be
//! the default on native targets only if it still works where there is no
//! second thread to run it on.

use rmx::prelude::*;
use std::sync::Arc;

use datalove_datafun as datafun;
use datafun::pipeline::WorkspaceDescriptor;
use datafun::pipeline::rider_load::{RiderNatives, register_natives};
use datalove_datafun_interp::Engine;
pub use datalove_datafun_common::ParallelMode;

/// What the script runs, one unit at a time; an expression's value is reported.
const UNITS: &[(&str, bool)] = &[
    ("require module sys/std/list", false),
    ("require module sys/std/u8", false),
    ("fun fib(n: int): int\n  if n .< 2\n    ret n\n  end if\n  ret fib(n - 1) + fib(n - 2)\nend fun", false),
    ("fib(20)", true),
    ("var xs: [u32] = [1]", false),
    ("call list.push(mut xs, 2)", false),
    ("xs", true),
    ("u8.max_value()", true),
];

/// Compile the system library in `mode`, run [`UNITS`] on the bytecode, and
/// report each expression's value, one per line.
pub fn run(mode: ParallelMode) -> AnyResult<String> {
    let sys = datalove_sys_packages::system_library();
    let workspace = WorkspaceDescriptor::from_system_library(&sys);
    let db = datafun::Database::default();
    let mut pipeline = workspace.to_pipeline(&db);
    let natives = Arc::new(RiderNatives::for_workspace(&workspace, &sys.natives));
    pipeline.set_natives(natives.clone());

    let compiled = pipeline.compile_fresh_with_mode(&db, mode);
    if compiled.has_errors() {
        bail!("the system library did not compile: {}", compiled.all_errors().join("; "));
    }
    let mut compiler = compiled.script_compiler_default(&db)
        .expect("the modules compiled");
    let mut executor = compiled.script_executor(datafun::DebugOutputMode::Disabled, None)
        .expect("the modules compiled");
    executor.set_engine(Engine::Bytecode);
    let _natives = register_natives(&natives, &compiled, &mut executor)?;

    let mut out = String::new();
    for (source, is_expr) in UNITS {
        let result = if *is_expr { compiler.compile_expr(source) } else { compiler.compile_fragment(source) };
        if let Some(error) = result.first_error() {
            bail!("`{source}` did not compile: {error}");
        }
        let ir_unit = result.ir_unit.as_ref().expect("a unit that compiled has ir");
        if *is_expr {
            let (_ty, value) = executor.execute_expr(ir_unit);
            if value.starts_with("Error:") {
                bail!("`{source}` failed: {value}");
            }
            out.push_str(&value);
            out.push('\n');
        } else {
            let output = executor.execute_fragment(ir_unit);
            if output.starts_with("Error:") {
                bail!("`{source}` failed: {output}");
            }
        }
    }
    Ok(out)
}

/// The entry points the wasm module exports, for a host with no other way in.
///
/// A run leaves its report, or the error or panic that ended it, in a buffer
/// the host reads out of linear memory. A panic here is a trap with no
/// message, so the hook writes the message there first.
#[cfg(target_arch = "wasm32")]
mod exports {
    use std::cell::RefCell;

    use super::{ParallelMode, run};

    thread_local! {
        static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    }

    fn set_output(text: &str) {
        OUTPUT.with(|output| *output.borrow_mut() = text.as_bytes().to_vec());
    }

    /// Run the script, sequentially if `parallel` is 0. Returns 0 on success.
    #[unsafe(no_mangle)]
    pub extern "C" fn smoke_run(parallel: i32) -> i32 {
        std::panic::set_hook(Box::new(|info| set_output(&format!("panic: {info}"))));
        let mode = if parallel == 0 { ParallelMode::Sequential } else { ParallelMode::Parallel };
        match run(mode) {
            Ok(report) => {
                set_output(&report);
                0
            }
            Err(error) => {
                set_output(&format!("{error:?}"));
                1
            }
        }
    }

    /// How many threads rayon's global pool has, which is what the parallel
    /// paths run on.
    #[unsafe(no_mangle)]
    pub extern "C" fn smoke_rayon_threads() -> i32 {
        rmx::rayon::current_num_threads() as i32
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn smoke_output_ptr() -> *const u8 {
        OUTPUT.with(|output| output.borrow().as_ptr())
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn smoke_output_len() -> usize {
        OUTPUT.with(|output| output.borrow().len())
    }
}
