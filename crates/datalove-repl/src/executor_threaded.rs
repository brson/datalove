//! Native multi-threaded executor using std::thread.

use rmx::prelude::*;
use crate::app::{ReplExecutor, WorkerResponse};
use crate::{Input, Command};
use datalove_datafun as datafun;
use datafun::pipeline::SystemLibrary;
use crate::engine::Engine;
use std::sync::mpsc::{channel, Sender, Receiver, TryRecvError};
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
    /// Whether the worker's death has already been reported to the app.
    ///
    /// It is reported once; repeating it would spin the app's poll loop.
    death_reported: bool,
}

impl ThreadedExecutor {
    /// Spawn the worker thread, which constructs the engine it owns.
    ///
    /// The system library is built on the worker thread rather than handed to
    /// it, because the addresses of its native functions are not `Send`.
    pub fn spawn(sys: fn() -> SystemLibrary) -> Self {
        let (main_tx, worker_rx) = channel();
        let (worker_tx, main_rx) = channel();

        thread::spawn(move || {
            // The database is created here and lives for the thread's lifetime.
            let db = datafun::Database::default();
            match start_engine(&db, sys()) {
                Ok(engine) => {
                    let _ = worker_tx.send(WorkerResponse::EngineReady);
                    worker_thread(engine, worker_rx, worker_tx);
                }
                Err(message) => {
                    let _ = worker_tx.send(WorkerResponse::EngineDead { message });
                }
            }
        });

        Self {
            worker_tx: main_tx,
            worker_rx: main_rx,
            death_reported: false,
        }
    }
}

/// Build the engine, turning a failed or panicking startup into a message.
///
/// Startup compiles the whole system library, which is where a bad stdlib
/// shows up.
fn start_engine(db: &datafun::Database, sys: SystemLibrary) -> Result<Engine<'_>, String> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Engine::new(db, sys)
    }));

    match result {
        Ok(Ok(engine)) => Ok(engine),
        Ok(Err(e)) => Err(fmt!("the engine failed to start: {e:#}")),
        Err(panic_info) => {
            let panic_msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                s.S()
            } else if let Some(s) = panic_info.downcast_ref::<String>() {
                s.C()
            } else {
                "Unknown panic".S()
            };
            Err(fmt!("the engine panicked while starting: {panic_msg}"))
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
        match self.worker_rx.try_recv() {
            Ok(response) => {
                // The worker says why it is dying before it dies; the
                // disconnect that follows adds nothing.
                if matches!(response, WorkerResponse::EngineDead { .. }) {
                    self.death_reported = true;
                }
                Some(response)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                if self.death_reported {
                    None
                } else {
                    self.death_reported = true;
                    Some(WorkerResponse::EngineDead {
                        message: "the engine thread exited".S(),
                    })
                }
            }
        }
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
