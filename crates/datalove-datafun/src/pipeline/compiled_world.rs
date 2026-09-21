//! A database and the pipeline that compiled its modules, kept together.
//!
//! Compiling a module world is the largest cost in a datalove invocation -- the
//! standard library is 81ms of a release run -- and it repeats not because the
//! database is thrown away but because the *pipeline* is. A pipeline owns the
//! `Source` inputs every tracked query is keyed on, so a new one makes new inputs
//! and nothing downstream can hit the memo. Keeping the same pipeline, compiling
//! the standard library a second time costs about a ninth of the first.
//!
//! So this is not a cache. It is the pair a caller has to hold on to if it wants
//! salsa to do what salsa is for, and it is ordinary owned state: no leak, no
//! thread-local, dropped when its owner drops.
//!
//! # One per thread
//!
//! A salsa `Database` is `Send` and not `Sync`, by design: its storage holds a
//! `ZalsaLocal` which is the per-thread query stack. `Storage::clone` keeps the
//! `Arc<Zalsa>` -- every memo -- and makes a fresh `ZalsaLocal`, so cloning is how
//! another thread gets a handle onto the same work. A `CompiledWorld` therefore
//! belongs to one thread at a time; what it produces does not, since an
//! `IrCodeUnit` is `Send + Sync` and carries nothing of salsa with it.

use crate::pipeline::{CompiledModules, ModuleCompilationPipeline, WorkspaceDescriptor};
use datalove_datafun_compiler::Database;

/// A database, and the pipeline whose inputs live in it.
///
/// Compile against it repeatedly rather than building one per script: that is the
/// whole point, and the reason it is a struct rather than a function.
pub struct CompiledWorld {
    db: Database,
    pipeline: ModuleCompilationPipeline,
}

impl CompiledWorld {
    /// Build the world `descriptor` describes.
    ///
    /// The modules are not compiled yet; the first [`compile`](Self::compile)
    /// does that and pays for it, and every one after reuses what it can.
    pub fn new(descriptor: &WorkspaceDescriptor) -> Self {
        let db = Database::default();
        let pipeline = descriptor.to_pipeline(&db);
        Self { db, pipeline }
    }

    /// The database the modules were compiled against.
    ///
    /// Wanted by a caller that goes on to compile a script, since the script's
    /// own compilation happens against the same database.
    pub fn db(&self) -> &Database {
        &self.db
    }

    /// Compile the modules, reusing whatever the last call left behind.
    ///
    /// Borrows `self` for as long as the result lives, which is what keeps this
    /// honest: the result names salsa values that belong to this database, so it
    /// cannot outlive the world that produced it.
    pub fn compile(&mut self) -> CompiledModules<'_> {
        self.pipeline.compile_fresh(&self.db)
    }

    /// Run `f` against the compiled modules.
    ///
    /// For a caller that wants the modules and the database at once without
    /// arguing with the borrow checker about which of the two it has.
    pub fn with_compiled<R>(&mut self, f: impl FnOnce(&Database, &CompiledModules<'_>) -> R) -> R {
        let compiled = self.pipeline.compile_fresh(&self.db);
        f(&self.db, &compiled)
    }
}
