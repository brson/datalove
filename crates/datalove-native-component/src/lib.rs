//! The runtime and native riders, as one library an AOT-compiled program links.
//!
//! An AOT-compiled datalove program is a native executable that calls the
//! runtime and the rider functions its modules use. Both live here so that a
//! program links one library rather than assembling it from crates.
//!
//! The `extern crate` declarations are what pull the runtime's and the riders'
//! `#[no_mangle]` symbols into the archive; nothing here calls them.

extern crate datalove_rt;
extern crate datalove_rider_std;
