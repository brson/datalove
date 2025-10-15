//! Native multi-threaded executor using std::thread.

use rmx::prelude::*;
use crate::repl;
use crate::executor::{ReplExecutor, WorkerResponse};
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;

/// Request sent to the worker thread.
#[derive(Debug)]
enum WorkerRequest {
    ParseAndEval { id: u64, input: String },
    Shutdown,
}

/// Multi-threaded executor for native platforms.
///
/// Spawns a worker thread to handle parse and eval operations,
/// keeping the UI thread responsive.
pub struct ThreadedExecutor {
    worker_tx: Sender<WorkerRequest>,
    worker_rx: Receiver<WorkerResponse>,
}

impl ReplExecutor for ThreadedExecutor {
    fn new() -> Self {
        let (main_tx, worker_rx) = channel();
        let (worker_tx, main_rx) = channel();

        // Spawn worker thread.
        thread::spawn(move || {
            // Construct Engine in worker thread.
            let mut engine = repl::Engine::new().X();
            worker_thread(engine, worker_rx, worker_tx);
        });

        Self {
            worker_tx: main_tx,
            worker_rx: main_rx,
        }
    }

    fn submit_parse_and_eval(&mut self, id: u64, input: String) {
        let _ = self.worker_tx.send(WorkerRequest::ParseAndEval { id, input });
    }

    fn try_recv_response(&mut self) -> Option<WorkerResponse> {
        self.worker_rx.try_recv().ok()
    }
}

/// Worker thread that handles parse and eval operations.
fn worker_thread(
    mut engine: repl::Engine,
    rx: Receiver<WorkerRequest>,
    tx: Sender<WorkerResponse>,
) {
    loop {
        match rx.recv() {
            Ok(WorkerRequest::ParseAndEval { id, input }) => {
                // Parse the input.
                let parse = engine.parse_input(&input);
                let _ = tx.send(WorkerResponse::ParseResult {
                    id,
                    parse: parse.clone(),
                });

                // Evaluate if we got a command.
                if let repl::InputParse::Command(command) = parse {
                    let eval = engine.eval(command);
                    let _ = tx.send(WorkerResponse::EvalResult { id, eval });

                    // Send updated environment.
                    let environment = engine.get_environment();
                    let _ = tx.send(WorkerResponse::EnvironmentUpdate { environment });
                }
            }
            Ok(WorkerRequest::Shutdown) => {
                // todo actually send this
                unreachable!();
            }
            Err(_) => {
                // todo shouldn't happen
                break;
            }
        }
    }
}
