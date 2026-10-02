//! What holds a rider library open, and what closes it.
//!
//! A rider library is mapped code that raw pointers lead into, so when it
//! closes decides whether those pointers are still code. These check the
//! arrangement directly rather than through an interpreter: two holders of one
//! library, and the library going when the last of them does.

use datalove_datafun as datafun;
use datafun::pipeline::rider_build::build_rider_dylib;
use datafun::pipeline::rider_load::{load_rider_library, open_library_count};
use datafun::pipeline::workspace::RiderCrate;
use datalove_datafun_interp::NativeFunctionTable;
use std::path::PathBuf;

/// A symbol every build of the `sys/std` rider exports.
const A_SYMBOL: &str = "dlr_std__f32_cos";

fn std_rider() -> RiderCrate {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../sys/std/rider")
        .canonicalize()
        .expect("sys/std/rider is in the tree");
    RiderCrate {
        rider_name: "std".to_string(),
        crate_name: "datalove-rider-sys-std".to_string(),
        version: "0.1.0".to_string(),
        dir: Some(dir),
    }
}

/// Build the rider dylib once for this test binary.
fn dylib() -> PathBuf {
    let work_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/datalove-work/rider_sharing_tests");
    std::fs::create_dir_all(&work_dir).expect("work dir");
    build_rider_dylib(&work_dir, &[std_rider()]).expect("the sys/std rider builds")
}

/// Two holders of one library, as two interpreters using the same rider are.
///
/// One test rather than several, because the registry is the whole process's
/// and a count is only worth asserting when nothing else is holding anything.
#[test]
fn a_library_is_opened_once_and_closed_by_its_last_holder() {
    let path = dylib();
    let symbols = vec![A_SYMBOL.to_string()];

    let mut table_one = NativeFunctionTable::new();
    let one = load_rider_library(&path, "std", &symbols, &mut table_one)
        .expect("first load");

    let mut table_two = NativeFunctionTable::new();
    let two = load_rider_library(&path, "std", &symbols, &mut table_two)
        .expect("second load");

    assert_eq!(open_library_count(), 1, "one path is one library, however often it is asked for");
    assert!(
        std::sync::Arc::ptr_eq(&one.library, &two.library),
        "both holders have the same library, not a copy each",
    );

    // Each found the same code, which is what makes sharing the mapping sound.
    assert_eq!(one.native_fn_ptrs, two.native_fn_ptrs);

    // One holder going is not the library going.
    drop(one);
    assert_eq!(open_library_count(), 1, "the other holder still has it");

    let code = two.native_fn_ptrs.clone();
    drop(two);
    drop(table_one);
    drop(table_two);
    assert_eq!(open_library_count(), 0, "the last holder going closes it");

    // And it can be taken again, the registry not left holding a dead entry.
    let mut table_three = NativeFunctionTable::new();
    let three = load_rider_library(&path, "std", &symbols, &mut table_three)
        .expect("load again after the library closed");
    assert_eq!(open_library_count(), 1);
    assert_eq!(three.native_fn_ptrs, code, "the same library gives the same code");

    // The table is a holder in its own right, which is the point: the caller's
    // copy going does not take the code out from under a table that can still
    // be called through. This is what the drop order at each caller used to
    // have to say by hand.
    drop(three);
    assert_eq!(
        open_library_count(), 1,
        "the table registered against this library still holds it",
    );

    drop(table_three);
    assert_eq!(open_library_count(), 0, "and going itself releases it");
}
