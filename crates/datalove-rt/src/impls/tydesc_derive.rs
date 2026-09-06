//! Type descriptors derived at run time from the descriptors of their parts.
//!
//! A generic that builds a collection needs a descriptor for the collection,
//! and the call site supplied one only for the type parameter: a `T`, not a
//! `[T]`. The call site cannot supply the `[T]` without being told which
//! shapes the callee builds, which is a question about the callee's body
//! rather than its signature, so the descriptor for the shape is put together
//! here instead.
//!
//! Deriving one is a small fixed job. A list descriptor is a tag, a size, an
//! alignment and a pointer at its element's; none of that depends on what the
//! element is. The same holds for a set, and a map is the same with two.
//!
//! Results are interned, so a given element type yields one descriptor however
//! often it is asked for. That matters for more than speed: `types_equivalent`
//! and the erasure checks compare descriptors by pointer in places, and two
//! descriptions of the same list type have to be the same pointer.
//!
//! An interned descriptor is never freed. It is immutable, there is one per
//! distinct element type the program reaches, and a value built with one can
//! outlive any particular call, so there is no point at which freeing would be
//! safe. They are allocated with rust's allocator rather than the runtime's,
//! so the leak check does not count them.

use std::collections::HashMap;
use std::sync::Mutex;

use datalove_rtdt as rtdt;
use rtdt::{TyDesc, TyTag};

/// Interned derived descriptors, keyed by shape and the parts it was built
/// from.
///
/// The addresses are held as integers because a `TyDesc` holds raw pointers
/// and so is not `Sync`. What they point at is immutable and leaked, so
/// handing the address back out is sound.
static DERIVED: Mutex<Option<HashMap<(u8, usize, usize), usize>>> = Mutex::new(None);

/// Look up a derived descriptor, building it the first time it is asked for.
fn intern(
    tag: TyTag,
    first: *const TyDesc,
    second: *const TyDesc,
    make: impl FnOnce() -> TyDesc,
) -> *const TyDesc {
    let key = (tag as u8, first as usize, second as usize);
    let mut guard = DERIVED.lock().expect("derived tydesc table poisoned");
    let table = guard.get_or_insert_with(HashMap::new);
    if let Some(found) = table.get(&key) {
        return *found as *const TyDesc;
    }
    let leaked: &'static TyDesc = Box::leak(Box::new(make()));
    let addr = leaked as *const TyDesc as usize;
    table.insert(key, addr);
    addr as *const TyDesc
}

/// The descriptor for a list of `element`.
pub fn list_of(element: *const TyDesc) -> *const TyDesc {
    intern(TyTag::List, element, std::ptr::null(), || TyDesc {
        type_tag: TyTag::List,
        size: std::mem::size_of::<rtdt::List>() as u32,
        align: std::mem::align_of::<rtdt::List>() as u32,
        type_info: rtdt::TyInfo {
            list: rtdt::TyInfoList { element_tydesc: element },
        },
    })
}

/// The descriptor for a set of `element`.
pub fn set_of(element: *const TyDesc) -> *const TyDesc {
    intern(TyTag::Set, element, std::ptr::null(), || TyDesc {
        type_tag: TyTag::Set,
        size: std::mem::size_of::<rtdt::Set>() as u32,
        align: std::mem::align_of::<rtdt::Set>() as u32,
        type_info: rtdt::TyInfo {
            set: rtdt::TyInfoSet { element_tydesc: element },
        },
    })
}

/// The descriptor for a map from `key` to `value`.
pub fn map_of(key: *const TyDesc, value: *const TyDesc) -> *const TyDesc {
    intern(TyTag::Map, key, value, || TyDesc {
        type_tag: TyTag::Map,
        size: std::mem::size_of::<rtdt::Map>() as u32,
        align: std::mem::align_of::<rtdt::Map>() as u32,
        type_info: rtdt::TyInfo {
            map: rtdt::TyInfoMap { key_tydesc: key, value_tydesc: value },
        },
    })
}
