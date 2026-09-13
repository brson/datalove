//! Worldfile generator for datalove-datafun.
//!
//! Generates worldfiles that fully typecheck, containing multiple modules
//! with type aliases, functions, control flow, and cross-module imports.

mod config;
mod context;
mod gen_type;
mod gen_expr;
mod gen_stmt;
mod gen_function;
mod gen_generic;
mod gen_module;
mod gen_script;
mod pretty;
mod worldfile;

pub use config::WorldGenConfig;
pub use datalove_datalit::Database;
pub use worldfile::gen_worldfile_seeded;
