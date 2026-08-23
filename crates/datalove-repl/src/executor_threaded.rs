//! Native multi-threaded executor using std::thread.

use rmx::prelude::*;
use crate::app::{ReplExecutor, WorkerResponse};
use crate::{Input, Command};
use datalove_datafun as datafun;
use crate::engine::Engine;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;

/// Request sent to the worker thread.
#[derive(Debug)]
enum WorkerRequest {
    Parse { id: u64, input: Input },
    Eval { id: u64, command: Command },
    #[allow(dead_code)]
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

impl ThreadedExecutor {
    /// Spawn the worker thread, which constructs the engine it owns.
    pub fn spawn() -> Self {
        let (main_tx, worker_rx) = channel();
        let (worker_tx, main_rx) = channel();

        thread::spawn(move || {
            // The database is created here and lives for the thread's lifetime.
            let db = datafun::Database::default();
            let engine = Engine::new(&db).X();
            worker_thread(engine, worker_rx, worker_tx);
        });

        Self {
            worker_tx: main_tx,
            worker_rx: main_rx,
        }
    }
}

impl ReplExecutor for ThreadedExecutor {
    fn submit_parse(&mut self, id: u64, input: Input) {
        let _ = self.worker_tx.send(WorkerRequest::Parse { id, input });
    }

    fn submit_eval(&mut self, id: u64, command: Command) {
        let _ = self.worker_tx.send(WorkerRequest::Eval { id, command });
    }

    fn try_recv_response(&mut self) -> Option<WorkerResponse> {
        self.worker_rx.try_recv().ok()
    }
}

/// Worker thread that handles parse and eval operations.
fn worker_thread(
    mut engine: Engine,
    rx: Receiver<WorkerRequest>,
    tx: Sender<WorkerResponse>,
) {
    loop {
        match rx.recv() {
            Ok(WorkerRequest::Parse { id, input }) => {
                // Parse the input.
                let parse = engine.parse_input(input);
                let _ = tx.send(WorkerResponse::ParseResult {
                    id,
                    parse,
                });
            }
            Ok(WorkerRequest::Eval { id, command }) => {
                // Evaluate the command.
                let eval = engine.eval(command);

                // Get updated environment.
                let environment = engine.get_environment();

                // Send eval result with environment.
                let _ = tx.send(WorkerResponse::EvalResult { id, eval, environment });
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
