//! Destructor implementation for all Datalove types.

use crate::{impls::rt_local, rtdt};
use crate::c::{LocalRtHandle, RtStatus};

/// Destroys any type of value, freeing allocations recursively.
pub unsafe fn any_destroy_local(
    rt: LocalRtHandle,
    value_in: *mut u8,
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        let ty = rtdt::TyDescRef::from_ptr(tydesc);
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);

        match ty.type_tag() {
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
                let int_ptr = value_in as *mut rtdt::Int;
                let int = &*int_ptr;

                // Free the limb buffer if it exists.
                if !int.data.is_null() && int.capacity > 0 {
                    // Each limb is a u32.
                    rt_ref.alloc.free(4, 4, int.capacity, int.data as *mut u8);
                }

                // Clear the int fields.
                (*int_ptr).data = std::ptr::null();
                (*int_ptr).size_and_sign = 0;
                (*int_ptr).capacity = 0;

                RtStatus::Ok
            }

            // String has allocations.
            rtdt::TyTag::String => {
                crate::impls::string::string_destroy_local(rt, value_in, tydesc)
            }

            // Map has allocations.
            rtdt::TyTag::Map => {
                crate::impls::btreemap::btreemap_destroy_impl(rt_ref, value_in, ty)
            }

            // List has allocations.
            rtdt::TyTag::List => {
                let list_ptr = value_in as *mut rtdt::List;
                let list = &*list_ptr;
                let element_ty = ty.list_element_ty();
                let element_tydesc = element_ty.as_ptr();

                // Recursively destroy each element.
                if !list.data.is_null() && list.size > 0 {
                    let element_size = element_ty.size() as usize;
                    for i in 0..list.size {
                        let element_ptr = (list.data as *mut u8).add((i as usize) * element_size);
                        let status = any_destroy_local(rt, element_ptr, element_tydesc);
                        if status != RtStatus::Ok {
                            return status;
                        }
                    }
                }

                // Free the list buffer if it exists.
                // Re-obtain rt_ref after recursive calls to satisfy Stacked Borrows.
                let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
                if !list.data.is_null() && list.capacity > 0 {
                    rt_ref.alloc.free(element_ty.size(), element_ty.align(), list.capacity, list.data as *mut u8);
                }

                // Clear the list fields.
                (*list_ptr).data = std::ptr::null();
                (*list_ptr).size = 0;
                (*list_ptr).capacity = 0;

                RtStatus::Ok
            }

            // Set has allocations.
            rtdt::TyTag::Set => {
                crate::impls::set::set_destroy_impl(rt_ref, value_in, tydesc)
            }

            // Tensor has allocations.
            rtdt::TyTag::Tensor => {
                let tensor = &*(value_in as *const rtdt::Tensor);
                let tensor_ptr = value_in as *mut rtdt::Tensor;

                let element_ty = ty.tensor_element_ty();
                let element_tydesc = element_ty.as_ptr();
                let rank = ty.tensor_rank();

                // Recursively destroy each element in the base buffer.
                if !tensor.ptr_base.is_null() && tensor.capacity_elems > 0 {
                    let element_size = element_ty.size() as usize;
                    for i in 0..tensor.capacity_elems {
                        let element_ptr = tensor.ptr_base.add((i as usize) * element_size);
                        let status = any_destroy_local(rt, element_ptr, element_tydesc);
                        if status != RtStatus::Ok {
                            return status;
                        }
                    }
                }

                // Free the data buffer if it exists.
                // Re-obtain rt_ref after recursive calls to satisfy Stacked Borrows.
                let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
                if !tensor.ptr_base.is_null() && tensor.capacity_elems > 0 {
                    rt_ref.alloc.free(
                        element_ty.size(),
                        element_ty.align(),
                        tensor.capacity_elems,
                        tensor.ptr_base,
                    );
                }

                // Free the shape array if it exists.
                if !tensor.shape.is_null() && rank > 0 {
                    rt_ref.alloc.free(4, 4, rank, tensor.shape as *mut u8);
                }

                // Free the stride array if it exists.
                if !tensor.strides.is_null() && rank > 0 {
                    rt_ref.alloc.free(4, 4, rank, tensor.strides as *mut u8);
                }

                // Clear the tensor fields.
                (*tensor_ptr).ptr_base = std::ptr::null_mut();
                (*tensor_ptr).offset_elems = 0;
                (*tensor_ptr).capacity_elems = 0;
                (*tensor_ptr).shape = std::ptr::null();
                (*tensor_ptr).strides = std::ptr::null();

                RtStatus::Ok
            }

            // Tuple - recursively destroy fields.
            rtdt::TyTag::Tuple => {
                for field in ty.iter_tuple_fields() {
                    let field_ptr = value_in.add(field.offset() as usize);
                    let status = any_destroy_local(rt, field_ptr, field.tydesc().as_ptr());
                    if status != RtStatus::Ok {
                        return status;
                    }
                }

                RtStatus::Ok
            }

            // Struct - recursively destroy fields.
            rtdt::TyTag::Struct => {
                for field in ty.iter_struct_fields() {
                    let field_ptr = value_in.add(field.offset() as usize);
                    let status = any_destroy_local(rt, field_ptr, field.tydesc().as_ptr());
                    if status != RtStatus::Ok {
                        return status;
                    }
                }

                RtStatus::Ok
            }

            // Enum - check tag and destroy payload.
            rtdt::TyTag::Enum => {
                let enum_info = ty.enum_info();
                let layout = rtdt::layout::compute_enum_layout(rtdt::TyDescRef::from_ptr(tydesc));

                // Read the discriminant (u32 at offset 0).
                let discriminant_ptr = value_in as *const u32;
                let discriminant = *discriminant_ptr;

                // Find the variant.
                if discriminant < enum_info.num_variants() {
                    if let core::option::Option::Some(variant) = enum_info.variant(discriminant as usize) {
                        // If variant has payload, destroy it.
                        if let core::option::Option::Some(payload_ty) = variant.payload() {
                            let payload_offset = layout.variant_offsets[discriminant as usize];
                            let payload_ptr = value_in.add(payload_offset as usize);
                            let status = any_destroy_local(rt, payload_ptr, payload_ty.as_ptr());
                            if status != RtStatus::Ok {
                                return status;
                            }
                        }
                    }
                }

                RtStatus::Ok
            }

            // Option - check tag and destroy Some value.
            rtdt::TyTag::Option => {
                let option_ptr = value_in as *const rtdt::Option;
                let tag = (*option_ptr).tag;

                if tag == rtdt::OptionTag::Some {
                    let inner_ty = ty.option_inner_ty();
                    let layout = rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(tydesc));
                    let payload_ptr = value_in.add(layout.payload_offset as usize);
                    any_destroy_local(rt, payload_ptr, inner_ty.as_ptr())
                } else {
                    RtStatus::Ok
                }
            }

            // Result - check tag and destroy Ok/Err value.
            rtdt::TyTag::Result => {
                let result_ptr = value_in as *const rtdt::Result;
                let tag = (*result_ptr).tag;

                let ok_ty = ty.result_ok_ty();
                let layout = rtdt::layout::compute_result_layout(rtdt::TyDescRef::from_ptr(tydesc));
                let payload_ptr = value_in.add(layout.payload_offset as usize);

                match tag {
                    rtdt::ResultTag::Ok => {
                        any_destroy_local(rt, payload_ptr, ok_ty.as_ptr())
                    }
                    rtdt::ResultTag::Err => {
                        // Destroy the Error payload.
                        // Create a temporary Error tydesc.
                        // fixme this is pretty sketchy!
                        let error_tydesc = rtdt::TyDesc {
                            type_tag: rtdt::TyTag::Error,
                            size: std::mem::size_of::<rtdt::Error>() as u32,
                            align: std::mem::align_of::<rtdt::Error>() as u32,
                            type_info: rtdt::TyInfo {
                                nothing: rtdt::TyInfoNothing,
                            },
                        };
                        any_destroy_local(rt, payload_ptr, &error_tydesc as *const rtdt::TyDesc)
                    }
                }
            }

            // Data and Error - dynamic types.
            rtdt::TyTag::Data => {
                let data_ptr = value_in as *const rtdt::Data;
                let data = &*data_ptr;

                match data.tag() {
                    rtdt::anypack::Tag::TwoPointers => {
                        // Extract inner tydesc and value pointer.
                        let inner_tydesc = data.tydesc();
                        let inner_value_ptr = data.value_ptr();

                        // Recursively destroy the inner value.
                        if !inner_value_ptr.is_null() && !inner_tydesc.is_null() {
                            let status = any_destroy_local(rt, inner_value_ptr as *mut u8, inner_tydesc);
                            if status != RtStatus::Ok {
                                return status;
                            }

                            // Free the inner value allocation.
                            // Re-obtain rt_ref after recursive call to satisfy Stacked Borrows.
                            let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
                            let inner_ty = rtdt::TyDescRef::from_ptr(inner_tydesc);
                            rt_ref.alloc.free(inner_ty.size(), inner_ty.align(), 1, inner_value_ptr as *mut u8);
                        }

                        RtStatus::Ok
                    }
                    rtdt::anypack::Tag::SmallImmediate | rtdt::anypack::Tag::InlineWithTyDesc => {
                        // Value is inline, no allocations to free.
                        RtStatus::Ok
                    }
                    _ => {
                        // Invalid tag.
                        RtStatus::Error
                    }
                }
            }

            rtdt::TyTag::Error => {
                // Error uses same encoding as Data.
                let error_ptr = value_in as *const rtdt::Error;
                let as_data_ptr = error_ptr as *const rtdt::Data;
                let data = &*as_data_ptr;

                match data.tag() {
                    rtdt::anypack::Tag::TwoPointers => {
                        // Extract inner tydesc and value pointer using Error methods.
                        let inner_tydesc = (*error_ptr).tydesc();
                        let inner_value_ptr = (*error_ptr).value_ptr();

                        // Recursively destroy the inner value.
                        if !inner_value_ptr.is_null() && !inner_tydesc.is_null() {
                            let status = any_destroy_local(rt, inner_value_ptr as *mut u8, inner_tydesc);
                            if status != RtStatus::Ok {
                                return status;
                            }

                            // Free the inner value allocation.
                            // Re-obtain rt_ref after recursive call to satisfy Stacked Borrows.
                            let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
                            let inner_ty = rtdt::TyDescRef::from_ptr(inner_tydesc);
                            rt_ref.alloc.free(inner_ty.size(), inner_ty.align(), 1, inner_value_ptr as *mut u8);
                        }

                        RtStatus::Ok
                    }
                    rtdt::anypack::Tag::SmallImmediate | rtdt::anypack::Tag::InlineWithTyDesc => {
                        // Value is inline, no allocations to free.
                        RtStatus::Ok
                    }
                    _ => {
                        // Invalid tag.
                        RtStatus::Error
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let rt = rt_local::RtLocal::new();
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

            let rt = Box::from_raw(rt_handle as *mut rt_local::RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_any_destroy_string() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let tydesc = unsafe { create_string_tydesc() };

        unsafe {
            let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            crate::impls::string::string_create_local(
                rt_handle,
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );
            let mut string = string.assume_init();

            crate::impls::string::string_push_bytes_local(
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

            let rt = Box::from_raw(rt_handle as *mut rt_local::RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_any_destroy_tuple_with_string() {
        let rt = rt_local::RtLocal::new();
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
            crate::impls::string::string_create_local(
                rt_handle,
                &mut tuple.field1 as *mut rtdt::String as *mut u8,
                &string_tydesc,
            );

            crate::impls::string::string_push_bytes_local(
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

            // Reconstruct and drop the fields Vec to avoid leaking.
            drop(Vec::from_raw_parts(fields_ptr as *mut rtdt::TyInfoTupleField, 2, 2));

            let rt = Box::from_raw(rt_handle as *mut rt_local::RtLocal);
            rt.shutdown();
        }
    }

    /// Helper to create a Data type descriptor.
    unsafe fn create_data_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::Data,
            size: std::mem::size_of::<rtdt::Data>() as u32,
            align: std::mem::align_of::<rtdt::Data>() as u32,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing,
            },
        }
    }

    /// Helper to create an Error type descriptor.
    unsafe fn create_error_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::Error,
            size: std::mem::size_of::<rtdt::Error>() as u32,
            align: std::mem::align_of::<rtdt::Error>() as u32,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing,
            },
        }
    }

    #[test]
    fn test_any_destroy_data_small_immediate() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let data_tydesc = unsafe { create_data_tydesc() };

        unsafe {
            // Create Data with SmallImmediate encoding (u32).
            let mut data = rtdt::Data::from_u32(42);

            let status = any_destroy_local(
                rt_handle,
                &mut data as *mut rtdt::Data as *mut u8,
                &data_tydesc,
            );

            assert_eq!(status, RtStatus::Ok);

            let rt = Box::from_raw(rt_handle as *mut rt_local::RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_any_destroy_data_inline_with_tydesc() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let data_tydesc = unsafe { create_data_tydesc() };
        let f64_tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::F64,
            size: 8,
            align: 8,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing,
            },
        };

        unsafe {
            // Create Data with InlineWithTyDesc encoding (f64).
            let mut data = rtdt::Data::from_f64(3.14159, &f64_tydesc);

            let status = any_destroy_local(
                rt_handle,
                &mut data as *mut rtdt::Data as *mut u8,
                &data_tydesc,
            );

            assert_eq!(status, RtStatus::Ok);

            let rt = Box::from_raw(rt_handle as *mut rt_local::RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_any_destroy_data_two_pointers_primitive() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut rt_local::RtLocal) };
        let data_tydesc = unsafe { create_data_tydesc() };
        let u32_tydesc = unsafe { create_u32_tydesc() };

        unsafe {
            // Allocate a u32 value.
            let inner_value_ptr = rt_ref.alloc.alloc(4, 4, 1) as *mut u32;
            *inner_value_ptr = 123;

            // Create Data with TwoPointers encoding.
            let mut data = rtdt::Data::from_pointers(&u32_tydesc, inner_value_ptr as *const u8);

            let status = any_destroy_local(
                rt_handle,
                &mut data as *mut rtdt::Data as *mut u8,
                &data_tydesc,
            );

            assert_eq!(status, RtStatus::Ok);

            let rt = Box::from_raw(rt_handle as *mut rt_local::RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_any_destroy_data_two_pointers_string() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut rt_local::RtLocal) };
        let data_tydesc = unsafe { create_data_tydesc() };
        let string_tydesc = unsafe { create_string_tydesc() };

        unsafe {
            // Allocate and initialize a String value.
            let inner_string_ptr = rt_ref.alloc.alloc(
                string_tydesc.size,
                string_tydesc.align,
                1
            ) as *mut rtdt::String;

            crate::impls::string::string_create_local(
                rt_handle,
                inner_string_ptr as *mut u8,
                &string_tydesc,
            );

            crate::impls::string::string_push_bytes_local(
                rt_handle,
                inner_string_ptr as *mut u8,
                &string_tydesc,
                b"hello data".as_ptr(),
                10,
            );

            // Create Data with TwoPointers encoding.
            let mut data = rtdt::Data::from_pointers(&string_tydesc, inner_string_ptr as *const u8);

            let status = any_destroy_local(
                rt_handle,
                &mut data as *mut rtdt::Data as *mut u8,
                &data_tydesc,
            );

            assert_eq!(status, RtStatus::Ok);

            let rt = Box::from_raw(rt_handle as *mut rt_local::RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_any_destroy_error_small_immediate() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let error_tydesc = unsafe { create_error_tydesc() };

        unsafe {
            // Create Error with SmallImmediate encoding (reinterpret Data).
            let data = rtdt::Data::from_u32(404);
            let mut error = std::mem::transmute::<rtdt::Data, rtdt::Error>(data);

            let status = any_destroy_local(
                rt_handle,
                &mut error as *mut rtdt::Error as *mut u8,
                &error_tydesc,
            );

            assert_eq!(status, RtStatus::Ok);

            let rt = Box::from_raw(rt_handle as *mut rt_local::RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_any_destroy_error_inline_with_tydesc() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let error_tydesc = unsafe { create_error_tydesc() };
        let i64_tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::I64,
            size: 8,
            align: 8,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing,
            },
        };

        unsafe {
            // Create Error with InlineWithTyDesc encoding.
            let data = rtdt::Data::from_i64(-500, &i64_tydesc);
            let mut error = std::mem::transmute::<rtdt::Data, rtdt::Error>(data);

            let status = any_destroy_local(
                rt_handle,
                &mut error as *mut rtdt::Error as *mut u8,
                &error_tydesc,
            );

            assert_eq!(status, RtStatus::Ok);

            let rt = Box::from_raw(rt_handle as *mut rt_local::RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_any_destroy_error_two_pointers_primitive() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut rt_local::RtLocal) };
        let error_tydesc = unsafe { create_error_tydesc() };
        let u32_tydesc = unsafe { create_u32_tydesc() };

        unsafe {
            // Allocate a u32 value.
            let inner_value_ptr = rt_ref.alloc.alloc(4, 4, 1) as *mut u32;
            *inner_value_ptr = 500;

            // Create Error with TwoPointers encoding.
            let data = rtdt::Data::from_pointers(&u32_tydesc, inner_value_ptr as *const u8);
            let mut error = std::mem::transmute::<rtdt::Data, rtdt::Error>(data);

            let status = any_destroy_local(
                rt_handle,
                &mut error as *mut rtdt::Error as *mut u8,
                &error_tydesc,
            );

            assert_eq!(status, RtStatus::Ok);

            let rt = Box::from_raw(rt_handle as *mut rt_local::RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_any_destroy_error_two_pointers_string() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut rt_local::RtLocal) };
        let error_tydesc = unsafe { create_error_tydesc() };
        let string_tydesc = unsafe { create_string_tydesc() };

        unsafe {
            // Allocate and initialize a String value.
            let inner_string_ptr = rt_ref.alloc.alloc(
                string_tydesc.size,
                string_tydesc.align,
                1
            ) as *mut rtdt::String;

            crate::impls::string::string_create_local(
                rt_handle,
                inner_string_ptr as *mut u8,
                &string_tydesc,
            );

            crate::impls::string::string_push_bytes_local(
                rt_handle,
                inner_string_ptr as *mut u8,
                &string_tydesc,
                b"error occurred".as_ptr(),
                14,
            );

            // Create Error with TwoPointers encoding.
            let data = rtdt::Data::from_pointers(&string_tydesc, inner_string_ptr as *const u8);
            let mut error = std::mem::transmute::<rtdt::Data, rtdt::Error>(data);

            let status = any_destroy_local(
                rt_handle,
                &mut error as *mut rtdt::Error as *mut u8,
                &error_tydesc,
            );

            assert_eq!(status, RtStatus::Ok);

            let rt = Box::from_raw(rt_handle as *mut rt_local::RtLocal);
            rt.shutdown();
        }
    }
}
