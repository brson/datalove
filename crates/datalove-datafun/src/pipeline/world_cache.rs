//! A compiled module world, kept for the life of the process.
//!
//! Compiling a module world is the largest cost in a datalove invocation -- the
//! standard library is 81ms of a release run, and a test suite that compiles it
//! once per fixture spends almost all of its time there -- and it produces the
//! same answer every time. This hands out the answer instead.
//!
//! What is cached is the whole bundle rather than the [`CompiledModules`] alone,
//! because that borrows both the database and the pipeline it came from, so the
//! three have to live or die together.
//!
//! # Per thread, not per process
//!
//! A salsa `Database` is `Send` but not `Sync`: its storage holds a `ZalsaLocal`
//! with a `RefCell` and an `UnsafeCell` inside it. So a `static` holding a
//! reference to one does not compile, and a process-wide cache would have to move
//! the bundle between threads under a lock -- lending it to one thread at a time,
//! which serialises every caller. A thread-local hands out a reference only
//! within the thread that owns it and needs no `Sync` at all. The cost is one
//! compilation per thread that asks rather than one per process, which for a test
//! runner is one per worker.
//!
//! # It is never freed
//!
//! The bundle is leaked, for two reasons: it is self-referential, so there is no
//! owner to hand back, and a cache that is dropped is not one. A caller that
//! wants the memory back wants a different function.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::pipeline::{CompiledModules, ModuleCompilationPipeline, WorkspaceDescriptor};
use datalove_datafun_compiler::Database;

/// A compiled world and everything it borrows from.
#[derive(Copy, Clone)]
pub struct World {
    /// The database the compiled modules were compiled against.
    pub db: &'static Database,
    /// What was compiled.
    pub descriptor: &'static WorkspaceDescriptor,
    /// The result.
    pub compiled: &'static CompiledModules<'static>,
}

thread_local! {
    /// Worlds this thread has built, by the key its caller named them with.
    static WORLDS: RefCell<HashMap<String, World>> = RefCell::new(HashMap::new());
}

/// The compiled world for `key`, building it with `build` if this thread has not
/// asked for it before.
///
/// `key` names the world rather than describing it: a descriptor is expensive to
/// compare and two callers that mean the same world know that better than
/// anything here could work it out. Naming two different worlds the same thing
/// hands out the first, which is a caller's bug and not a detectable one.
///
/// `build` runs at most once per thread per key. Whatever it panics with, or
/// whatever the compilation reports as a module error, is a failure of the world
/// rather than of the caller asking for it, so it is left to propagate.
pub fn with_world<R>(
    key: &str,
    build: impl FnOnce() -> WorkspaceDescriptor,
    f: impl FnOnce(World) -> R,
) -> R {
    // The borrow is dropped before `f` runs, because `f` may ask for a world of
    // its own.
    let existing = WORLDS.with(|worlds| worlds.borrow().get(key).copied());
    let world = match existing {
        Some(world) => world,
        None => {
            let world = build_world(build());
            WORLDS.with(|worlds| worlds.borrow_mut().insert(key.to_string(), world));
            world
        }
    };
    f(world)
}

/// Compile `descriptor` and leak the result.
fn build_world(descriptor: WorkspaceDescriptor) -> World {
    let db: &'static Database = Box::leak(Box::new(Database::default()));
    let descriptor: &'static WorkspaceDescriptor = Box::leak(Box::new(descriptor));
    // Leaked rather than owned because `compile_fresh` borrows it for as long as
    // its result lives.
    let pipeline: &'static mut ModuleCompilationPipeline =
        Box::leak(Box::new(descriptor.to_pipeline(db)));
    let compiled: &'static CompiledModules<'static> =
        Box::leak(Box::new(pipeline.compile_fresh(db)));

    World { db, descriptor, compiled }
}
