//! Deep cloning for runtime values.

use crate::{LocalRtHandle, RtStatus, alloc, rtdt, rt_local};

/// Clone any type into the local heap.
///
/// Space is already allocated for the proximate type
/// at the `value_out` location - we just need to allocate
/// any needed buffers.
pub unsafe fn clone_value(
    rt: LocalRtHandle,
    value_in: *const u8,
    tydesc_in: *const rtdt::TyDesc,
    value_out: *mut u8,
) -> RtStatus {
    assert!(!value_in.is_null());
    assert!(!tydesc_in.is_null());
    assert!(!value_out.is_null());

    unsafe {
        let ty = rtdt::TyDescRef::from_ptr(tydesc_in);
        clone_impl(rt, value_in, ty, value_out)
    }
}

/// Internal clone implementation.
unsafe fn clone_impl(
    rt: LocalRtHandle,
    value_in: *const u8,
    tydesc: rtdt::TyDescRef,
    value_out: *mut u8,
) -> RtStatus {
    use rtdt::TyTag;

    let ty = tydesc;

    match ty.type_tag() {
        // Scalars - just copy bytes.
        TyTag::Bool | TyTag::U8 | TyTag::I8 | TyTag::U16 | TyTag::I16 |
        TyTag::U32 | TyTag::I32 | TyTag::U64 | TyTag::I64 |
        TyTag::F32 | TyTag::F64 => {
            unsafe {
                std::ptr::copy_nonoverlapping(value_in, value_out, ty.size() as usize);
            }
            RtStatus::Ok
        }

        // Bigint - allocate new buffer and copy limbs.
        TyTag::Int => {
            let int_in = unsafe { &*(value_in as *const rtdt::Int) };
            let int_out = unsafe { &mut *(value_out as *mut rtdt::Int) };

            let num_limbs = int_in.size_and_sign.abs() as u32;

            if num_limbs == 0 || int_in.data.is_null() {
                // Zero or empty.
                int_out.data = std::ptr::null();
                int_out.size_and_sign = 0;
                int_out.capacity = 0;
            } else {
                // Allocate new limb buffer.
                let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };
                let limb_size = std::mem::size_of::<u32>() as u32;
                let limb_align = std::mem::align_of::<u32>() as u32;
                let new_data = unsafe { rt_ref.alloc.alloc(limb_size, limb_align, num_limbs) as *mut u32 };

                // Copy limbs.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        int_in.data,
                        new_data,
                        num_limbs as usize
                    );
                }

                int_out.data = new_data;
                int_out.size_and_sign = int_in.size_and_sign;
                int_out.capacity = num_limbs;
            }

            RtStatus::Ok
        }

        // String - allocate new buffer and copy bytes.
        TyTag::String => {
            let str_in = unsafe { &*(value_in as *const rtdt::String) };
            let str_out = unsafe { &mut *(value_out as *mut rtdt::String) };

            if str_in.size == 0 || str_in.data.is_null() {
                // Empty string.
                str_out.data = std::ptr::null();
                str_out.size = 0;
                str_out.capacity = 0;
            } else {
                // Allocate new string buffer.
                let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };
                let new_data = unsafe { rt_ref.alloc.alloc(1, 1, str_in.size) };

                // Copy bytes.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        str_in.data,
                        new_data,
                        str_in.size as usize
                    );
                }

                str_out.data = new_data;
                str_out.size = str_in.size;
                str_out.capacity = str_in.size;
            }

            RtStatus::Ok
        }

        // Tuple - recursively clone each field.
        TyTag::Tuple => {
            for field in ty.iter_tuple_fields() {
                let field_in = unsafe { value_in.add(field.offset() as usize) };
                let field_out = unsafe { value_out.add(field.offset() as usize) };

                let status = unsafe {
                    clone_impl(rt, field_in, field.tydesc(), field_out)
                };

                if status != RtStatus::Ok {
                    return status;
                }
            }

            RtStatus::Ok
        }

        // Struct - recursively clone each field.
        TyTag::Struct => {
            for field in ty.iter_struct_fields() {
                let field_in = unsafe { value_in.add(field.offset() as usize) };
                let field_out = unsafe { value_out.add(field.offset() as usize) };

                let status = unsafe {
                    clone_impl(rt, field_in, field.tydesc(), field_out)
                };

                if status != RtStatus::Ok {
                    return status;
                }
            }

            RtStatus::Ok
        }

        // Enum - copy discriminant and clone payload if present.
        TyTag::Enum => {
            let enum_info = ty.enum_info();
            let discriminant = unsafe { *(value_in as *const u32) };

            // Copy discriminant.
            unsafe {
                *(value_out as *mut u32) = discriminant;
            }

            // Find variant and clone payload if it exists.
            if (discriminant as usize) < enum_info.num_variants() as usize {
                if let core::option::Option::Some(variant) = enum_info.variant(discriminant as usize) {
                    if let core::option::Option::Some(payload_ty) = variant.payload() {
                        let payload_in = unsafe { value_in.add(variant.offset() as usize) };
                        let payload_out = unsafe { value_out.add(variant.offset() as usize) };

                        return unsafe {
                            clone_impl(rt, payload_in, payload_ty, payload_out)
                        };
                    }
                }
            }

            RtStatus::Ok
        }

        // List - allocate new buffer and recursively clone each element.
        TyTag::List => {
            let list_in = unsafe { &*(value_in as *const rtdt::List) };
            let list_out = unsafe { &mut *(value_out as *mut rtdt::List) };

            let elem_ty = ty.list_element_ty();
            let elem_size = elem_ty.size();

            if list_in.size == 0 || list_in.data.is_null() {
                // Empty list.
                list_out.data = std::ptr::null();
                list_out.size = 0;
                list_out.capacity = 0;
            } else {
                // Allocate new list buffer.
                let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };
                let elem_align = elem_ty.align();
                let new_data = unsafe { rt_ref.alloc.alloc(elem_size, elem_align, list_in.size) };

                // Clone each element.
                for i in 0..list_in.size {
                    let elem_in = unsafe { list_in.data.add((i * elem_size) as usize) };
                    let elem_out = unsafe { new_data.add((i * elem_size) as usize) };

                    let status = unsafe {
                        clone_impl(rt, elem_in, elem_ty, elem_out)
                    };

                    if status != RtStatus::Ok {
                        return status;
                    }
                }

                list_out.data = new_data;
                list_out.size = list_in.size;
                list_out.capacity = list_in.size;
            }

            RtStatus::Ok
        }

        // Map - recursively clone the tree structure.
        TyTag::Map => {
            let map_in = unsafe { &*(value_in as *const rtdt::Map) };
            let map_out = unsafe { &mut *(value_out as *mut rtdt::Map) };

            if map_in.root.is_null() {
                map_out.root = std::ptr::null();
                map_out.len = 0;
            } else {
                let key_ty = ty.map_key_ty();
                let value_ty = ty.map_value_ty();
                let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };

                let new_root = unsafe {
                    crate::btreemap::btreemap_clone_tree(
                        rt_ref,
                        map_in.root,
                        key_ty,
                        value_ty,
                    )
                };

                if new_root.is_null() {
                    return RtStatus::Error;
                }

                map_out.root = new_root;
                map_out.len = map_in.len;
            }

            RtStatus::Ok
        }

        // Set - recursively clone the tree structure.
        TyTag::Set => {
            let set_in = unsafe { &*(value_in as *const rtdt::Set) };
            let set_out = unsafe { &mut *(value_out as *mut rtdt::Set) };

            if set_in.root.is_null() {
                set_out.root = std::ptr::null();
                set_out.len = 0;
            } else {
                let element_ty = ty.set_element_ty();
                let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };

                let new_root = unsafe {
                    crate::set::set_clone_tree(
                        rt_ref,
                        set_in.root,
                        element_ty.as_ptr(),
                    )
                };

                if new_root.is_null() {
                    return RtStatus::Error;
                }

                set_out.root = new_root;
                set_out.len = set_in.len;
            }

            RtStatus::Ok
        }

        // Tensor - clone tensor structure.
        TyTag::Tensor => {
            todo!("tensor cloning not yet implemented")
        }

        // Option - copy tag and clone payload if Some.
        TyTag::Option => {
            let opt_in = unsafe { &*(value_in as *const rtdt::Option) };
            let opt_out = unsafe { &mut *(value_out as *mut rtdt::Option) };

            // Copy tag.
            opt_out.tag = opt_in.tag;

            if opt_in.tag == rtdt::OptionTag::Some {
                let inner_ty = ty.option_inner_ty();
                let payload_offset = rtdt::layout::option_payload_offset(inner_ty.align());

                let payload_in = unsafe { value_in.add(payload_offset as usize) };
                let payload_out = unsafe { value_out.add(payload_offset as usize) };

                return unsafe {
                    clone_impl(rt, payload_in, inner_ty, payload_out)
                };
            }

            RtStatus::Ok
        }

        // Result - copy tag and clone payload.
        TyTag::Result => {
            let res_in = unsafe { &*(value_in as *const rtdt::Result) };
            let res_out = unsafe { &mut *(value_out as *mut rtdt::Result) };

            // Copy tag.
            res_out.tag = res_in.tag;

            let ok_ty = ty.result_ok_ty();
            let payload_offset = rtdt::layout::result_payload_offset(ok_ty.align());

            // For now, we only handle Ok case.
            // Error type cloning would need the error tydesc.
            if res_in.tag == rtdt::ResultTag::Ok {
                let payload_in = unsafe { value_in.add(payload_offset as usize) };
                let payload_out = unsafe { value_out.add(payload_offset as usize) };

                return unsafe {
                    clone_impl(rt, payload_in, ok_ty, payload_out)
                };
            }

            // For Err, we need to know the Error layout.
            // For now, just copy the error bytes.
            let err_size = std::mem::size_of::<rtdt::Error>() as u32;
            unsafe {
                std::ptr::copy_nonoverlapping(
                    value_in.add(payload_offset as usize),
                    value_out.add(payload_offset as usize),
                    err_size as usize
                );
            }

            RtStatus::Ok
        }

        // Data/Error - copy the wrapper.
        TyTag::Data => {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    value_in,
                    value_out,
                    std::mem::size_of::<rtdt::Data>()
                );
            }
            RtStatus::Ok
        }

        TyTag::Error => {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    value_in,
                    value_out,
                    std::mem::size_of::<rtdt::Error>()
                );
            }
            RtStatus::Ok
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Helper to create a test runtime.
    fn make_rt() -> Box<rt_local::RtLocal> {
        rt_local::RtLocal::new()
    }

    /// Helper to create a simple scalar tydesc.
    fn make_u32_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::U32,
            size: 4,
            align: 4,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing,
            },
        }
    }

    fn make_f32_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::F32,
            size: 4,
            align: 4,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing,
            },
        }
    }

    fn make_bool_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::Bool,
            size: 1,
            align: 1,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing,
            },
        }
    }

    fn make_int_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::Int,
            size: std::mem::size_of::<rtdt::Int>() as u32,
            align: std::mem::align_of::<rtdt::Int>() as u32,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing,
            },
        }
    }

    fn make_string_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::String,
            size: std::mem::size_of::<rtdt::String>() as u32,
            align: std::mem::align_of::<rtdt::String>() as u32,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing,
            },
        }
    }

    #[test]
    fn test_clone_u32_zero() {
        let mut rt = make_rt();
        let tydesc = make_u32_tydesc();

        let value_in: u32 = 0;
        let mut value_out: u32 = 0xDEADBEEF;

        let status = unsafe {
            clone_value(
                Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                &value_in as *const _ as *const u8,
                &tydesc,
                &mut value_out as *mut _ as *mut u8,
            )
        };

        assert_eq!(status, RtStatus::Ok);
        assert_eq!(value_out, 0);
    }

    proptest! {
        #[test]
        fn prop_clone_u32(value in any::<u32>()) {
            let mut rt = make_rt();
            let tydesc = make_u32_tydesc();

            let mut value_out: u32 = 0;

            let status = unsafe {
                clone_value(
                    Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                    &value as *const _ as *const u8,
                    &tydesc,
                    &mut value_out as *mut _ as *mut u8,
                )
            };

            prop_assert_eq!(status, RtStatus::Ok);
            prop_assert_eq!(value_out, value);
        }

        #[test]
        fn prop_clone_f32(value in any::<f32>()) {
            let mut rt = make_rt();
            let tydesc = make_f32_tydesc();

            let mut value_out: f32 = 0.0;

            let status = unsafe {
                clone_value(
                    Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                    &value as *const _ as *const u8,
                    &tydesc,
                    &mut value_out as *mut _ as *mut u8,
                )
            };

            prop_assert_eq!(status, RtStatus::Ok);
            if value.is_nan() {
                prop_assert!(value_out.is_nan());
            } else {
                prop_assert_eq!(value_out, value);
            }
        }

        #[test]
        fn prop_clone_bool(value in any::<bool>()) {
            let mut rt = make_rt();
            let tydesc = make_bool_tydesc();

            let value_in: u8 = value as u8;
            let mut value_out: u8 = 0;

            let status = unsafe {
                clone_value(
                    Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                    &value_in as *const _ as *const u8,
                    &tydesc,
                    &mut value_out as *mut _ as *mut u8,
                )
            };

            prop_assert_eq!(status, RtStatus::Ok);
            prop_assert_eq!(value_out, value_in);
        }
    }

    #[test]
    fn test_clone_empty_int() {
        let mut rt = make_rt();
        let tydesc = make_int_tydesc();

        let value_in = rtdt::Int {
            data: std::ptr::null(),
            size_and_sign: 0,
            capacity: 0,
        };

        let mut value_out = rtdt::Int {
            data: std::ptr::null_mut(),
            size_and_sign: 999,
            capacity: 999,
        };

        let status = unsafe {
            clone_value(
                Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                &value_in as *const _ as *const u8,
                &tydesc,
                &mut value_out as *mut _ as *mut u8,
            )
        };

        assert_eq!(status, RtStatus::Ok);
        assert!(value_out.data.is_null());
        assert_eq!(value_out.size_and_sign, 0);
        assert_eq!(value_out.capacity, 0);
    }

    proptest! {
        #[test]
        fn prop_clone_int(
            limbs in prop::collection::vec(any::<u32>(), 0..10),
            is_negative in any::<bool>()
        ) {
            let mut rt = make_rt();
            let tydesc = make_int_tydesc();

            // Allocate limbs.
            let limb_data = if limbs.is_empty() {
                std::ptr::null()
            } else {
                unsafe {
                    let ptr = rt.alloc.alloc(4, 4, limbs.len() as u32) as *mut u32;
                    for (i, &limb) in limbs.iter().enumerate() {
                        *ptr.add(i) = limb;
                    }
                    ptr as *const u32
                }
            };

            let size = limbs.len() as i32;
            let value_in = rtdt::Int {
                data: limb_data,
                size_and_sign: if is_negative { -size } else { size },
                capacity: limbs.len() as u32,
            };

            let mut value_out = rtdt::Int {
                data: std::ptr::null(),
                size_and_sign: 0,
                capacity: 0,
            };

            let status = unsafe {
                clone_value(
                    Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                    &value_in as *const _ as *const u8,
                    &tydesc,
                    &mut value_out as *mut _ as *mut u8,
                )
            };

            prop_assert_eq!(status, RtStatus::Ok);
            prop_assert_eq!(value_out.size_and_sign, value_in.size_and_sign);

            if !limbs.is_empty() {
                // Verify data was copied and pointers are different.
                prop_assert_ne!(value_out.data, value_in.data);

                // Verify limb values are equal.
                for i in 0..limbs.len() {
                    let in_limb = unsafe { *value_in.data.add(i) };
                    let out_limb = unsafe { *value_out.data.add(i) };
                    prop_assert_eq!(in_limb, out_limb);
                }

                // Cleanup.
                unsafe {
                    rt.alloc.free(4, 4, limbs.len() as u32, value_out.data as *mut u8);
                }
            }

            // Cleanup input.
            if !limbs.is_empty() {
                unsafe {
                    rt.alloc.free(4, 4, limbs.len() as u32, limb_data as *mut u8);
                }
            }
        }
    }

    #[test]
    fn test_clone_empty_string() {
        let mut rt = make_rt();
        let tydesc = make_string_tydesc();

        let value_in = rtdt::String {
            data: std::ptr::null(),
            size: 0,
            capacity: 0,
        };

        let mut value_out = rtdt::String {
            data: std::ptr::null(),
            size: 999,
            capacity: 999,
        };

        let status = unsafe {
            clone_value(
                Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                &value_in as *const _ as *const u8,
                &tydesc,
                &mut value_out as *mut _ as *mut u8,
            )
        };

        assert_eq!(status, RtStatus::Ok);
        assert!(value_out.data.is_null());
        assert_eq!(value_out.size, 0);
        assert_eq!(value_out.capacity, 0);
    }

    proptest! {
        #[test]
        fn prop_clone_string(bytes in prop::collection::vec(any::<u8>(), 0..100)) {
            let mut rt = make_rt();
            let tydesc = make_string_tydesc();

            // Allocate string data.
            let str_data = if bytes.is_empty() {
                std::ptr::null()
            } else {
                unsafe {
                    let ptr = rt.alloc.alloc(1, 1, bytes.len() as u32);
                    for (i, &byte) in bytes.iter().enumerate() {
                        *ptr.add(i) = byte;
                    }
                    ptr as *const u8
                }
            };

            let value_in = rtdt::String {
                data: str_data,
                size: bytes.len() as u32,
                capacity: bytes.len() as u32,
            };

            let mut value_out = rtdt::String {
                data: std::ptr::null(),
                size: 0,
                capacity: 0,
            };

            let status = unsafe {
                clone_value(
                    Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                    &value_in as *const _ as *const u8,
                    &tydesc,
                    &mut value_out as *mut _ as *mut u8,
                )
            };

            prop_assert_eq!(status, RtStatus::Ok);
            prop_assert_eq!(value_out.size, value_in.size);

            if !bytes.is_empty() {
                // Verify data was copied and pointers are different.
                prop_assert_ne!(value_out.data, value_in.data);

                // Verify byte values are equal.
                for i in 0..bytes.len() {
                    let in_byte = unsafe { *value_in.data.add(i) };
                    let out_byte = unsafe { *value_out.data.add(i) };
                    prop_assert_eq!(in_byte, out_byte);
                }

                // Cleanup.
                unsafe {
                    rt.alloc.free(1, 1, bytes.len() as u32, value_out.data as *mut u8);
                }
            }

            // Cleanup input.
            if !bytes.is_empty() {
                unsafe {
                    rt.alloc.free(1, 1, bytes.len() as u32, str_data as *mut u8);
                }
            }
        }
    }

    #[test]
    fn test_clone_tuple_u32_u32() {
        let mut rt = make_rt();

        // Build type descriptor for (u32, u32).
        let u32_tydesc = Box::new(make_u32_tydesc());
        let u32_tydesc_ptr = Box::as_ref(&u32_tydesc) as *const rtdt::TyDesc;

        let fields = Box::new([
            rtdt::TyInfoTupleField {
                offset: 0,
                tydesc: u32_tydesc_ptr,
            },
            rtdt::TyInfoTupleField {
                offset: 4,
                tydesc: u32_tydesc_ptr,
            },
        ]);

        let fields_ptr = fields.as_ptr();

        let tuple_tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::Tuple,
            size: 8,
            align: 4,
            type_info: rtdt::TyInfo {
                tuple: rtdt::TyInfoTuple {
                    num_fields: 2,
                    fields: fields_ptr,
                },
            },
        };

        #[repr(C)]
        struct Tuple {
            a: u32,
            b: u32,
        }

        let value_in = Tuple { a: 42, b: 100 };
        let mut value_out = Tuple { a: 0, b: 0 };

        let status = unsafe {
            clone_value(
                Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                &value_in as *const _ as *const u8,
                &tuple_tydesc,
                &mut value_out as *mut _ as *mut u8,
            )
        };

        assert_eq!(status, RtStatus::Ok);
        assert_eq!(value_out.a, 42);
        assert_eq!(value_out.b, 100);
    }

    #[test]
    fn test_clone_empty_list() {
        let mut rt = make_rt();

        // Build type descriptor for [u32].
        let u32_tydesc = Box::new(make_u32_tydesc());
        let u32_tydesc_ptr = Box::as_ref(&u32_tydesc) as *const rtdt::TyDesc;

        let list_tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::List,
            size: std::mem::size_of::<rtdt::List>() as u32,
            align: std::mem::align_of::<rtdt::List>() as u32,
            type_info: rtdt::TyInfo {
                list: rtdt::TyInfoList {
                    element_tydesc: u32_tydesc_ptr,
                },
            },
        };

        let value_in = rtdt::List {
            data: std::ptr::null(),
            size: 0,
            capacity: 0,
        };

        let mut value_out = rtdt::List {
            data: std::ptr::null(),
            size: 999,
            capacity: 999,
        };

        let status = unsafe {
            clone_value(
                Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                &value_in as *const _ as *const u8,
                &list_tydesc,
                &mut value_out as *mut _ as *mut u8,
            )
        };

        assert_eq!(status, RtStatus::Ok);
        assert!(value_out.data.is_null());
        assert_eq!(value_out.size, 0);
        assert_eq!(value_out.capacity, 0);
    }

    proptest! {
        #[test]
        fn prop_clone_list_u32(elements in prop::collection::vec(any::<u32>(), 0..20)) {
            let mut rt = make_rt();

            // Build type descriptor for [u32].
            let u32_tydesc = Box::new(make_u32_tydesc());
            let u32_tydesc_ptr = Box::as_ref(&u32_tydesc) as *const rtdt::TyDesc;

            let list_tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::List,
                size: std::mem::size_of::<rtdt::List>() as u32,
                align: std::mem::align_of::<rtdt::List>() as u32,
                type_info: rtdt::TyInfo {
                    list: rtdt::TyInfoList {
                        element_tydesc: u32_tydesc_ptr,
                    },
                },
            };

            // Allocate list data.
            let list_data = if elements.is_empty() {
                std::ptr::null()
            } else {
                unsafe {
                    let ptr = rt.alloc.alloc(4, 4, elements.len() as u32) as *mut u32;
                    for (i, &elem) in elements.iter().enumerate() {
                        *ptr.add(i) = elem;
                    }
                    ptr as *const u8
                }
            };

            let value_in = rtdt::List {
                data: list_data,
                size: elements.len() as u32,
                capacity: elements.len() as u32,
            };

            let mut value_out = rtdt::List {
                data: std::ptr::null(),
                size: 0,
                capacity: 0,
            };

            let status = unsafe {
                clone_value(
                    Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                    &value_in as *const _ as *const u8,
                    &list_tydesc,
                    &mut value_out as *mut _ as *mut u8,
                )
            };

            prop_assert_eq!(status, RtStatus::Ok);
            prop_assert_eq!(value_out.size, value_in.size);

            if !elements.is_empty() {
                // Verify data was copied and pointers are different.
                prop_assert_ne!(value_out.data, value_in.data);

                // Verify element values are equal.
                for i in 0..elements.len() {
                    let in_elem = unsafe { *(value_in.data as *const u32).add(i) };
                    let out_elem = unsafe { *(value_out.data as *const u32).add(i) };
                    prop_assert_eq!(in_elem, out_elem);
                }

                // Cleanup.
                unsafe {
                    rt.alloc.free(4, 4, elements.len() as u32, value_out.data as *mut u8);
                }
            }

            // Cleanup input.
            if !elements.is_empty() {
                unsafe {
                    rt.alloc.free(4, 4, elements.len() as u32, list_data as *mut u8);
                }
            }
        }
    }

    #[test]
    fn test_clone_option_none() {
        let mut rt = make_rt();

        // Build type descriptor for ?u32.
        let u32_tydesc = Box::new(make_u32_tydesc());
        let u32_tydesc_ptr = Box::as_ref(&u32_tydesc) as *const rtdt::TyDesc;

        let option_tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::Option,
            size: 8,
            align: 4,
            type_info: rtdt::TyInfo {
                option: rtdt::TyInfoOption {
                    inner_tydesc: u32_tydesc_ptr,
                },
            },
        };

        #[repr(C)]
        struct OptionU32 {
            tag: rtdt::OptionTag,
            _pad: [u8; 3],
            value: u32,
        }

        let value_in = OptionU32 {
            tag: rtdt::OptionTag::None,
            _pad: [0; 3],
            value: 0,
        };

        let mut value_out = OptionU32 {
            tag: rtdt::OptionTag::Some,
            _pad: [0xFF; 3],
            value: 0xDEADBEEF,
        };

        let status = unsafe {
            clone_value(
                Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                &value_in as *const _ as *const u8,
                &option_tydesc,
                &mut value_out as *mut _ as *mut u8,
            )
        };

        assert_eq!(status, RtStatus::Ok);
        assert_eq!(value_out.tag, rtdt::OptionTag::None);
    }

    proptest! {
        #[test]
        fn prop_clone_option_some(value in any::<u32>()) {
            let mut rt = make_rt();

            // Build type descriptor for ?u32.
            let u32_tydesc = Box::new(make_u32_tydesc());
            let u32_tydesc_ptr = Box::as_ref(&u32_tydesc) as *const rtdt::TyDesc;

            let option_tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::Option,
                size: 8,
                align: 4,
                type_info: rtdt::TyInfo {
                    option: rtdt::TyInfoOption {
                        inner_tydesc: u32_tydesc_ptr,
                    },
                },
            };

            #[repr(C)]
            struct OptionU32 {
                tag: rtdt::OptionTag,
                _pad: [u8; 3],
                value: u32,
            }

            let value_in = OptionU32 {
                tag: rtdt::OptionTag::Some,
                _pad: [0; 3],
                value,
            };

            let mut value_out = OptionU32 {
                tag: rtdt::OptionTag::None,
                _pad: [0; 3],
                value: 0,
            };

            let status = unsafe {
                clone_value(
                    Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                    &value_in as *const _ as *const u8,
                    &option_tydesc,
                    &mut value_out as *mut _ as *mut u8,
                )
            };

            prop_assert_eq!(status, RtStatus::Ok);
            prop_assert_eq!(value_out.tag, rtdt::OptionTag::Some);
            prop_assert_eq!(value_out.value, value);
        }
    }

    #[test]
    fn test_clone_empty_map() {
        let mut rt = make_rt();

        // Build type descriptor for {u32: u32}.
        let u32_tydesc = Box::new(make_u32_tydesc());
        let u32_tydesc_ptr = Box::as_ref(&u32_tydesc) as *const rtdt::TyDesc;

        let map_tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::Map,
            size: std::mem::size_of::<rtdt::Map>() as u32,
            align: std::mem::align_of::<rtdt::Map>() as u32,
            type_info: rtdt::TyInfo {
                map: rtdt::TyInfoMap {
                    key_tydesc: u32_tydesc_ptr,
                    value_tydesc: u32_tydesc_ptr,
                },
            },
        };

        let value_in = rtdt::Map {
            root: std::ptr::null(),
            len: 0,
        };

        let mut value_out = rtdt::Map {
            root: std::ptr::null(),
            len: 999,
        };

        let status = unsafe {
            clone_value(
                Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                &value_in as *const _ as *const u8,
                &map_tydesc,
                &mut value_out as *mut _ as *mut u8,
            )
        };

        assert_eq!(status, RtStatus::Ok);
        assert!(value_out.root.is_null());
        assert_eq!(value_out.len, 0);
    }

    #[test]
    fn test_clone_empty_set() {
        let mut rt = make_rt();

        // Build type descriptor for {u32}.
        let u32_tydesc = Box::new(make_u32_tydesc());
        let u32_tydesc_ptr = Box::as_ref(&u32_tydesc) as *const rtdt::TyDesc;

        let set_tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::Set,
            size: std::mem::size_of::<rtdt::Set>() as u32,
            align: std::mem::align_of::<rtdt::Set>() as u32,
            type_info: rtdt::TyInfo {
                set: rtdt::TyInfoSet {
                    element_tydesc: u32_tydesc_ptr,
                },
            },
        };

        let value_in = rtdt::Set {
            root: std::ptr::null(),
            len: 0,
        };

        let mut value_out = rtdt::Set {
            root: std::ptr::null(),
            len: 999,
        };

        let status = unsafe {
            clone_value(
                Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                &value_in as *const _ as *const u8,
                &set_tydesc,
                &mut value_out as *mut _ as *mut u8,
            )
        };

        assert_eq!(status, RtStatus::Ok);
        assert!(value_out.root.is_null());
        assert_eq!(value_out.len, 0);
    }
}
