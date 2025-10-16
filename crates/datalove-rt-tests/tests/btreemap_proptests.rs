//! Property-based tests for btreemap runtime functions.

use rmx::prelude::*;
use datalove_rt::rtdt;
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

/// Create a tuple (u32, u32) type descriptor for map entry.
fn create_tuple_u32_u32_tydesc() -> (Box<rtdt::TyDesc>, Box<rtdt::TyDesc>, Box<rtdt::TyDesc>) {
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

    // Create tuple field array.
    let fields = vec![
        rtdt::TyInfoTupleField {
            offset: 0,
            tydesc: &*key_tydesc as *const rtdt::TyDesc,
        },
        rtdt::TyInfoTupleField {
            offset: 4,
            tydesc: &*value_tydesc as *const rtdt::TyDesc,
        },
    ].into_boxed_slice();

    let fields_ptr = fields.as_ptr();
    std::mem::forget(fields);

    let tuple_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Tuple,
        size: 8,
        align: 4,
        type_info: rtdt::TyInfo {
            tuple: rtdt::TyInfoTuple {
                num_fields: 2,
                fields: fields_ptr,
            },
        },
    });

    (tuple_tydesc, key_tydesc, value_tydesc)
}

proptest! {
    /// Test inserting random u32 key-value pairs.
    #[test]
    fn proptest_insert_u32_u32(entries in prop::collection::vec((any::<u32>(), any::<u32>()), 0..1000)) {
        let rt = datalove_rt::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();
        let (tuple_tydesc, _tuple_key_tydesc, _tuple_value_tydesc) = create_tuple_u32_u32_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: 0,
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        // Create the map.
        let status = unsafe {
            datalove_rt::dtlv_rti_btreemap_create_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
            )
        };
        prop_assert_eq!(status, datalove_rt::RtStatus::Ok);

        // Track unique keys for expected length.
        let mut unique_keys = std::collections::HashSet::new();

        // Insert all entries.
        for (key, value) in &entries {
            unique_keys.insert(*key);

            let mut entry = (*key, *value);
            let entry_ptr = &mut entry as *mut (u32, u32) as *mut u8;

            let status = unsafe {
                datalove_rt::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc as *const rtdt::TyDesc,
                    entry_ptr,
                    &*tuple_tydesc as *const rtdt::TyDesc,
                )
            };
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
        }

        // Verify map length matches unique keys.
        prop_assert_eq!(map.len, unique_keys.len() as u32);

        // Clean up.
        let status = unsafe {
            datalove_rt::dtlv_rti_btreemap_destroy_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
            )
        };
        prop_assert_eq!(status, datalove_rt::RtStatus::Ok);

        let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
        prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
    }

    /// Test inserting in different orders produces same length.
    #[test]
    fn proptest_insert_order_independence(
        mut entries in prop::collection::vec((any::<u32>(), any::<u32>()), 1..1000)
    ) {
        let rt = datalove_rt::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();
        let (tuple_tydesc, _tuple_key_tydesc, _tuple_value_tydesc) = create_tuple_u32_u32_tydesc();

        // Insert in original order.
        let mut map1 = rtdt::Map {
            root: ptr::null(),
            len: 0,
        };
        let map1_ptr = &mut map1 as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::dtlv_rti_btreemap_create_local(rt, map1_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);

            for (key, value) in &entries {
                let mut entry = (*key, *value);
                let entry_ptr = &mut entry as *mut (u32, u32) as *mut u8;
                let status = datalove_rt::dtlv_rti_btreemap_insert_local(
                    rt,
                    map1_ptr,
                    &*map_tydesc,
                    entry_ptr,
                    &*tuple_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
            }
        }

        // Insert in reversed order.
        entries.reverse();
        let mut map2 = rtdt::Map {
            root: ptr::null(),
            len: 0,
        };
        let map2_ptr = &mut map2 as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::dtlv_rti_btreemap_create_local(rt, map2_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);

            for (key, value) in &entries {
                let mut entry = (*key, *value);
                let entry_ptr = &mut entry as *mut (u32, u32) as *mut u8;
                let status = datalove_rt::dtlv_rti_btreemap_insert_local(
                    rt,
                    map2_ptr,
                    &*map_tydesc,
                    entry_ptr,
                    &*tuple_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
            }
        }

        // Both maps should have the same length.
        prop_assert_eq!(map1.len, map2.len);

        // Clean up.
        unsafe {
            let status = datalove_rt::dtlv_rti_btreemap_destroy_local(rt, map1_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
            let status = datalove_rt::dtlv_rti_btreemap_destroy_local(rt, map2_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
            let status = datalove_rt::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
        }
    }

    /// Test that updates don't change the length.
    #[test]
    fn proptest_update_preserves_length(
        key in any::<u32>(),
        values in prop::collection::vec(any::<u32>(), 1..1000)
    ) {
        let rt = datalove_rt::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();
        let (tuple_tydesc, _tuple_key_tydesc, _tuple_value_tydesc) = create_tuple_u32_u32_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: 0,
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);

            // Insert with first value.
            let mut entry = (key, values[0]);
            let entry_ptr = &mut entry as *mut (u32, u32) as *mut u8;
            let status = datalove_rt::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc,
                entry_ptr,
                &*tuple_tydesc,
            );
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);

            prop_assert_eq!(map.len, 1);

            // Update with remaining values.
            for value in &values[1..] {
                let mut entry = (key, *value);
                let entry_ptr = &mut entry as *mut (u32, u32) as *mut u8;
                let status = datalove_rt::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    entry_ptr,
                    &*tuple_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::RtStatus::Ok);

                // Length should remain 1.
                prop_assert_eq!(map.len, 1);
            }

            // Clean up.
            let status = datalove_rt::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
            let status = datalove_rt::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
        }
    }

    /// Test inserting many elements to stress the tree structure.
    #[test]
    fn proptest_insert_many(num_entries in 0usize..1000) {
        let rt = datalove_rt::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();
        let (tuple_tydesc, _tuple_key_tydesc, _tuple_value_tydesc) = create_tuple_u32_u32_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: 0,
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);

            // Insert sequential keys.
            for i in 0..num_entries {
                let mut entry = (i as u32, i as u32 * 100);
                let entry_ptr = &mut entry as *mut (u32, u32) as *mut u8;
                let status = datalove_rt::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    entry_ptr,
                    &*tuple_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
            }

            prop_assert_eq!(map.len, num_entries as u32);

            // Clean up.
            let status = datalove_rt::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
            let status = datalove_rt::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
        }
    }

    /// Test clear operation resets length to zero.
    #[test]
    fn proptest_clear_resets_length(entries in prop::collection::vec((any::<u32>(), any::<u32>()), 1..1000)) {
        let rt = datalove_rt::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();
        let (tuple_tydesc, _tuple_key_tydesc, _tuple_value_tydesc) = create_tuple_u32_u32_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: 0,
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);

            // Insert all entries.
            for (key, value) in &entries {
                let mut entry = (*key, *value);
                let entry_ptr = &mut entry as *mut (u32, u32) as *mut u8;
                let status = datalove_rt::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    entry_ptr,
                    &*tuple_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
            }

            let len_before_clear = map.len;
            prop_assert!(len_before_clear > 0);

            // Clear the map.
            let status = datalove_rt::dtlv_rti_btreemap_clear_local(
                rt,
                map_ptr,
                &*map_tydesc,
            );
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);

            // Length should be zero.
            prop_assert_eq!(map.len, 0);
            prop_assert!(map.root.is_null());

            // Clean up.
            let status = datalove_rt::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
            let status = datalove_rt::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
        }
    }

    /// Test alternating inserts and clears.
    #[test]
    fn proptest_insert_clear_cycles(
        operations in prop::collection::vec((prop::bool::ANY, any::<u32>(), any::<u32>()), 0..50)
    ) {
        let rt = datalove_rt::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();
        let (tuple_tydesc, _tuple_key_tydesc, _tuple_value_tydesc) = create_tuple_u32_u32_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: 0,
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);

            for (is_insert, key, value) in operations {
                if is_insert {
                    let mut entry = (key, value);
                    let entry_ptr = &mut entry as *mut (u32, u32) as *mut u8;
                    let status = datalove_rt::dtlv_rti_btreemap_insert_local(
                        rt,
                        map_ptr,
                        &*map_tydesc,
                        entry_ptr,
                        &*tuple_tydesc,
                    );
                    prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
                } else {
                    let status = datalove_rt::dtlv_rti_btreemap_clear_local(
                        rt,
                        map_ptr,
                        &*map_tydesc,
                    );
                    prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
                    prop_assert_eq!(map.len, 0);
                }
            }

            // Clean up.
            let status = datalove_rt::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
            let status = datalove_rt::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
        }
    }

    /// Test that duplicate keys result in correct final length.
    #[test]
    fn proptest_duplicate_keys(
        base_key in any::<u32>(),
        num_duplicates in 1usize..20,
        other_entries in prop::collection::vec((any::<u32>(), any::<u32>()), 0..10)
    ) {
        let rt = datalove_rt::dtlv_rti_init();
        prop_assert!(!rt.is_null());

        let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();
        let (tuple_tydesc, _tuple_key_tydesc, _tuple_value_tydesc) = create_tuple_u32_u32_tydesc();

        let mut map = rtdt::Map {
            root: ptr::null(),
            len: 0,
        };
        let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

        unsafe {
            let status = datalove_rt::dtlv_rti_btreemap_create_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);

            // Insert the base key multiple times with different values.
            for i in 0..num_duplicates {
                let mut entry = (base_key, i as u32);
                let entry_ptr = &mut entry as *mut (u32, u32) as *mut u8;
                let status = datalove_rt::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    entry_ptr,
                    &*tuple_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
            }

            // Insert other entries.
            for (key, value) in &other_entries {
                let mut entry = (*key, *value);
                let entry_ptr = &mut entry as *mut (u32, u32) as *mut u8;
                let status = datalove_rt::dtlv_rti_btreemap_insert_local(
                    rt,
                    map_ptr,
                    &*map_tydesc,
                    entry_ptr,
                    &*tuple_tydesc,
                );
                prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
            }

            // Calculate expected unique keys.
            let mut unique_keys = std::collections::HashSet::new();
            unique_keys.insert(base_key);
            for (key, _) in &other_entries {
                unique_keys.insert(*key);
            }

            prop_assert_eq!(map.len, unique_keys.len() as u32);

            // Clean up.
            let status = datalove_rt::dtlv_rti_btreemap_destroy_local(rt, map_ptr, &*map_tydesc);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
            let status = datalove_rt::dtlv_rti_shutdown(rt);
            prop_assert_eq!(status, datalove_rt::RtStatus::Ok);
        }
    }
}
