//! Native binary entry point for datalove-repl-egui.

#![allow(unused)]

use rmx::prelude::*;


#[cfg(not(target_arch = "wasm32"))]
fn main() -> AnyResult<()> {
    datalove_repl_egui::main()
}

#[cfg(target_arch = "wasm32")]
fn main() {
    // No main for WASM - entry point is the `start()` function
}
