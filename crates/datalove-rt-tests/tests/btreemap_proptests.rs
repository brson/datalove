//! Property-based tests for btreemap runtime functions.

#![cfg(feature = "slow_tests")]

use rmx::prelude::*;
use datalove_rtdt as rtdt;
use std::ptr;
use proptest::prelude::*;

/// Create a Map<u32, u32> type descriptor.
fn create_map_u32_u32_tydesc() -> (Box<rtdt::TyDesc>, Box<rtdt::TyDesc>, Box<rtdt::TyDesc>) {
    let key_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::U32,
        size: 4,
        align: 4,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing,
        },
    });

    let value_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::U32,
        size: 4,
        align: 4,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing,
        },
    });

    let map_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Map,
        size: std::mem::size_of::<rtdt::Map>() as u32,
        align: std::mem::align_of::<rtdt::Map>() as u32,
        type_info: rtdt::TyInfo {
            map: rtdt::TyInfoMap {
                key_tydesc: &*key_tydesc as *const rtdt::TyDesc,
                value_tydesc: &*value_tydesc as *const rtdt::TyDesc,
            },
        },
    });

    (map_tydesc, key_tydesc, value_tydesc)
}


// ==================== String type helpers ====================

/// Create a String type descriptor.
fn create_string_tydesc() -> Box<rtdt::TyDesc> {
    Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::String,
        size: std::mem::size_of::<rtdt::String>() as u32,
        align: std::mem::align_of::<rtdt::String>() as u32,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing,
        },
    })
}

/// Create a Map<String, String> type descriptor.
fn create_map_string_string_tydesc() -> (Box<rtdt::TyDesc>, Box<rtdt::TyDesc>, Box<rtdt::TyDesc>) {
    let key_tydesc = create_string_tydesc();
    let value_tydesc = create_string_tydesc();

    let map_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Map,
        size: std::mem::size_of::<rtdt::Map>() as u32,
        align: std::mem::align_of::<rtdt::Map>() as u32,
        type_info: rtdt::TyInfo {
            map: rtdt::TyInfoMap {
                key_tydesc: &*key_tydesc as *const rtdt::TyDesc,
                value_tydesc: &*value_tydesc as *const rtdt::TyDesc,
            },
        },
    });

    (map_tydesc, key_tydesc, value_tydesc)
}


/// Helper to create a String from a &str using the runtime.
unsafe fn create_runtime_string(
    rt: datalove_rt::c::LocalRtHandle,
    s: &str,
    string_tydesc: *const rtdt::TyDesc,
) -> rtdt::String {
    unsafe {
        let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            string_tydesc,
        );
        let mut string = string.assume_init();

        if !s.is_empty() {
            datalove_rt::c::dtlv_rti_string_push_bytes_local(
                rt,
                &mut string as *mut rtdt::String as *mut u8,
                string_tydesc,
                s.as_ptr(),
                (s.len() as u32).into(),
            );
        }

        string
    }
}

proptest! {
    /// Test inserting random u32 key-value pairs.
    #[test]
    fn proptest_insert_u32_u32(entries in prop::collection::vec((any::<u32>(), any::<u32>()), 0..1000)) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        // Create the map.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_create_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
            )
        };
        prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        // Track unique keys for expected length.
        let mut unique_keys = std::collections::HashSet::new();

        // Insert all entries.
        for (key, value) in &entries {
            unique_keys.insert(*key);

            let mut key_val = *key;
            let mut value_val = *value;

            let status = unsafe {
                datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc as *const rtdt::TyDesc,
                    &mut key_val as *mut u32 as *mut u8,
                    &*_key_tydesc as *const rtdt::TyDesc,
                    &mut value_val as *mut u32 as *mut u8,
                    &*_value_tydesc as *const rtdt::TyDesc,
                )
            };
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }

        // Verify map length matches unique keys.
        prop_assert_eq!(map.len, rtdt::Usize(unique_keys.len() as u32));

        // Clean up.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_destroy_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
            )
        };
        prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
        prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    /// Test inserting in different orders produces same length.
    #[test]
    fn proptest_insert_order_independence(
        mut entries in prop::collection::vec((any::<u32>(), any::<u32>()), 1..1000)
    ) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

        // Insert in original order.
        let mut map1 = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map1_ptr = &mut map1 as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map1_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            for (key, value) in &entries {
                let mut key_val = *key;
                let mut value_val = *value;
                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map1_ptr,
                    &*map_tydesc,
                    &mut key_val as *mut u32 as *mut u8,
                    &*_key_tydesc,
                    &mut value_val as *mut u32 as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            }
        }

        // Insert in reversed order.
        entries.reverse();
        let mut map2 = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map2_ptr = &mut map2 as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map2_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            for (key, value) in &entries {
                let mut key_val = *key;
                let mut value_val = *value;
                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map2_ptr,
                    &*map_tydesc,
                    &mut key_val as *mut u32 as *mut u8,
                    &*_key_tydesc,
                    &mut value_val as *mut u32 as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            }
        }

        // Both maps should have the same length.
        prop_assert_eq!(map1.len, map2.len);

        // Clean up.
        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map1_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map2_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    /// Test that updates don't change the length.
    #[test]
    fn proptest_update_preserves_length(
        key in any::<u32>(),
        values in prop::collection::vec(any::<u32>(), 1..1000)
    ) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            // Insert with first value.
            let mut key_val = key;
            let mut value_val = values[0];
            let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc,
                &mut key_val as *mut u32 as *mut u8,
                &*_key_tydesc,
                &mut value_val as *mut u32 as *mut u8,
                &*_value_tydesc,
            );
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            prop_assert_eq!(map.len, rtdt::Usize(1));

            // Update with remaining values.
            for value in &values[1..] {
                let mut key_val = key;
                let mut value_val = *value;
                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    &mut key_val as *mut u32 as *mut u8,
                    &*_key_tydesc,
                    &mut value_val as *mut u32 as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

                // Length should remain 1.
                prop_assert_eq!(map.len, rtdt::Usize(1));
            }

            // Clean up.
            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    /// Test inserting many elements to stress the tree structure.
    #[test]
    fn proptest_insert_many(num_entries in 0usize..1000) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            // Insert sequential keys.
            for i in 0..num_entries {
                let mut key_val = i as u32;
                let mut value_val = i as u32 * 100;
                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    &mut key_val as *mut u32 as *mut u8,
                    &*_key_tydesc,
                    &mut value_val as *mut u32 as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            }

            prop_assert_eq!(map.len, rtdt::Usize(num_entries as u32));

            // Clean up.
            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    /// Test clear operation resets length to zero.
    #[test]
    fn proptest_clear_resets_length(entries in prop::collection::vec((any::<u32>(), any::<u32>()), 1..1000)) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            // Insert all entries.
            for (key, value) in &entries {
                let mut key_val = *key;
                let mut value_val = *value;
                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    &mut key_val as *mut u32 as *mut u8,
                    &*_key_tydesc,
                    &mut value_val as *mut u32 as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            }

            let len_before_clear = map.len;
            prop_assert!(len_before_clear > rtdt::Usize(0));

            // Clear the map.
            let status = datalove_rt::c::dtlv_rti_btreemap_clear_local(
                rt,
                map_ptr,
                &*map_tydesc,
            );
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            // Length should be zero.
            prop_assert_eq!(map.len, rtdt::Usize(0));
            prop_assert!(map.root.is_null());

            // Clean up.
            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    /// Test alternating inserts and clears.
    #[test]
    fn proptest_insert_clear_cycles(
        operations in prop::collection::vec((prop::bool::ANY, any::<u32>(), any::<u32>()), 0..50)
    ) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            for (is_insert, key, value) in operations {
                if is_insert {
                    let mut key_val = key;
                    let mut value_val = value;
                    let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                        rt,
                        map_ptr,
                        &*map_tydesc,
                        &mut key_val as *mut u32 as *mut u8,
                        &*_key_tydesc,
                        &mut value_val as *mut u32 as *mut u8,
                        &*_value_tydesc,
                    );
                    prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
                } else {
                    let status = datalove_rt::c::dtlv_rti_btreemap_clear_local(
                        rt,
                        map_ptr,
                        &*map_tydesc,
                    );
                    prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
                    prop_assert_eq!(map.len, rtdt::Usize(0));
                }
            }

            // Clean up.
            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    /// Test that duplicate keys result in correct final length.
    #[test]
    fn proptest_duplicate_keys(
        base_key in any::<u32>(),
        num_duplicates in 1usize..20,
        other_entries in prop::collection::vec((any::<u32>(), any::<u32>()), 0..10)
    ) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            // Insert the base key multiple times with different values.
            for i in 0..num_duplicates {
                let mut key_val = base_key;
                let mut value_val = i as u32;
                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    &mut key_val as *mut u32 as *mut u8,
                    &*_key_tydesc,
                    &mut value_val as *mut u32 as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            }

            // Insert other entries.
            for (key, value) in &other_entries {
                let mut key_val = *key;
                let mut value_val = *value;
                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    &mut key_val as *mut u32 as *mut u8,
                    &*_key_tydesc,
                    &mut value_val as *mut u32 as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            }

            // Calculate expected unique keys.
            let mut unique_keys = std::collections::HashSet::new();
            unique_keys.insert(base_key);
            for (key, _) in &other_entries {
                unique_keys.insert(*key);
            }

            prop_assert_eq!(map.len, rtdt::Usize(unique_keys.len() as u32));

            // Clean up.
            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }
}

// ==================== String property tests ====================

proptest! {
    /// Test inserting random String key-value pairs.
    #[test]
    fn proptest_insert_string_string(entries in prop::collection::vec((any::<u32>(), any::<u32>()), 0..200)) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_string_string_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            let mut unique_keys = std::collections::HashSet::new();

            for (key, value) in &entries {
                let key_str = format!("key_{}", key);
                let value_str = format!("value_{}", value);
                unique_keys.insert(key_str.clone());

                let mut key_rt = create_runtime_string(rt, &key_str, &*_key_tydesc);
                let mut value_rt = create_runtime_string(rt, &value_str, &*_value_tydesc);

                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    &mut key_rt as *mut rtdt::String as *mut u8,
                    &*_key_tydesc,
                    &mut value_rt as *mut rtdt::String as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

                // Note: key_rt and value_rt have been moved into the btreemap.
                // We must NOT destroy them here - the btreemap now owns them.
            }

            prop_assert_eq!(map.len, rtdt::Usize(unique_keys.len() as u32));

            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    /// Test inserting Strings in different orders produces same length.
    #[test]
    fn proptest_insert_order_independence_string(
        mut entries in prop::collection::vec((any::<u32>(), any::<u32>()), 1..200)
    ) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_string_string_tydesc();

        let mut map1 = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map1_ptr = &mut map1 as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map1_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            for (key, value) in &entries {
                let key_str = format!("key_{}", key);
                let value_str = format!("value_{}", value);

                let mut key_rt = create_runtime_string(rt, &key_str, &*_key_tydesc);
                let mut value_rt = create_runtime_string(rt, &value_str, &*_value_tydesc);

                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map1_ptr,
                    &*map_tydesc,
                    &mut key_rt as *mut rtdt::String as *mut u8,
                    &*_key_tydesc,
                    &mut value_rt as *mut rtdt::String as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

                // Note: key_rt and value_rt have been moved into the btreemap.
                // We must NOT destroy them here - the btreemap now owns them.
            }
        }

        entries.reverse();
        let mut map2 = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map2_ptr = &mut map2 as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map2_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            for (key, value) in &entries {
                let key_str = format!("key_{}", key);
                let value_str = format!("value_{}", value);

                let mut key_rt = create_runtime_string(rt, &key_str, &*_key_tydesc);
                let mut value_rt = create_runtime_string(rt, &value_str, &*_value_tydesc);

                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map2_ptr,
                    &*map_tydesc,
                    &mut key_rt as *mut rtdt::String as *mut u8,
                    &*_key_tydesc,
                    &mut value_rt as *mut rtdt::String as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

                // Note: key_rt and value_rt have been moved into the btreemap.
                // We must NOT destroy them here - the btreemap now owns them.
            }
        }

        prop_assert_eq!(map1.len, map2.len);

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map1_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map2_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    /// Test that String updates don't change the length.
    #[test]
    fn proptest_update_preserves_length_string(
        key_num in any::<u32>(),
        values in prop::collection::vec(any::<u32>(), 1..100)
    ) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_string_string_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            let key_str = format!("key_{}", key_num);

            for value in &values {
                let value_str = format!("value_{}", value);

                let mut key_rt = create_runtime_string(rt, &key_str, &*_key_tydesc);
                let mut value_rt = create_runtime_string(rt, &value_str, &*_value_tydesc);

                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    &mut key_rt as *mut rtdt::String as *mut u8,
                    &*_key_tydesc,
                    &mut value_rt as *mut rtdt::String as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
                prop_assert_eq!(map.len, rtdt::Usize(1));

                // Note: key_rt and value_rt have been moved into the btreemap.
                // We must NOT destroy them here - the btreemap now owns them.
            }

            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    /// Test inserting many String elements to stress the tree structure.
    #[test]
    fn proptest_insert_many_string(num_entries in 0usize..200) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_string_string_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            for i in 0..num_entries {
                let key_str = format!("key_{:06}", i);
                let value_str = format!("value_{:06}", i);

                let mut key_rt = create_runtime_string(rt, &key_str, &*_key_tydesc);
                let mut value_rt = create_runtime_string(rt, &value_str, &*_value_tydesc);

                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    &mut key_rt as *mut rtdt::String as *mut u8,
                    &*_key_tydesc,
                    &mut value_rt as *mut rtdt::String as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

                // Note: key_rt and value_rt have been moved into the btreemap.
                // We must NOT destroy them here - the btreemap now owns them.
            }

            prop_assert_eq!(map.len, rtdt::Usize(num_entries as u32));

            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    /// Test clear operation resets String map length to zero.
    #[test]
    fn proptest_clear_resets_length_string(entries in prop::collection::vec((any::<u32>(), any::<u32>()), 1..200)) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_string_string_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            for (key, value) in &entries {
                let key_str = format!("key_{}", key);
                let value_str = format!("value_{}", value);

                let mut key_rt = create_runtime_string(rt, &key_str, &*_key_tydesc);
                let mut value_rt = create_runtime_string(rt, &value_str, &*_value_tydesc);

                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    &mut key_rt as *mut rtdt::String as *mut u8,
                    &*_key_tydesc,
                    &mut value_rt as *mut rtdt::String as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

                // Note: key_rt and value_rt have been moved into the btreemap.
                // We must NOT destroy them here - the btreemap now owns them.
            }

            let len_before_clear = map.len;
            prop_assert!(len_before_clear > rtdt::Usize(0));

            let status = datalove_rt::c::dtlv_rti_btreemap_clear_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            prop_assert_eq!(map.len, rtdt::Usize(0));
            prop_assert!(map.root.is_null());

            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    /// Test alternating String inserts and clears.
    #[test]
    fn proptest_insert_clear_cycles_string(
        operations in prop::collection::vec((prop::bool::ANY, any::<u32>(), any::<u32>()), 0..30)
    ) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_string_string_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            for (is_insert, key, value) in operations {
                if is_insert {
                    let key_str = format!("key_{}", key);
                    let value_str = format!("value_{}", value);

                    let mut key_rt = create_runtime_string(rt, &key_str, &*_key_tydesc);
                    let mut value_rt = create_runtime_string(rt, &value_str, &*_value_tydesc);

                    let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                        rt,
                        map_ptr,
                        &*map_tydesc,
                        &mut key_rt as *mut rtdt::String as *mut u8,
                        &*_key_tydesc,
                        &mut value_rt as *mut rtdt::String as *mut u8,
                        &*_value_tydesc,
                    );
                    prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

                    // Note: key_rt and value_rt have been moved into the btreemap.
                    // We must NOT destroy them here - the btreemap now owns them.
                } else {
                    let status = datalove_rt::c::dtlv_rti_btreemap_clear_local(rt, map_ptr, &*map_tydesc);
                    prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
                    prop_assert_eq!(map.len, rtdt::Usize(0));
                }
            }

            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    /// Test that duplicate String keys result in correct final length.
    #[test]
    fn proptest_duplicate_keys_string(
        base_key in any::<u32>(),
        num_duplicates in 1usize..20,
        other_entries in prop::collection::vec((any::<u32>(), any::<u32>()), 0..10)
    ) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_string_string_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            let base_key_str = format!("key_{}", base_key);

            for i in 0..num_duplicates {
                let value_str = format!("value_{}", i);

                let mut key_rt = create_runtime_string(rt, &base_key_str, &*_key_tydesc);
                let mut value_rt = create_runtime_string(rt, &value_str, &*_value_tydesc);

                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    &mut key_rt as *mut rtdt::String as *mut u8,
                    &*_key_tydesc,
                    &mut value_rt as *mut rtdt::String as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

                // Note: key_rt and value_rt have been moved into the btreemap.
                // We must NOT destroy them here - the btreemap now owns them.
            }

            for (key, value) in &other_entries {
                let key_str = format!("key_{}", key);
                let value_str = format!("value_{}", value);

                let mut key_rt = create_runtime_string(rt, &key_str, &*_key_tydesc);
                let mut value_rt = create_runtime_string(rt, &value_str, &*_value_tydesc);

                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    &mut key_rt as *mut rtdt::String as *mut u8,
                    &*_key_tydesc,
                    &mut value_rt as *mut rtdt::String as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

                // Note: key_rt and value_rt have been moved into the btreemap.
                // We must NOT destroy them here - the btreemap now owns them.
            }

            let mut unique_keys = std::collections::HashSet::new();
            unique_keys.insert(base_key_str);
            for (key, _) in &other_entries {
                unique_keys.insert(format!("key_{}", key));
            }

            prop_assert_eq!(map.len, rtdt::Usize(unique_keys.len() as u32));

            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }
}

// ==================== btreemap_get property tests ====================

/// Create an Option<u32> type descriptor.
fn create_option_u32_tydesc() -> (Box<rtdt::TyDesc>, Box<rtdt::TyDesc>) {
    let inner_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::U32,
        size: 4,
        align: 4,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing,
        },
    });

    // Create the option tydesc first with placeholder size/align.
    let mut option_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Option,
        size: 0,
        align: 0,
        type_info: rtdt::TyInfo {
            option: rtdt::TyInfoOption {
                inner_tydesc: &*inner_tydesc as *const rtdt::TyDesc,
            },
        },
    });

    // Now compute the layout.
    let option_layout = rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ref(&option_tydesc));

    // Update size and align.
    option_tydesc.size = option_layout.size;
    option_tydesc.align = option_layout.align;

    (option_tydesc, inner_tydesc)
}

proptest! {
    /// Test that get returns Some for all inserted keys.
    #[test]
    fn proptest_get_inserted_keys(entries in prop::collection::vec((any::<u32>(), any::<u32>()), 1..200)) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();
        let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            // Track the last value for each key.
            let mut expected_values = std::collections::HashMap::new();

            // Insert all entries.
            for (key, value) in &entries {
                expected_values.insert(*key, *value);

                let mut key_val = *key;
                let mut value_val = *value;

                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    &mut key_val as *mut u32 as *mut u8,
                    &*_key_tydesc,
                    &mut value_val as *mut u32 as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            }

            // Get each unique key and verify we get Some with correct value.
            let option_layout = rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(&*option_tydesc as *const rtdt::TyDesc));

            for (key, expected_value) in &expected_values {
                let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);

                let status = datalove_rt::c::dtlv_rti_btreemap_get_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    key as *const u32 as *const u8,
                    &*_key_tydesc,
                    option_buffer.as_mut_ptr(),
                    &*option_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

                // Check we got Some.
                let tag = unsafe { *option_buffer.as_ptr() };
                prop_assert_eq!(tag, rtdt::OptionTag::Some as u8);

                // Check the value matches.
                let value_ptr = option_buffer.as_ptr().add(option_layout.payload_offset as usize) as *const u32;
                let retrieved_value = *value_ptr;
                prop_assert_eq!(retrieved_value, *expected_value);
            }

            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    /// Test that get returns None for keys not inserted.
    #[test]
    fn proptest_get_nonexistent_keys(
        inserted_keys in prop::collection::vec(any::<u32>(), 1..100),
        query_keys in prop::collection::vec(any::<u32>(), 1..20)
    ) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();
        let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            // Insert keys.
            let inserted_set: std::collections::HashSet<u32> = inserted_keys.iter().copied().collect();

            for key in &inserted_keys {
                let mut key_val = *key;
                let mut value_val = key.wrapping_mul(10);

                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    &mut key_val as *mut u32 as *mut u8,
                    &*_key_tydesc,
                    &mut value_val as *mut u32 as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            }

            // Query keys and check if they should exist.
            let option_layout = rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(&*option_tydesc as *const rtdt::TyDesc));

            for key in &query_keys {
                let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);

                let status = datalove_rt::c::dtlv_rti_btreemap_get_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    key as *const u32 as *const u8,
                    &*_key_tydesc,
                    option_buffer.as_mut_ptr(),
                    &*option_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

                let tag = unsafe { *option_buffer.as_ptr() };

                if inserted_set.contains(key) {
                    // Key exists, should get Some.
                    prop_assert_eq!(tag, rtdt::OptionTag::Some as u8);
                } else {
                    // Key doesn't exist, should get None.
                    prop_assert_eq!(tag, rtdt::OptionTag::None as u8);
                }
            }

            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    /// Test that get retrieves the last value for updated keys.
    #[test]
    fn proptest_get_after_updates(
        key in any::<u32>(),
        values in prop::collection::vec(any::<u32>(), 1..50)
    ) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();
        let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            let option_layout = rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(&*option_tydesc as *const rtdt::TyDesc));

            // Insert and update the same key multiple times.
            for (i, value) in values.iter().enumerate() {
                let mut key_val = key;
                let mut value_val = *value;

                let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    &mut key_val as *mut u32 as *mut u8,
                    &*_key_tydesc,
                    &mut value_val as *mut u32 as *mut u8,
                    &*_value_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

                // After each update, verify we get the current value.
                let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);

                let status = datalove_rt::c::dtlv_rti_btreemap_get_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    &key as *const u32 as *const u8,
                    &*_key_tydesc,
                    option_buffer.as_mut_ptr(),
                    &*option_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

                let tag = unsafe { *option_buffer.as_ptr() };
                prop_assert_eq!(tag, rtdt::OptionTag::Some as u8);

                let value_ptr = option_buffer.as_ptr().add(option_layout.payload_offset as usize) as *const u32;
                let retrieved_value = *value_ptr;
                prop_assert_eq!(retrieved_value, *value, "Mismatch after update {}", i);
            }

            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    /// Test get with random operations (inserts and gets).
    #[test]
    fn proptest_get_mixed_operations(
        operations in prop::collection::vec((prop::bool::ANY, any::<u32>(), any::<u32>()), 1..100)
    ) {
        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();
        let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: rtdt::Usize(0),
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::c::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            let option_layout = rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(&*option_tydesc as *const rtdt::TyDesc));
            let mut expected_values: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();

            for (is_insert, key, value) in operations {
                if is_insert {
                    // Insert/update operation.
                    expected_values.insert(key, value);

                    let mut key_val = key;
                    let mut value_val = value;

                    let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                        rt,
                        map_ptr,
                        &*map_tydesc,
                        &mut key_val as *mut u32 as *mut u8,
                        &*_key_tydesc,
                        &mut value_val as *mut u32 as *mut u8,
                        &*_value_tydesc,
                    );
                    prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
                } else {
                    // Get operation.
                    let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);

                    let status = datalove_rt::c::dtlv_rti_btreemap_get_local(
                        rt,
                        map_ptr,
                        &*map_tydesc,
                        &key as *const u32 as *const u8,
                        &*_key_tydesc,
                        option_buffer.as_mut_ptr(),
                        &*option_tydesc,
                    );
                    prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

                    let tag = unsafe { *option_buffer.as_ptr() };

                    if let Some(expected_value) = expected_values.get(&key) {
                        // Key should exist.
                        prop_assert_eq!(tag, rtdt::OptionTag::Some as u8);

                        let value_ptr = option_buffer.as_ptr().add(option_layout.payload_offset as usize) as *const u32;
                        let retrieved_value = *value_ptr;
                        prop_assert_eq!(retrieved_value, *expected_value);
                    } else {
                        // Key shouldn't exist.
                        prop_assert_eq!(tag, rtdt::OptionTag::None as u8);
                    }
                }
            }

            let status = datalove_rt::c::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
            let status = datalove_rt::c::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }
}
