//! Web Worker-based executor for WASM.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Worker, WorkerOptions, WorkerType, MessageEvent, ErrorEvent};
use serde::{Serialize, Deserialize};
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::VecDeque;

use crate::executor::{ReplExecutor, WorkerResponse};
use crate::repl;

/// Request sent from main thread to worker.
#[derive(Debug, Serialize, Deserialize)]
enum WorkerRequest {
    Parse { id: u64, input: repl::Input },
    Eval { id: u64, command: repl::Command },
}

/// Web Worker-based executor for WASM platforms.
///
/// Spawns a Web Worker to handle parse and eval operations,
/// keeping the UI thread responsive.
pub struct WebWorkerExecutor {
    worker: Worker,
    response_queue: Rc<RefCell<VecDeque<WorkerResponse>>>,
    _onmessage_closure: Closure<dyn FnMut(MessageEvent)>,
    _onerror_closure: Closure<dyn FnMut(ErrorEvent)>,
}

impl ReplExecutor for WebWorkerExecutor {
    fn new() -> Self {
        web_sys::console::log_1(&"WebWorkerExecutor: Creating worker...".into());

        // Create the worker as a module worker to support ES6 imports.
        // The worker script is in the worker-dist directory.
        let mut options = WorkerOptions::new();
        options.set_type(WorkerType::Module);

        let worker = Worker::new_with_options("./worker-dist/worker.js", &options)
            .expect("Failed to create worker");

        web_sys::console::log_1(&"WebWorkerExecutor: Worker created".into());

        // Create shared response queue.
        let response_queue = Rc::new(RefCell::new(VecDeque::new()));

        // Set up message handler.
        let queue_clone = response_queue.clone();
        let onmessage = Closure::wrap(Box::new(move |event: MessageEvent| {
            let data = event.data();
            let json_str = match data.as_string() {
                Some(s) => s,
                None => {
                    web_sys::console::error_1(&"Main: Received non-string message from worker".into());
                    return;
                }
            };

            // Deserialize the response.
            let response: WorkerResponse = match serde_json::from_str(&json_str) {
                Ok(resp) => resp,
                Err(e) => {
                    web_sys::console::error_1(&format!("Main: Failed to parse worker response: {}", e).into());
                    return;
                }
            };

            // Add to response queue.
            queue_clone.borrow_mut().push_back(response);
        }) as Box<dyn FnMut(MessageEvent)>);

        worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        // Set up error handler.
        let onerror = Closure::wrap(Box::new(move |event: ErrorEvent| {
            web_sys::console::error_1(&format!("Worker error: {}", event.message()).into());
        }) as Box<dyn FnMut(ErrorEvent)>);

        worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));

        web_sys::console::log_1(&"WebWorkerExecutor: Initialized".into());

        Self {
            worker,
            response_queue,
            _onmessage_closure: onmessage,
            _onerror_closure: onerror,
        }
    }

    fn submit_parse(&mut self, id: u64, input: repl::Input) {
        web_sys::console::log_1(&format!("WebWorkerExecutor: Submitting parse request id={}", id).into());

        let request = WorkerRequest::Parse { id, input };
        let json = serde_json::to_string(&request)
            .expect("Failed to serialize request");

        if let Err(e) = self.worker.post_message(&JsValue::from_str(&json)) {
            web_sys::console::error_1(&format!("Failed to post message to worker: {:?}", e).into());
        }
    }

    fn submit_eval(&mut self, id: u64, command: repl::Command) {
        web_sys::console::log_1(&format!("WebWorkerExecutor: Submitting eval request id={}", id).into());

        let request = WorkerRequest::Eval { id, command };
        let json = serde_json::to_string(&request)
            .expect("Failed to serialize request");

        if let Err(e) = self.worker.post_message(&JsValue::from_str(&json)) {
            web_sys::console::error_1(&format!("Failed to post message to worker: {:?}", e).into());
        }
    }

    fn try_recv_response(&mut self) -> Option<WorkerResponse> {
        self.response_queue.borrow_mut().pop_front()
    }
}

impl Drop for WebWorkerExecutor {
    fn drop(&mut self) {
        web_sys::console::log_1(&"WebWorkerExecutor: Terminating worker".into());
        self.worker.terminate();
    }
}
