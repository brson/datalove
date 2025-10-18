//! Web Worker entry point for REPL evaluation.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{DedicatedWorkerGlobalScope, MessageEvent};
use serde::{Serialize, Deserialize};

use crate::repl;
use crate::executor::WorkerResponse;

/// Request sent from main thread to worker.
#[derive(Debug, Serialize, Deserialize)]
enum WorkerRequest {
    Parse { id: u64, input: repl::Input },
    Eval { id: u64, command: repl::Command },
}

/// Entry point for the Web Worker.
/// Note: This function doesn't have #[wasm_bindgen(start)] because it's
/// included in both the main app and worker modules. The worker-specific
/// crate adds the start attribute.
pub fn worker_main() -> Result<(), JsValue> {
    // Set up panic hook for debugging.
    console_error_panic_hook::set_once();

    // Log that worker has started.
    web_sys::console::log_1(&"Worker: Initializing...".into());

    // Get the worker global scope.
    let global = js_sys::global()
        .dyn_into::<DedicatedWorkerGlobalScope>()
        .map_err(|_| JsValue::from_str("Not running in a worker context"))?;

    // Create the REPL engine with leaked database (lives for worker lifetime).
    let db: &'static repl::datafun::Database = Box::leak(Box::new(repl::datafun::Database::default()));
    let mut engine = repl::Engine::new(db)
        .map_err(|e| JsValue::from_str(&format!("Failed to create engine: {}", e)))?;

    web_sys::console::log_1(&"Worker: Engine created successfully".into());

    // Set up message handler.
    let onmessage = Closure::wrap(Box::new(move |event: MessageEvent| {
        // Parse the incoming message.
        let data = event.data();
        let json_str = match data.as_string() {
            Some(s) => s,
            None => {
                web_sys::console::error_1(&"Worker: Received non-string message".into());
                return;
            }
        };

        // Deserialize the request.
        let request: WorkerRequest = match serde_json::from_str(&json_str) {
            Ok(req) => req,
            Err(e) => {
                web_sys::console::error_1(&format!("Worker: Failed to parse request: {}", e).into());
                return;
            }
        };

        match request {
            WorkerRequest::Parse { id, input } => {
                web_sys::console::log_1(&format!("Worker: Processing parse request id={}", id).into());

                // Parse the input.
                let parse = engine.parse_input(input);
                let parse_response = WorkerResponse::ParseResult {
                    id,
                    parse,
                };

                // Send parse result back to main thread.
                let parse_json = serde_json::to_string(&parse_response).unwrap();
                if let Err(e) = post_message(&parse_json) {
                    web_sys::console::error_1(&format!("Worker: Failed to send parse result: {:?}", e).into());
                }
            }
            WorkerRequest::Eval { id, command } => {
                web_sys::console::log_1(&format!("Worker: Processing eval request id={}", id).into());

                // Evaluate the command.
                let eval = engine.eval(command);

                // Get updated environment.
                let environment = engine.get_environment();

                // Send eval result with environment.
                let eval_response = WorkerResponse::EvalResult {
                    id,
                    eval,
                    environment,
                };

                let eval_json = serde_json::to_string(&eval_response).unwrap();
                if let Err(e) = post_message(&eval_json) {
                    web_sys::console::error_1(&format!("Worker: Failed to send eval result: {:?}", e).into());
                }
            }
        }
    }) as Box<dyn FnMut(MessageEvent)>);

    global.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

    // Keep the closure alive for the lifetime of the worker.
    onmessage.forget();

    web_sys::console::log_1(&"Worker: Ready to receive messages".into());

    Ok(())
}

/// Helper function to post a message back to the main thread.
fn post_message(message: &str) -> Result<(), JsValue> {
    let global = js_sys::global()
        .dyn_into::<DedicatedWorkerGlobalScope>()?;
    global.post_message(&JsValue::from_str(message))
}
