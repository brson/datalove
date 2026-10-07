//! The datalove language.
//!
//! This crate re-exports the crates that make up the datalove toolchain.

pub use bct;
pub use datalove_datalit as datalit;
pub use datalove_datafun as datafun;
pub use datalove_datafun_ast as datafun_ast;
pub use datalove_datafun_ir as datafun_ir;
pub use datalove_datafun_interp as datafun_interp;
pub use datalove_datafun_cranelift_jit as datafun_jit;
pub use datalove_datafun_pkg as datafun_pkg;
pub use datalove_datafun_sema as datafun_sema;
pub use datalove_diagnostic as diagnostic;
pub use datalove_paths as paths;
pub use datalove_repl as repl;
pub use datalove_rt as rt;
pub use datalove_sys_packages as sys_packages;
