//! The runtime's B-trees, under string keys.
//!
//! A string key owns a heap buffer, so a key cloned or destroyed inside the
//! tree is an allocation or a free, and what the tree does with its keys as it
//! splits, merges and rebalances shows here, where an integer key would hide
//! it. Each run inserts `n` keys in a shuffled order into an empty map or set,
//! then removes them all in another, calling the runtime's exports directly,
//! as compiled code does. Making the keys is outside the timing.

use datalove_rt::c as rt;
use datalove_rtdt as rtdt;

fn main() {
    divan::main();
}

const SIZES: &[usize] = &[1_000, 10_000, 100_000];

/// Descriptors for a `string`, a `u32`, a `%{string = u32}` and a
/// `#{string}`, leaked: a benchmark process makes one set.
struct Types {
    string: &'static rtdt::TyDesc,
    u32: &'static rtdt::TyDesc,
    map: &'static rtdt::TyDesc,
    set: &'static rtdt::TyDesc,
}

fn plain(type_tag: rtdt::TyTag, size: u32, align: u32) -> &'static rtdt::TyDesc {
    Box::leak(Box::new(rtdt::TyDesc {
        type_tag,
        flags: 0,
        size,
        align,
        type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
    }))
}

fn types() -> Types {
    let string = plain(rtdt::TyTag::String, size_of::<rtdt::String>() as u32, align_of::<rtdt::String>() as u32);
    let u32 = plain(rtdt::TyTag::U32, 4, 4);
    let map = Box::leak(Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Map,
        flags: 0,
        size: size_of::<rtdt::Map>() as u32,
        align: align_of::<rtdt::Map>() as u32,
        type_info: rtdt::TyInfo { map: unsafe { rtdt::TyInfoMap::new(string, u32) } },
    }));
    let set = Box::leak(Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Set,
        flags: 0,
        size: size_of::<rtdt::Set>() as u32,
        align: align_of::<rtdt::Set>() as u32,
        type_info: rtdt::TyInfo { set: unsafe { rtdt::TyInfoSet::new(string) } },
    }));
    Types { string, u32, map, set }
}

/// A deterministic shuffle of `0..n`.
fn shuffled(n: usize, mut seed: u64) -> Vec<usize> {
    let mut v: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        v.swap(i, (seed % (i as u64 + 1)) as usize);
    }
    v
}

fn key_text(i: usize) -> String {
    format!("key number {i:010}")
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

/// A runtime, the keys to insert in their order, and the keys to remove by.
struct Input {
    handle: rt::LocalRtHandle,
    inserts: Vec<rtdt::String>,
    removes: Vec<rtdt::String>,
}

fn input(ty: &Types, n: usize) -> Input {
    let handle = rt::dtlv_rti_init();
    let make = |order: Vec<usize>| order.into_iter()
        .map(|i| unsafe { make_string(handle, ty.string, &key_text(i)) })
        .collect();
    Input { handle, inserts: make(shuffled(n, 0x9e3779b97f4a7c15)), removes: make(shuffled(n, 0xd1b54a32d192ed03)) }
}

/// Destroy the keys removed by, and the runtime.
fn finish(ty: &Types, handle: rt::LocalRtHandle, mut removes: Vec<rtdt::String>) {
    unsafe {
        for key in removes.iter_mut() {
            rt::dtlv_rti_string_destroy_local(handle, rtdt::AnyOwned::new(key as *mut rtdt::String as *mut u8), ty.string);
        }
        std::mem::forget(removes);
        rt::dtlv_rti_shutdown(handle);
    }
}

#[divan::bench(args = SIZES)]
fn map_insert_remove(bencher: divan::Bencher, n: usize) {
    let ty = types();
    bencher.with_inputs(|| input(&ty, n)).bench_local_values(|i| run_map(&ty, i));
}

fn run_map(ty: &Types, Input { handle, inserts, removes }: Input) {
    unsafe {
        let mut map = rtdt::Map { root: std::ptr::null(), len: rtdt::Index::ZERO };
        rt::dtlv_rti_btreemap_create_local(handle, rtdt::AnyOut::new(&mut map as *mut _ as *mut u8), ty.map);
        for (i, mut key) in inserts.into_iter().enumerate() {
            let mut value = i as u32;
            rt::dtlv_rti_btreemap_insert_local(
                handle, rtdt::AnyMut::from_mut(&mut map), ty.map,
                rtdt::AnyOwned::new((&mut key) as *mut _ as *mut u8), ty.string,
                rtdt::AnyOwned::new((&mut value) as *mut _ as *mut u8), ty.u32);
            std::mem::forget(key);
        }
        for key in &removes {
            rt::dtlv_rti_btreemap_remove_local(
                handle, rtdt::AnyMut::from_mut(&mut map), ty.map, rtdt::AnyRef::from_ref(key), ty.string);
        }
        assert_eq!(map.len, rtdt::Index::ZERO);
        rt::dtlv_rti_btreemap_destroy_local(handle, rtdt::AnyOwned::new((&mut map) as *mut _ as *mut u8), ty.map);
        finish(ty, handle, removes);
    }
}

#[divan::bench(args = SIZES)]
fn set_insert_remove(bencher: divan::Bencher, n: usize) {
    let ty = types();
    bencher.with_inputs(|| input(&ty, n)).bench_local_values(|i| run_set(&ty, i));
}

fn run_set(ty: &Types, Input { handle, inserts, removes }: Input) {
    unsafe {
        let mut set = std::mem::MaybeUninit::<rtdt::Set>::uninit();
        rt::dtlv_rti_btreeset_create_local(handle, rtdt::AnyOut::new(set.as_mut_ptr() as *mut u8), ty.set);
        let mut set = set.assume_init();
        let mut flag = false;
        for mut key in inserts {
            rt::dtlv_rti_btreeset_insert_local(
                handle, rtdt::AnyMut::from_mut(&mut set), ty.set,
                rtdt::AnyOwned::new((&mut key) as *mut _ as *mut u8), ty.string,
                rtdt::AnyOut::new(&mut flag as *mut bool as *mut u8));
            std::mem::forget(key);
        }
        for key in &removes {
            rt::dtlv_rti_btreeset_remove_local(
                handle, rtdt::AnyMut::from_mut(&mut set), ty.set, rtdt::AnyRef::from_ref(key), ty.string,
                rtdt::AnyOut::new(&mut flag as *mut bool as *mut u8));
        }
        rt::dtlv_rti_btreeset_destroy_local(handle, rtdt::AnyOwned::new((&mut set) as *mut _ as *mut u8), ty.set);
        finish(ty, handle, removes);
    }
}
