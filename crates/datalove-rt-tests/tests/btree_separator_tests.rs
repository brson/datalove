//! Separators in a map's or set's internal nodes are views of keys in leaves,
//! not copies, so a removal that frees a key a separator views has to renew
//! the separator before anything compares against it.
//!
//! String keys own heap buffers, which a removal frees and a later insertion
//! takes again for another key, so a separator left viewing a freed key reads
//! another key's bytes, or freed ones, and routes lookups wrong. These churn a
//! tree with insertions and removals of a pool of keys, checking every key's
//! presence as they go. Under Miri a stale separator is a use after free.

use rmx::prelude::*;
use datalove_rtdt as rtdt;
use datalove_rt::c as rt;
use std::collections::BTreeSet;

fn plain(type_tag: rtdt::TyTag, size: u32, align: u32) -> Box<rtdt::TyDesc> {
    Box::new(rtdt::TyDesc {
        type_tag,
        flags: 0,
        size,
        align,
        type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
    })
}

fn string_tydesc() -> Box<rtdt::TyDesc> {
    plain(rtdt::TyTag::String, size_of::<rtdt::String>() as u32, align_of::<rtdt::String>() as u32)
}

unsafe fn make_string(handle: rt::LocalRtHandle, ty: &rtdt::TyDesc, s: &str) -> rtdt::String {
    unsafe {
        let mut out = std::mem::MaybeUninit::<rtdt::String>::uninit();
        rt::dtlv_rti_string_create_local(handle, rtdt::AnyOut::new(out.as_mut_ptr() as *mut u8), ty);
        let mut string = out.assume_init();
        rt::dtlv_rti_string_push_bytes_local(
            handle, rtdt::AnyMut::from_mut(&mut string), ty, s.as_ptr(), (s.len() as u32).into());
        string
    }
}

unsafe fn drop_string(handle: rt::LocalRtHandle, ty: &rtdt::TyDesc, mut s: rtdt::String) {
    unsafe {
        rt::dtlv_rti_string_destroy_local(handle, rtdt::AnyOwned::new(&mut s as *mut rtdt::String as *mut u8), ty);
    }
}

/// The text of key `k`: of differing lengths, and not in `k`'s order.
fn key_text(k: u32) -> String {
    format!("{:0width$}", (k * 7919) % 100_003, width = 8 + (k % 24) as usize)
}

/// A fixed sequence of steps over a pool of `pool` keys, each a key to insert
/// if it is absent and to remove if it is present.
fn steps(pool: u32, count: usize) -> Vec<u32> {
    let mut seed = 0x2545f4914f6cdd1du64;
    (0..count).map(|_| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % pool as u64) as u32
    }).collect()
}

/// The pool and step count: smaller under Miri, which is slow.
fn scale() -> (u32, usize) {
    if cfg!(miri) { (150, 1_500) } else { (600, 30_000) }
}

/// A map with string keys churned by insertions and removals finds exactly
/// the keys it holds after every step.
#[test]
fn map_churn_finds_its_keys() {
    let (pool, count) = scale();
    let handle = rt::dtlv_rti_init();
    let string = string_tydesc();
    let u32_td = plain(rtdt::TyTag::U32, 4, 4);
    let map_td = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Map,
        flags: 0,
        size: size_of::<rtdt::Map>() as u32,
        align: align_of::<rtdt::Map>() as u32,
        type_info: rtdt::TyInfo { map: unsafe { rtdt::TyInfoMap::new(&*string, &*u32_td) } },
    });
    let mut map = rtdt::Map { root: std::ptr::null(), len: rtdt::Index::ZERO };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;
    let mut present = BTreeSet::new();

    let contains = |k: u32| unsafe {
        let probe = make_string(handle, &string, &key_text(k));
        let mut found = false;
        rt::dtlv_rti_btreemap_contains_key_local(
            handle, rtdt::AnyRef::new(map_ptr), &map_td, rtdt::AnyRef::from_ref(&probe), &string, &mut found);
        drop_string(handle, &string, probe);
        found
    };

    for (i, k) in steps(pool, count).into_iter().enumerate() {
        unsafe {
            if present.remove(&k) {
                let probe = make_string(handle, &string, &key_text(k));
                rt::dtlv_rti_btreemap_remove_local(
                    handle, rtdt::AnyMut::new(map_ptr), &map_td, rtdt::AnyRef::from_ref(&probe), &string);
                drop_string(handle, &string, probe);
            } else {
                present.insert(k);
                let mut key = make_string(handle, &string, &key_text(k));
                let mut value = k;
                rt::dtlv_rti_btreemap_insert_local(
                    handle, rtdt::AnyMut::new(map_ptr), &map_td,
                    rtdt::AnyOwned::new(&mut key as *mut rtdt::String as *mut u8), &string,
                    rtdt::AnyOwned::new(&mut value as *mut u32 as *mut u8), &u32_td);
                std::mem::forget(key);
            }
        }
        assert_eq!(map.len, rtdt::Index(present.len() as rtdt::IndexRepr), "step {i}");
        if i % 50 == 0 {
            for k in 0..pool {
                assert_eq!(contains(k), present.contains(&k), "step {i}: key {k}");
            }
        }
    }
    for k in 0..pool {
        assert_eq!(contains(k), present.contains(&k), "at the end: key {k}");
    }

    unsafe {
        rt::dtlv_rti_btreemap_destroy_local(handle, rtdt::AnyOwned::new(map_ptr), &map_td);
        assert_eq!(rt::dtlv_rti_shutdown(handle), rt::RtStatus::Ok);
    }
}

/// A set with string elements churned by insertions and removals finds
/// exactly the elements it holds after every step.
#[test]
fn set_churn_finds_its_elements() {
    let (pool, count) = scale();
    let handle = rt::dtlv_rti_init();
    let string = string_tydesc();
    let set_td = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Set,
        flags: 0,
        size: size_of::<rtdt::Set>() as u32,
        align: align_of::<rtdt::Set>() as u32,
        type_info: rtdt::TyInfo { set: unsafe { rtdt::TyInfoSet::new(&*string) } },
    });
    let mut set = std::mem::MaybeUninit::<rtdt::Set>::uninit();
    unsafe { rt::dtlv_rti_btreeset_create_local(handle, rtdt::AnyOut::new(set.as_mut_ptr() as *mut u8), &set_td) };
    let set_ptr = set.as_mut_ptr() as *mut u8;
    let mut present = BTreeSet::new();
    let mut flag = false;
    let flag_ptr = &mut flag as *mut bool as *mut u8;

    let contains = |k: u32| unsafe {
        let probe = make_string(handle, &string, &key_text(k));
        let mut found = false;
        rt::dtlv_rti_btreeset_contains_local(
            handle, rtdt::AnyRef::new(set_ptr), &set_td, rtdt::AnyRef::from_ref(&probe), &string,
            rtdt::AnyOut::new(&mut found as *mut bool as *mut u8));
        drop_string(handle, &string, probe);
        found
    };

    for (i, k) in steps(pool, count).into_iter().enumerate() {
        unsafe {
            if present.remove(&k) {
                let probe = make_string(handle, &string, &key_text(k));
                rt::dtlv_rti_btreeset_remove_local(
                    handle, rtdt::AnyMut::new(set_ptr), &set_td, rtdt::AnyRef::from_ref(&probe), &string,
                    rtdt::AnyOut::new(flag_ptr));
                drop_string(handle, &string, probe);
            } else {
                present.insert(k);
                let mut element = make_string(handle, &string, &key_text(k));
                rt::dtlv_rti_btreeset_insert_local(
                    handle, rtdt::AnyMut::new(set_ptr), &set_td,
                    rtdt::AnyOwned::new(&mut element as *mut rtdt::String as *mut u8), &string,
                    rtdt::AnyOut::new(flag_ptr));
                std::mem::forget(element);
            }
        }
        if i % 50 == 0 {
            for k in 0..pool {
                assert_eq!(contains(k), present.contains(&k), "step {i}: element {k}");
            }
        }
    }
    for k in 0..pool {
        assert_eq!(contains(k), present.contains(&k), "at the end: element {k}");
    }

    unsafe {
        rt::dtlv_rti_btreeset_destroy_local(handle, rtdt::AnyOwned::new(set_ptr), &set_td);
        assert_eq!(rt::dtlv_rti_shutdown(handle), rt::RtStatus::Ok);
    }
}
