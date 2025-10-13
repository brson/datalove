//! Web Worker WASM module for REPL evaluation.
//! This is compiled as a separate WASM module and loaded by the main app.

use wasm_bindgen::prelude::*;

/// Worker entry point with #[wasm_bindgen(start)] attribute.
/// This wrapper ensures the function only runs automatically in the worker context,
/// not in the main app which also links to datalove-repl-rat.
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    datalove_repl_rat::worker::worker_main()
}
