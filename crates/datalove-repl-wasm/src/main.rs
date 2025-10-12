//! Native binary entry point for datalove-repl-wasm.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    datalove_repl_wasm::main()
}

#[cfg(target_arch = "wasm32")]
fn main() {
    // No main for WASM - entry point is the `start()` function
}
