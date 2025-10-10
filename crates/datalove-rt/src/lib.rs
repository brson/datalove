//! Datalove runtime.

#![allow(unused)]

use rmx::prelude::*;

pub use datalove_rtdt as rtdt;

pub type RtHandle = *mut u8;

/// For basic success-fail functions.
#[repr(u8)]
pub enum RtResult {
    Ok = 1,
    Err = 2,
}
