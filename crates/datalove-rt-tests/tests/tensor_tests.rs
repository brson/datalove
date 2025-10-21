//! Tests for tensor runtime functions.

use rmx::prelude::*;
use datalove_rt::rtdt;
use std::ptr;

// ============================================================================
// Test Helper Functions
// ============================================================================

/// Create a u32 type descriptor.
fn create_u32_tydesc() -> Box<rtdt::TyDesc> {
    Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::U32,
        size: 4,
        align: 4,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing,
        },
    })
}

/// Create a List<u32> type descriptor.
fn create_list_u32_tydesc() -> (Box<rtdt::TyDesc>, Box<rtdt::TyDesc>) {
    let element_tydesc = create_u32_tydesc();

    let list_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::List,
        size: std::mem::size_of::<rtdt::List>() as u32,
        align: std::mem::align_of::<rtdt::List>() as u32,
        type_info: rtdt::TyInfo {
            list: rtdt::TyInfoList {
                element_tydesc: &*element_tydesc as *const rtdt::TyDesc,
            },
        },
    });

    (list_tydesc, element_tydesc)
}

/// Create a Tensor<u32, N> type descriptor.
fn create_tensor_u32_tydesc(rank: u32) -> (Box<rtdt::TyDesc>, Box<rtdt::TyDesc>) {
    let element_tydesc = create_u32_tydesc();

    let tensor_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Tensor,
        size: std::mem::size_of::<rtdt::Tensor>() as u32,
        align: std::mem::align_of::<rtdt::Tensor>() as u32,
        type_info: rtdt::TyInfo {
            tensor: rtdt::TyInfoTensor {
                element_tydesc: &*element_tydesc as *const rtdt::TyDesc,
                rank,
            },
        },
    });

    (tensor_tydesc, element_tydesc)
}

/// Helper to create a List<u32> from a slice using the runtime.
unsafe fn create_runtime_u32_list(
    rt: datalove_rt::LocalRtHandle,
    values: &[u32],
    list_tydesc: *const rtdt::TyDesc,
    element_tydesc: *const rtdt::TyDesc,
) -> rtdt::List {
    unsafe {
        let mut list = std::mem::MaybeUninit::<rtdt::List>::uninit();
        datalove_rt::dtlv_rti_list_create_local(
            rt,
            list.as_mut_ptr() as *mut u8,
            list_tydesc,
        );
        let mut list = list.assume_init();

        for &value in values {
            let mut v = value;
            datalove_rt::dtlv_rti_list_push_local(
                rt,
                &mut list as *mut rtdt::List as *mut u8,
                list_tydesc,
                &mut v as *mut u32 as *mut u8,
                element_tydesc,
            );
        }

        list
    }
}

// ============================================================================
// Basic Constructor/Destructor Tests
// ============================================================================

/// Test creating a 1D tensor from slice.
#[test]
fn test_tensor_create_from_slice_1d() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(1);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create slice data: [1, 2, 3, 4, 5]
    let slice_data = vec![1u32, 2, 3, 4, 5];

    // Create shape list: [5]
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[5], &*list_tydesc, &*list_element_tydesc)
    };

    // Create tensor.
    let mut tensor = rtdt::Tensor {
        ptr_base: ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: ptr::null(),
        strides: ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::dtlv_rti_tensor_create_from_slice_local(
            rt,
            slice_data.as_ptr() as *const u8,
            slice_data.len() as u32,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut shape_list as *mut rtdt::List as *mut u8,
            &*list_tydesc as *const rtdt::TyDesc,
            rtdt::TensorLayout::RowMajor as u8,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify tensor fields.
    assert!(!tensor.ptr_base.is_null());
    assert_eq!(tensor.offset_elems, 0);
    assert_eq!(tensor.capacity_elems, 5);
    assert!(!tensor.shape.is_null());
    assert!(!tensor.strides.is_null());
    assert_eq!(tensor.layout, rtdt::TensorLayout::RowMajor);

    // Verify shape.
    let shape = unsafe { std::slice::from_raw_parts(tensor.shape, 1) };
    assert_eq!(shape[0], 5);

    // Verify strides (1D row-major: [1]).
    let strides = unsafe { std::slice::from_raw_parts(tensor.strides, 1) };
    assert_eq!(strides[0], 1);

    // Verify data.
    let data = unsafe {
        std::slice::from_raw_parts(tensor.ptr_base as *const u32, 5)
    };
    assert_eq!(data, &[1, 2, 3, 4, 5]);

    // Clean up.
    let status = unsafe {
        datalove_rt::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify tensor is cleared.
    assert!(tensor.ptr_base.is_null());
    assert_eq!(tensor.offset_elems, 0);
    assert_eq!(tensor.capacity_elems, 0);
    assert!(tensor.shape.is_null());
    assert!(tensor.strides.is_null());

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test creating a 2D tensor with row-major layout.
#[test]
fn test_tensor_create_from_slice_2d_row_major() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create slice data: 2x3 = [1, 2, 3, 4, 5, 6]
    let slice_data = vec![1u32, 2, 3, 4, 5, 6];

    // Create shape list: [2, 3]
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[2, 3], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: ptr::null(),
        strides: ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::dtlv_rti_tensor_create_from_slice_local(
            rt,
            slice_data.as_ptr() as *const u8,
            slice_data.len() as u32,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut shape_list as *mut rtdt::List as *mut u8,
            &*list_tydesc as *const rtdt::TyDesc,
            rtdt::TensorLayout::RowMajor as u8,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify tensor fields.
    assert!(!tensor.ptr_base.is_null());
    assert_eq!(tensor.offset_elems, 0);
    assert_eq!(tensor.capacity_elems, 6);
    assert_eq!(tensor.layout, rtdt::TensorLayout::RowMajor);

    // Verify shape: [2, 3]
    let shape = unsafe { std::slice::from_raw_parts(tensor.shape, 2) };
    assert_eq!(shape, &[2, 3]);

    // Verify strides (row-major [2, 3]: [3, 1]).
    let strides = unsafe { std::slice::from_raw_parts(tensor.strides, 2) };
    assert_eq!(strides, &[3, 1]);

    // Verify data.
    let data = unsafe {
        std::slice::from_raw_parts(tensor.ptr_base as *const u32, 6)
    };
    assert_eq!(data, &[1, 2, 3, 4, 5, 6]);

    // Clean up.
    let status = unsafe {
        datalove_rt::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test creating a 2D tensor with column-major layout.
#[test]
fn test_tensor_create_from_slice_2d_col_major() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create slice data: 2x3 = [1, 2, 3, 4, 5, 6]
    let slice_data = vec![1u32, 2, 3, 4, 5, 6];

    // Create shape list: [2, 3]
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[2, 3], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: ptr::null(),
        strides: ptr::null(),
        layout: rtdt::TensorLayout::ColMajor,
    };

    let status = unsafe {
        datalove_rt::dtlv_rti_tensor_create_from_slice_local(
            rt,
            slice_data.as_ptr() as *const u8,
            slice_data.len() as u32,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut shape_list as *mut rtdt::List as *mut u8,
            &*list_tydesc as *const rtdt::TyDesc,
            rtdt::TensorLayout::ColMajor as u8,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify tensor fields.
    assert!(!tensor.ptr_base.is_null());
    assert_eq!(tensor.offset_elems, 0);
    assert_eq!(tensor.capacity_elems, 6);
    assert_eq!(tensor.layout, rtdt::TensorLayout::ColMajor);

    // Verify shape: [2, 3]
    let shape = unsafe { std::slice::from_raw_parts(tensor.shape, 2) };
    assert_eq!(shape, &[2, 3]);

    // Verify strides (col-major [2, 3]: [1, 2]).
    let strides = unsafe { std::slice::from_raw_parts(tensor.strides, 2) };
    assert_eq!(strides, &[1, 2]);

    // Clean up.
    let status = unsafe {
        datalove_rt::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test creating a 3D tensor.
#[test]
fn test_tensor_create_from_slice_3d() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(3);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create slice data: 2x3x4 = 24 elements
    let slice_data: Vec<u32> = (0..24).collect();

    // Create shape list: [2, 3, 4]
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[2, 3, 4], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: ptr::null(),
        strides: ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::dtlv_rti_tensor_create_from_slice_local(
            rt,
            slice_data.as_ptr() as *const u8,
            slice_data.len() as u32,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut shape_list as *mut rtdt::List as *mut u8,
            &*list_tydesc as *const rtdt::TyDesc,
            rtdt::TensorLayout::RowMajor as u8,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify tensor fields.
    assert!(!tensor.ptr_base.is_null());
    assert_eq!(tensor.offset_elems, 0);
    assert_eq!(tensor.capacity_elems, 24);
    assert_eq!(tensor.layout, rtdt::TensorLayout::RowMajor);

    // Verify shape: [2, 3, 4]
    let shape = unsafe { std::slice::from_raw_parts(tensor.shape, 3) };
    assert_eq!(shape, &[2, 3, 4]);

    // Verify strides (row-major [2, 3, 4]: [12, 4, 1]).
    let strides = unsafe { std::slice::from_raw_parts(tensor.strides, 3) };
    assert_eq!(strides, &[12, 4, 1]);

    // Verify data.
    let data = unsafe {
        std::slice::from_raw_parts(tensor.ptr_base as *const u32, 24)
    };
    assert_eq!(data, slice_data.as_slice());

    // Clean up.
    let status = unsafe {
        datalove_rt::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// Edge Case Tests
// ============================================================================

/// Test creating tensor with mismatched slice length returns error.
#[test]
fn test_tensor_create_mismatched_length() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create slice data with 5 elements
    let slice_data = vec![1u32, 2, 3, 4, 5];

    // Create shape list: [2, 3] (expects 6 elements)
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[2, 3], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: ptr::null(),
        strides: ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    // This should fail because slice_len (5) != shape product (6).
    let status = unsafe {
        datalove_rt::dtlv_rti_tensor_create_from_slice_local(
            rt,
            slice_data.as_ptr() as *const u8,
            slice_data.len() as u32,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut shape_list as *mut rtdt::List as *mut u8,
            &*list_tydesc as *const rtdt::TyDesc,
            rtdt::TensorLayout::RowMajor as u8,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Error);

    // Tensor should remain uninitialized.
    assert!(tensor.ptr_base.is_null());

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test creating tensor with null pointers returns error.
#[test]
fn test_tensor_create_null_pointers() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(1);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    let slice_data = vec![1u32, 2, 3];

    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[3], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: ptr::null(),
        strides: ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    // Test with null tensor_value_out.
    let status = unsafe {
        datalove_rt::dtlv_rti_tensor_create_from_slice_local(
            rt,
            slice_data.as_ptr() as *const u8,
            slice_data.len() as u32,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut shape_list as *mut rtdt::List as *mut u8,
            &*list_tydesc as *const rtdt::TyDesc,
            rtdt::TensorLayout::RowMajor as u8,
            ptr::null_mut(),
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Error);

    // Test with null slice_ptr_ref.
    let status = unsafe {
        datalove_rt::dtlv_rti_tensor_create_from_slice_local(
            rt,
            ptr::null(),
            3,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut shape_list as *mut rtdt::List as *mut u8,
            &*list_tydesc as *const rtdt::TyDesc,
            rtdt::TensorLayout::RowMajor as u8,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Error);

    // Clean up shape list.
    unsafe {
        datalove_rt::dtlv_rti_list_destroy_local(
            rt,
            &mut shape_list as *mut rtdt::List as *mut u8,
            &*list_tydesc as *const rtdt::TyDesc,
        );
    }

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test destroying tensor with null pointer returns error.
#[test]
fn test_tensor_destroy_null_pointer() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, _element_tydesc) = create_tensor_u32_tydesc(2);

    let status = unsafe {
        datalove_rt::dtlv_rti_tensor_destroy_local(
            rt,
            ptr::null_mut(),
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Error);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// Multiple Tensors Test
// ============================================================================

/// Test creating and destroying multiple tensors.
#[test]
fn test_multiple_tensors() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(1);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create three different tensors.
    let mut tensors = vec![];

    for i in 0..3 {
        let size = (i + 1) * 5;
        let slice_data: Vec<u32> = (0..size).collect();

        let mut shape_list = unsafe {
            create_runtime_u32_list(rt, &[size as u32], &*list_tydesc, &*list_element_tydesc)
        };

        let mut tensor = rtdt::Tensor {
            ptr_base: ptr::null_mut(),
            offset_elems: 0,
            capacity_elems: 0,
            shape: ptr::null(),
            strides: ptr::null(),
            layout: rtdt::TensorLayout::RowMajor,
        };

        let status = unsafe {
            datalove_rt::dtlv_rti_tensor_create_from_slice_local(
                rt,
                slice_data.as_ptr() as *const u8,
                slice_data.len() as u32,
                &*element_tydesc as *const rtdt::TyDesc,
                &mut shape_list as *mut rtdt::List as *mut u8,
                &*list_tydesc as *const rtdt::TyDesc,
                rtdt::TensorLayout::RowMajor as u8,
                &mut tensor as *mut rtdt::Tensor as *mut u8,
                &*tensor_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::RtStatus::Ok);
        assert!(!tensor.ptr_base.is_null());

        tensors.push(tensor);
    }

    // Verify all tensors have distinct allocations.
    for i in 0..3 {
        for j in (i + 1)..3 {
            assert_ne!(tensors[i].ptr_base, tensors[j].ptr_base);
            assert_ne!(tensors[i].shape, tensors[j].shape);
            assert_ne!(tensors[i].strides, tensors[j].strides);
        }
    }

    // Destroy all tensors.
    for tensor in &mut tensors {
        let status = unsafe {
            datalove_rt::dtlv_rti_tensor_destroy_local(
                rt,
                tensor as *mut rtdt::Tensor as *mut u8,
                &*tensor_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::RtStatus::Ok);
        assert!(tensor.ptr_base.is_null());
    }

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}
