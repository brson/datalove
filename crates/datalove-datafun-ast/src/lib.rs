pub mod ast;
pub mod ast_serde;
pub mod reachable;
pub mod script;
pub mod spans;

/// The datalit crate, whose types appear throughout this AST.
///
/// Re-exported so a consumer can name a `TypeHint` variant without taking its
/// own dependency on datalit just to match on what this crate handed it.
pub use datalove_datalit as datalit;
