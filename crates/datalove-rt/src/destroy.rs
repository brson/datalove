//! Destructor implementation for all Datalove types.

use rmx::prelude::*;
use crate::alloc::LocalRt;
use crate::rtdt;
use crate::{LocalRtHandle, RtStatus};

/// Destroys any type of value, freeing allocations recursively.
pub unsafe fn any_destroy_local(
    rt: LocalRtHandle,
    value_in: *mut u8,
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    if rt.is_null() || value_in.is_null() || tydesc.is_null() {
        return RtStatus::Error;
    }

    unsafe {
        let ty = &*tydesc;
        let rt_ref = &mut *(rt as *mut LocalRt);

        match ty.type_tag {
            // Primitives - no allocations to free.
            rtdt::TyTag::Bool
            | rtdt::TyTag::U8
            | rtdt::TyTag::I8
            | rtdt::TyTag::U16
            | rtdt::TyTag::I16
            | rtdt::TyTag::U32
            | rtdt::TyTag::I32
            | rtdt::TyTag::U64
            | rtdt::TyTag::I64
            | rtdt::TyTag::F32
            | rtdt::TyTag::F64 => RtStatus::Ok,

            // Int has allocations.
            rtdt::TyTag::Int => {
                todo!("Int destructor")
            }

            // String has allocations.
            rtdt::TyTag::String => {
                crate::string::string_destroy_local(rt, value_in, tydesc)
            }

            // Map has allocations.
            rtdt::TyTag::Map => {
                crate::btreemap::btreemap_destroy_impl(rt_ref, value_in, tydesc)
            }

            // List has allocations.
            rtdt::TyTag::List => {
                todo!("List destructor")
            }

            // Set has allocations.
            rtdt::TyTag::Set => {
                todo!("Set destructor")
            }

            // Tuple - recursively destroy fields.
            rtdt::TyTag::Tuple => {
                let tuple_info = ty.type_info.tuple;
                let fields = std::slice::from_raw_parts(
                    tuple_info.fields,
                    tuple_info.num_fields as usize,
                );

                for field in fields {
                    let field_ptr = value_in.add(field.offset as usize);
                    let status = any_destroy_local(rt, field_ptr, field.tydesc);
                    if status != RtStatus::Ok {
                        return status;
                    }
                }

                RtStatus::Ok
            }

            // Struct - recursively destroy fields.
            rtdt::TyTag::Struct => {
                let struct_info = ty.type_info.struct_;
                let fields = std::slice::from_raw_parts(
                    struct_info.fields,
                    struct_info.num_fields as usize,
                );

                for field in fields {
                    let field_ptr = value_in.add(field.offset as usize);
                    let status = any_destroy_local(rt, field_ptr, field.tydesc);
                    if status != RtStatus::Ok {
                        return status;
                    }
                }

                RtStatus::Ok
            }

            // Enum - check tag and destroy payload.
            rtdt::TyTag::Enum => {
                todo!("Enum destructor")
            }

            // Option - check tag and destroy Some value.
            rtdt::TyTag::Option => {
                let option_ptr = value_in as *const rtdt::Option;
                let tag = (*option_ptr).tag;

                if tag == rtdt::OptionTag::Some {
                    let option_info = ty.type_info.option;
                    let layout = rtdt::layout::compute_option_layout(option_info.inner_tydesc);
                    let payload_ptr = value_in.add(layout.payload_offset as usize);
                    any_destroy_local(rt, payload_ptr, option_info.inner_tydesc)
                } else {
                    RtStatus::Ok
                }
            }

            // Result - check tag and destroy Ok/Err value.
            rtdt::TyTag::Result => {
                let result_ptr = value_in as *const rtdt::Result;
                let tag = (*result_ptr).tag;

                let result_info = ty.type_info.result;
                let layout = rtdt::layout::compute_result_layout(result_info.ok_tydesc);
                let payload_ptr = value_in.add(layout.payload_offset as usize);

                match tag {
                    rtdt::ResultTag::Ok => {
                        any_destroy_local(rt, payload_ptr, result_info.ok_tydesc)
                    }
                    rtdt::ResultTag::Err => {
                        // Error type is not parameterized - need to figure out how to destroy it.
                        todo!("Error destructor")
                    }
                }
            }

            // Data and Error - dynamic types.
            rtdt::TyTag::Data => {
                todo!("Data destructor")
            }

            rtdt::TyTag::Error => {
                todo!("Error destructor")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc;

    /// Helper to create a string type descriptor.
    unsafe fn create_string_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::String,
            size: std::mem::size_of::<rtdt::String>() as u32,
            align: std::mem::align_of::<rtdt::String>() as u32,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing,
            },
        }
    }

    /// Helper to create a U32 type descriptor.
    unsafe fn create_u32_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::U32,
            size: 4,
            align: 4,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing,
            },
        }
    }

    #[test]
    fn test_any_destroy_primitive() {
        let rt = alloc::LocalRt::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let tydesc = unsafe { create_u32_tydesc() };

        unsafe {
            let mut value = 42u32;
            let status = any_destroy_local(
                rt_handle,
                &mut value as *mut u32 as *mut u8,
                &tydesc,
            );

            assert_eq!(status, RtStatus::Ok);

            let rt = Box::from_raw(rt_handle as *mut alloc::LocalRt);
            rt.shutdown();
        }
    }

    #[test]
    fn test_any_destroy_string() {
        let rt = alloc::LocalRt::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let tydesc = unsafe { create_string_tydesc() };

        unsafe {
            let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            crate::string::string_create_local(
                rt_handle,
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );
            let mut string = string.assume_init();

            crate::string::string_push_bytes_local(
                rt_handle,
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
                b"hello world".as_ptr(),
                11,
            );

            let status = any_destroy_local(
                rt_handle,
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
            );

            assert_eq!(status, RtStatus::Ok);
            assert!(string.data.is_null());
            assert_eq!(string.size, 0);
            assert_eq!(string.capacity, 0);

            let rt = Box::from_raw(rt_handle as *mut alloc::LocalRt);
            rt.shutdown();
        }
    }

    #[test]
    fn test_any_destroy_tuple_with_string() {
        let rt = alloc::LocalRt::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let string_tydesc = unsafe { create_string_tydesc() };
        let u32_tydesc = unsafe { create_u32_tydesc() };

        unsafe {
            // Create a tuple (u32, String) type descriptor.
            #[repr(C)]
            struct TupleU32String {
                field0: u32,
                field1: rtdt::String,
            }

            let field0_offset = std::mem::offset_of!(TupleU32String, field0) as u32;
            let field1_offset = std::mem::offset_of!(TupleU32String, field1) as u32;

            let fields = vec![
                rtdt::TyInfoTupleField {
                    offset: field0_offset,
                    tydesc: &u32_tydesc as *const _,
                },
                rtdt::TyInfoTupleField {
                    offset: field1_offset,
                    tydesc: &string_tydesc as *const _,
                },
            ];

            let fields_ptr = fields.as_ptr();
            std::mem::forget(fields);

            let tuple_tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::Tuple,
                size: std::mem::size_of::<TupleU32String>() as u32,
                align: std::mem::align_of::<TupleU32String>() as u32,
                type_info: rtdt::TyInfo {
                    tuple: rtdt::TyInfoTuple {
                        num_fields: 2,
                        fields: fields_ptr,
                    },
                },
            };

            // Create the tuple value.
            let mut tuple = TupleU32String {
                field0: 42,
                field1: std::mem::zeroed(),
            };

            // Initialize the string field.
            crate::string::string_create_local(
                rt_handle,
                &mut tuple.field1 as *mut rtdt::String as *mut u8,
                &string_tydesc,
            );

            crate::string::string_push_bytes_local(
                rt_handle,
                &mut tuple.field1 as *mut rtdt::String as *mut u8,
                &string_tydesc,
                b"test".as_ptr(),
                4,
            );

            // Verify string was created.
            assert!(!tuple.field1.data.is_null());
            assert_eq!(tuple.field1.size, 4);

            // Destroy the tuple using any_destroy.
            let status = any_destroy_local(
                rt_handle,
                &mut tuple as *mut TupleU32String as *mut u8,
                &tuple_tydesc,
            );

            assert_eq!(status, RtStatus::Ok);
            assert!(tuple.field1.data.is_null());
            assert_eq!(tuple.field1.size, 0);
            assert_eq!(tuple.field1.capacity, 0);

            let rt = Box::from_raw(rt_handle as *mut alloc::LocalRt);
            rt.shutdown();
        }
    }
}
