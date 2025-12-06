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
    rt: datalove_rt::c::LocalRtHandle,
    values: &[u32],
    list_tydesc: *const rtdt::TyDesc,
    element_tydesc: *const rtdt::TyDesc,
) -> rtdt::List {
    unsafe {
        let mut list = std::mem::MaybeUninit::<rtdt::List>::uninit();
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list.as_mut_ptr() as *mut u8,
            list_tydesc,
        );
        let mut list = list.assume_init();

        for &value in values {
            let mut v = value;
            datalove_rt::c::dtlv_rti_list_push_local(
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
    let rt = datalove_rt::c::dtlv_rti_init();
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

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
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify tensor is cleared.
    assert!(tensor.ptr_base.is_null());
    assert_eq!(tensor.offset_elems, 0);
    assert_eq!(tensor.capacity_elems, 0);
    assert!(tensor.shape.is_null());
    assert!(tensor.strides.is_null());

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test creating a 2D tensor with row-major layout.
#[test]
fn test_tensor_create_from_slice_2d_row_major() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

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
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test creating a 2D tensor with column-major layout.
#[test]
fn test_tensor_create_from_slice_2d_col_major() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

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
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test creating a 3D tensor.
#[test]
fn test_tensor_create_from_slice_3d() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

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
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// Edge Case Tests
// ============================================================================

/// Test creating tensor with mismatched slice length returns error.
#[test]
fn test_tensor_create_mismatched_length() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Tensor should remain uninitialized.
    assert!(tensor.ptr_base.is_null());

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test creating tensor with null pointers returns error.
#[test]
fn test_tensor_create_null_pointers() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Test with null slice_ptr_ref.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Clean up shape list.
    unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            &mut shape_list as *mut rtdt::List as *mut u8,
            &*list_tydesc as *const rtdt::TyDesc,
        );
    }

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test destroying tensor with null pointer returns error.
#[test]
fn test_tensor_destroy_null_pointer() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, _element_tydesc) = create_tensor_u32_tydesc(2);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            ptr::null_mut(),
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// Multiple Tensors Test
// ============================================================================

/// Test creating and destroying multiple tensors.
#[test]
fn test_multiple_tensors() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
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
            datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
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
            datalove_rt::c::dtlv_rti_tensor_destroy_local(
                rt,
                tensor as *mut rtdt::Tensor as *mut u8,
                &*tensor_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert!(tensor.ptr_base.is_null());
    }

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// Tensor Get Tests
// ============================================================================

/// Test getting elements from a 1D tensor.
#[test]
fn test_tensor_get_1d() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(1);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a 1D tensor: [10, 20, 30, 40, 50]
    let slice_data: Vec<u32> = vec![10, 20, 30, 40, 50];
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[5], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: std::ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: std::ptr::null(),
        strides: std::ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Get each element and verify.
    for i in 0..5 {
        let indices = [i];
        let mut element_value = std::mem::MaybeUninit::<u32>::uninit();

        let status = unsafe {
            datalove_rt::c::dtlv_rti_tensor_get_local(
                rt,
                &tensor as *const rtdt::Tensor as *const u8,
                &*tensor_tydesc as *const rtdt::TyDesc,
                indices.as_ptr(),
                element_value.as_mut_ptr() as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let mut value = unsafe { element_value.assume_init() };
        assert_eq!(value, slice_data[i as usize]);

        // Destroy the cloned element.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                rt,
                &mut value as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Destroy tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test getting elements from a 2D tensor.
#[test]
fn test_tensor_get_2d() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a 2x3 tensor in row-major order:
    // [[10, 20, 30],
    //  [40, 50, 60]]
    let slice_data: Vec<u32> = vec![10, 20, 30, 40, 50, 60];
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[2, 3], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: std::ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: std::ptr::null(),
        strides: std::ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Test specific elements.
    let test_cases = [
        ([0, 0], 10),
        ([0, 1], 20),
        ([0, 2], 30),
        ([1, 0], 40),
        ([1, 1], 50),
        ([1, 2], 60),
    ];

    for (indices, expected) in test_cases {
        let mut element_value = std::mem::MaybeUninit::<u32>::uninit();

        let status = unsafe {
            datalove_rt::c::dtlv_rti_tensor_get_local(
                rt,
                &tensor as *const rtdt::Tensor as *const u8,
                &*tensor_tydesc as *const rtdt::TyDesc,
                indices.as_ptr(),
                element_value.as_mut_ptr() as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let mut value = unsafe { element_value.assume_init() };
        assert_eq!(value, expected);

        // Destroy the cloned element.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                rt,
                &mut value as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Destroy tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test getting elements from a 3D tensor.
#[test]
fn test_tensor_get_3d() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(3);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a 2x2x3 tensor.
    let slice_data: Vec<u32> = (0..12).collect();
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[2, 2, 3], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: std::ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: std::ptr::null(),
        strides: std::ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Test some specific elements.
    // Shape is [2, 2, 3], strides are [6, 3, 1].
    // Linear index = i*6 + j*3 + k.
    let test_cases = [
        ([0, 0, 0], 0),   // 0*6 + 0*3 + 0 = 0
        ([0, 0, 2], 2),   // 0*6 + 0*3 + 2 = 2
        ([0, 1, 0], 3),   // 0*6 + 1*3 + 0 = 3
        ([1, 0, 0], 6),   // 1*6 + 0*3 + 0 = 6
        ([1, 1, 2], 11),  // 1*6 + 1*3 + 2 = 11
    ];

    for (indices, expected) in test_cases {
        let mut element_value = std::mem::MaybeUninit::<u32>::uninit();

        let status = unsafe {
            datalove_rt::c::dtlv_rti_tensor_get_local(
                rt,
                &tensor as *const rtdt::Tensor as *const u8,
                &*tensor_tydesc as *const rtdt::TyDesc,
                indices.as_ptr(),
                element_value.as_mut_ptr() as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let mut value = unsafe { element_value.assume_init() };
        assert_eq!(value, expected);

        // Destroy the cloned element.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                rt,
                &mut value as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Destroy tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test out of bounds access.
#[test]
fn test_tensor_get_out_of_bounds() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a 2x3 tensor.
    let slice_data: Vec<u32> = vec![10, 20, 30, 40, 50, 60];
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[2, 3], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: std::ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: std::ptr::null(),
        strides: std::ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Test out of bounds accesses.
    let invalid_indices = [
        [2, 0],  // First dimension out of bounds (max is 1).
        [0, 3],  // Second dimension out of bounds (max is 2).
        [2, 3],  // Both out of bounds.
    ];

    for indices in invalid_indices {
        let mut element_value = std::mem::MaybeUninit::<u32>::uninit();

        let status = unsafe {
            datalove_rt::c::dtlv_rti_tensor_get_local(
                rt,
                &tensor as *const rtdt::Tensor as *const u8,
                &*tensor_tydesc as *const rtdt::TyDesc,
                indices.as_ptr(),
                element_value.as_mut_ptr() as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Error);
    }

    // Destroy tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test tensor_get with null pointers.
#[test]
fn test_tensor_get_null_pointers() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(1);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a valid tensor.
    let slice_data: Vec<u32> = vec![10, 20, 30];
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[3], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: std::ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: std::ptr::null(),
        strides: std::ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let indices = [0];
    let mut element_value = std::mem::MaybeUninit::<u32>::uninit();

    // Null tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_get_local(
            rt,
            std::ptr::null(),
            &*tensor_tydesc as *const rtdt::TyDesc,
            indices.as_ptr(),
            element_value.as_mut_ptr() as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Null indices.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_get_local(
            rt,
            &tensor as *const rtdt::Tensor as *const u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            std::ptr::null(),
            element_value.as_mut_ptr() as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Null output pointer.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_get_local(
            rt,
            &tensor as *const rtdt::Tensor as *const u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            indices.as_ptr(),
            std::ptr::null_mut(),
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Destroy tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test tensor_get with column-major layout.
#[test]
fn test_tensor_get_col_major() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a 2x3 tensor in column-major order.
    // The slice is in column-major order:
    // col0=[10, 40], col1=[20, 50], col2=[30, 60]
    // So linear memory: [10, 40, 20, 50, 30, 60]
    let slice_data: Vec<u32> = vec![10, 40, 20, 50, 30, 60];
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[2, 3], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: std::ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: std::ptr::null(),
        strides: std::ptr::null(),
        layout: rtdt::TensorLayout::ColMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify strides are [1, 2] for column-major.
    let strides = unsafe { std::slice::from_raw_parts(tensor.strides, 2) };
    assert_eq!(strides, &[1, 2]);

    // Test specific elements.
    // For col-major with shape [2, 3] and strides [1, 2]:
    // Linear index = i*1 + j*2
    let test_cases = [
        ([0, 0], 10),  // 0*1 + 0*2 = 0
        ([1, 0], 40),  // 1*1 + 0*2 = 1
        ([0, 1], 20),  // 0*1 + 1*2 = 2
        ([1, 1], 50),  // 1*1 + 1*2 = 3
        ([0, 2], 30),  // 0*1 + 2*2 = 4
        ([1, 2], 60),  // 1*1 + 2*2 = 5
    ];

    for (indices, expected) in test_cases {
        let mut element_value = std::mem::MaybeUninit::<u32>::uninit();

        let status = unsafe {
            datalove_rt::c::dtlv_rti_tensor_get_local(
                rt,
                &tensor as *const rtdt::Tensor as *const u8,
                &*tensor_tydesc as *const rtdt::TyDesc,
                indices.as_ptr(),
                element_value.as_mut_ptr() as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let mut value = unsafe { element_value.assume_init() };
        assert_eq!(value, expected);

        // Destroy the cloned element.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                rt,
                &mut value as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Destroy tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// Tensor Set Tests
// ============================================================================

/// Test setting elements in a 1D tensor.
#[test]
fn test_tensor_set_1d() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(1);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a 1D tensor: [10, 20, 30, 40, 50]
    let slice_data: Vec<u32> = vec![10, 20, 30, 40, 50];
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[5], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: std::ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: std::ptr::null(),
        strides: std::ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Set each element to a new value.
    for i in 0..5 {
        let indices = [i];
        let new_value = 100 + i * 10;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_tensor_set_local(
                rt,
                &mut tensor as *mut rtdt::Tensor as *mut u8,
                &*tensor_tydesc as *const rtdt::TyDesc,
                indices.as_ptr(),
                &new_value as *const u32 as *const u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Verify all elements were updated.
    for i in 0..5 {
        let indices = [i];
        let mut element_value = std::mem::MaybeUninit::<u32>::uninit();

        let status = unsafe {
            datalove_rt::c::dtlv_rti_tensor_get_local(
                rt,
                &tensor as *const rtdt::Tensor as *const u8,
                &*tensor_tydesc as *const rtdt::TyDesc,
                indices.as_ptr(),
                element_value.as_mut_ptr() as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let mut value = unsafe { element_value.assume_init() };
        assert_eq!(value, 100 + i * 10);

        // Destroy the cloned element.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                rt,
                &mut value as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Destroy tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test setting elements in a 2D tensor with row-major layout.
#[test]
fn test_tensor_set_2d_row_major() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a 2x3 tensor in row-major order:
    // [[10, 20, 30],
    //  [40, 50, 60]]
    let slice_data: Vec<u32> = vec![10, 20, 30, 40, 50, 60];
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[2, 3], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: std::ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: std::ptr::null(),
        strides: std::ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Set specific elements.
    let updates = [
        ([0, 0], 100u32),
        ([0, 2], 102u32),
        ([1, 1], 111u32),
    ];

    for (indices, new_value) in updates {
        let status = unsafe {
            datalove_rt::c::dtlv_rti_tensor_set_local(
                rt,
                &mut tensor as *mut rtdt::Tensor as *mut u8,
                &*tensor_tydesc as *const rtdt::TyDesc,
                indices.as_ptr(),
                &new_value as *const u32 as *const u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Verify updates.
    let expected = [
        ([0, 0], 100),
        ([0, 1], 20),
        ([0, 2], 102),
        ([1, 0], 40),
        ([1, 1], 111),
        ([1, 2], 60),
    ];

    for (indices, expected_value) in expected {
        let mut element_value = std::mem::MaybeUninit::<u32>::uninit();

        let status = unsafe {
            datalove_rt::c::dtlv_rti_tensor_get_local(
                rt,
                &tensor as *const rtdt::Tensor as *const u8,
                &*tensor_tydesc as *const rtdt::TyDesc,
                indices.as_ptr(),
                element_value.as_mut_ptr() as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let mut value = unsafe { element_value.assume_init() };
        assert_eq!(value, expected_value);

        // Destroy the cloned element.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                rt,
                &mut value as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Destroy tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test setting elements in a 2D tensor with column-major layout.
#[test]
fn test_tensor_set_2d_col_major() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a 2x3 tensor in column-major order.
    let slice_data: Vec<u32> = vec![10, 40, 20, 50, 30, 60];
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[2, 3], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: std::ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: std::ptr::null(),
        strides: std::ptr::null(),
        layout: rtdt::TensorLayout::ColMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Set some elements.
    let new_value = 999u32;
    let indices = [1, 2];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_set_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            indices.as_ptr(),
            &new_value as *const u32 as *const u8,
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify the update.
    let mut element_value = std::mem::MaybeUninit::<u32>::uninit();
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_get_local(
            rt,
            &tensor as *const rtdt::Tensor as *const u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            indices.as_ptr(),
            element_value.as_mut_ptr() as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut value = unsafe { element_value.assume_init() };
    assert_eq!(value, 999);

    // Destroy the cloned element.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            &mut value as *mut u32 as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test setting elements in a 3D tensor.
#[test]
fn test_tensor_set_3d() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(3);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a 2x2x3 tensor.
    let slice_data: Vec<u32> = (0..12).collect();
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[2, 2, 3], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: std::ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: std::ptr::null(),
        strides: std::ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Set some elements.
    let updates = [
        ([0, 0, 0], 1000u32),
        ([1, 1, 2], 1112u32),
        ([0, 1, 1], 111u32),
    ];

    for (indices, new_value) in updates {
        let status = unsafe {
            datalove_rt::c::dtlv_rti_tensor_set_local(
                rt,
                &mut tensor as *mut rtdt::Tensor as *mut u8,
                &*tensor_tydesc as *const rtdt::TyDesc,
                indices.as_ptr(),
                &new_value as *const u32 as *const u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Verify updates.
    for (indices, expected_value) in updates {
        let mut element_value = std::mem::MaybeUninit::<u32>::uninit();

        let status = unsafe {
            datalove_rt::c::dtlv_rti_tensor_get_local(
                rt,
                &tensor as *const rtdt::Tensor as *const u8,
                &*tensor_tydesc as *const rtdt::TyDesc,
                indices.as_ptr(),
                element_value.as_mut_ptr() as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let mut value = unsafe { element_value.assume_init() };
        assert_eq!(value, expected_value);

        // Destroy the cloned element.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                rt,
                &mut value as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Destroy tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test tensor_set with out of bounds indices.
#[test]
fn test_tensor_set_out_of_bounds() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a 2x3 tensor.
    let slice_data: Vec<u32> = vec![10, 20, 30, 40, 50, 60];
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[2, 3], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: std::ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: std::ptr::null(),
        strides: std::ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Test out of bounds accesses.
    let invalid_indices = [
        [2, 0],  // First dimension out of bounds.
        [0, 3],  // Second dimension out of bounds.
        [2, 3],  // Both out of bounds.
    ];

    let new_value = 999u32;
    for indices in invalid_indices {
        let status = unsafe {
            datalove_rt::c::dtlv_rti_tensor_set_local(
                rt,
                &mut tensor as *mut rtdt::Tensor as *mut u8,
                &*tensor_tydesc as *const rtdt::TyDesc,
                indices.as_ptr(),
                &new_value as *const u32 as *const u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Error);
    }

    // Destroy tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test tensor_set with null pointers.
#[test]
fn test_tensor_set_null_pointers() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(1);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a valid tensor.
    let slice_data: Vec<u32> = vec![10, 20, 30];
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[3], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: std::ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: std::ptr::null(),
        strides: std::ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let indices = [0];
    let new_value = 999u32;

    // Null tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_set_local(
            rt,
            std::ptr::null_mut(),
            &*tensor_tydesc as *const rtdt::TyDesc,
            indices.as_ptr(),
            &new_value as *const u32 as *const u8,
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Null indices.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_set_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            std::ptr::null(),
            &new_value as *const u32 as *const u8,
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Null value pointer.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_set_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            indices.as_ptr(),
            std::ptr::null(),
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Destroy tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// Tensor Transpose Tests
// ============================================================================

/// Test basic 2D transpose with row-major layout.
#[test]
fn test_tensor_transpose_2d_row_major() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a 2x3 tensor in row-major order:
    // [[10, 20, 30],
    //  [40, 50, 60]]
    let slice_data: Vec<u32> = vec![10, 20, 30, 40, 50, 60];
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Transpose [1, 0] to get 3x2 tensor:
    // [[10, 40],
    //  [20, 50],
    //  [30, 60]]
    let perm = [1u32, 0];
    let mut transposed = rtdt::Tensor {
        ptr_base: ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: ptr::null(),
        strides: ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_transpose_local(
            rt,
            &tensor as *const rtdt::Tensor as *const u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            perm.as_ptr(),
            &mut transposed as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify input tensor is cleared (ownership transferred).
    assert!(tensor.ptr_base.is_null());
    assert!(tensor.shape.is_null());
    assert!(tensor.strides.is_null());

    // Verify transposed shape is [3, 2].
    let shape = unsafe { std::slice::from_raw_parts(transposed.shape, 2) };
    assert_eq!(shape, &[3, 2]);

    // Verify transposed strides are [1, 3] (swapped from [3, 1]).
    let strides = unsafe { std::slice::from_raw_parts(transposed.strides, 2) };
    assert_eq!(strides, &[1, 3]);

    // Verify layout changed to ColMajorTransposed.
    assert_eq!(transposed.layout, rtdt::TensorLayout::ColMajorTransposed);

    // Verify data via tensor_get.
    let test_cases = [
        ([0, 0], 10),
        ([0, 1], 40),
        ([1, 0], 20),
        ([1, 1], 50),
        ([2, 0], 30),
        ([2, 1], 60),
    ];

    for (indices, expected) in test_cases {
        let mut element_value = std::mem::MaybeUninit::<u32>::uninit();
        let status = unsafe {
            datalove_rt::c::dtlv_rti_tensor_get_local(
                rt,
                &transposed as *const rtdt::Tensor as *const u8,
                &*tensor_tydesc as *const rtdt::TyDesc,
                indices.as_ptr(),
                element_value.as_mut_ptr() as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let mut value = unsafe { element_value.assume_init() };
        assert_eq!(value, expected);

        // Destroy the cloned element.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                rt,
                &mut value as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut transposed as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test 2D transpose with column-major layout.
#[test]
fn test_tensor_transpose_2d_col_major() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a 2x3 tensor in column-major order.
    // Memory layout: [10, 40, 20, 50, 30, 60]
    let slice_data: Vec<u32> = vec![10, 40, 20, 50, 30, 60];
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Transpose.
    let perm = [1u32, 0];
    let mut transposed = rtdt::Tensor {
        ptr_base: ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: ptr::null(),
        strides: ptr::null(),
        layout: rtdt::TensorLayout::ColMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_transpose_local(
            rt,
            &tensor as *const rtdt::Tensor as *const u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            perm.as_ptr(),
            &mut transposed as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify transposed shape is [3, 2].
    let shape = unsafe { std::slice::from_raw_parts(transposed.shape, 2) };
    assert_eq!(shape, &[3, 2]);

    // Verify transposed strides are [2, 1] (swapped from [1, 2]).
    let strides = unsafe { std::slice::from_raw_parts(transposed.strides, 2) };
    assert_eq!(strides, &[2, 1]);

    // Verify layout changed to RowMajorTransposed.
    assert_eq!(transposed.layout, rtdt::TensorLayout::RowMajorTransposed);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut transposed as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test 3D transpose with permutation [2, 0, 1].
#[test]
fn test_tensor_transpose_3d() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(3);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a 2x3x4 tensor.
    let slice_data: Vec<u32> = (0..24).collect();
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Transpose with permutation [2, 0, 1] to get 4x2x3 tensor.
    let perm = [2u32, 0, 1];
    let mut transposed = rtdt::Tensor {
        ptr_base: ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: ptr::null(),
        strides: ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_transpose_local(
            rt,
            &tensor as *const rtdt::Tensor as *const u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            perm.as_ptr(),
            &mut transposed as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify transposed shape is [4, 2, 3].
    let shape = unsafe { std::slice::from_raw_parts(transposed.shape, 3) };
    assert_eq!(shape, &[4, 2, 3]);

    // Verify transposed strides are [1, 12, 4] (permutation of [12, 4, 1]).
    let strides = unsafe { std::slice::from_raw_parts(transposed.strides, 3) };
    assert_eq!(strides, &[1, 12, 4]);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut transposed as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test identity permutation (no-op transpose).
#[test]
fn test_tensor_transpose_identity() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a 2x3 tensor.
    let slice_data: Vec<u32> = vec![10, 20, 30, 40, 50, 60];
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Identity permutation [0, 1].
    let perm = [0u32, 1];
    let mut transposed = rtdt::Tensor {
        ptr_base: ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: ptr::null(),
        strides: ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_transpose_local(
            rt,
            &tensor as *const rtdt::Tensor as *const u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            perm.as_ptr(),
            &mut transposed as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify shape is unchanged [2, 3].
    let shape = unsafe { std::slice::from_raw_parts(transposed.shape, 2) };
    assert_eq!(shape, &[2, 3]);

    // Verify strides are unchanged [3, 1].
    let strides = unsafe { std::slice::from_raw_parts(transposed.strides, 2) };
    assert_eq!(strides, &[3, 1]);

    // Layout should be unchanged (not a standard transpose).
    assert_eq!(transposed.layout, rtdt::TensorLayout::RowMajor);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut transposed as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test transpose with invalid permutation (out of bounds).
#[test]
fn test_tensor_transpose_invalid_permutation_out_of_bounds() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a 2x3 tensor.
    let slice_data: Vec<u32> = vec![10, 20, 30, 40, 50, 60];
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Invalid permutation with out-of-bounds index.
    let perm = [0u32, 2];  // 2 is out of bounds for rank 2.
    let mut transposed = rtdt::Tensor {
        ptr_base: ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: ptr::null(),
        strides: ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_transpose_local(
            rt,
            &tensor as *const rtdt::Tensor as *const u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            perm.as_ptr(),
            &mut transposed as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Original tensor should still be valid.
    assert!(!tensor.ptr_base.is_null());

    // Clean up original tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test transpose with invalid permutation (duplicate indices).
#[test]
fn test_tensor_transpose_invalid_permutation_duplicates() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a 2x3 tensor.
    let slice_data: Vec<u32> = vec![10, 20, 30, 40, 50, 60];
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Invalid permutation with duplicate index.
    let perm = [0u32, 0];
    let mut transposed = rtdt::Tensor {
        ptr_base: ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: ptr::null(),
        strides: ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_transpose_local(
            rt,
            &tensor as *const rtdt::Tensor as *const u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            perm.as_ptr(),
            &mut transposed as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test transpose with null pointers.
#[test]
fn test_tensor_transpose_null_pointers() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a valid tensor.
    let slice_data: Vec<u32> = vec![10, 20, 30, 40, 50, 60];
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let perm = [1u32, 0];
    let mut transposed = rtdt::Tensor {
        ptr_base: ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: ptr::null(),
        strides: ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    // Null input tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_transpose_local(
            rt,
            ptr::null(),
            &*tensor_tydesc as *const rtdt::TyDesc,
            perm.as_ptr(),
            &mut transposed as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Null permutation.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_transpose_local(
            rt,
            &tensor as *const rtdt::Tensor as *const u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            ptr::null(),
            &mut transposed as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Null output tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_transpose_local(
            rt,
            &tensor as *const rtdt::Tensor as *const u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            perm.as_ptr(),
            ptr::null_mut(),
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test that tensor_get clones elements and caller destroys them.
#[test]
fn test_tensor_get_clones_and_caller_destroys() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(1);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create a 1D tensor: [10, 20, 30]
    let slice_data: Vec<u32> = vec![10, 20, 30];
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[3], &*list_tydesc, &*list_element_tydesc)
    };

    let mut tensor = rtdt::Tensor {
        ptr_base: std::ptr::null_mut(),
        offset_elems: 0,
        capacity_elems: 0,
        shape: std::ptr::null(),
        strides: std::ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Get element at index 1 (value = 20).
    let indices = [1];
    let mut element_value = std::mem::MaybeUninit::<u32>::uninit();

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_get_local(
            rt,
            &tensor as *const rtdt::Tensor as *const u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            indices.as_ptr(),
            element_value.as_mut_ptr() as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify the cloned value.
    let mut value = unsafe { element_value.assume_init() };
    assert_eq!(value, 20);

    // Caller is responsible for destroying the cloned element.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            &mut value as *mut u32 as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Get another element to demonstrate the pattern multiple times.
    let indices = [2];
    let mut element_value = std::mem::MaybeUninit::<u32>::uninit();

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_get_local(
            rt,
            &tensor as *const rtdt::Tensor as *const u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            indices.as_ptr(),
            element_value.as_mut_ptr() as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut value = unsafe { element_value.assume_init() };
    assert_eq!(value, 30);

    // Again, caller destroys the cloned element.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            &mut value as *mut u32 as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy tensor.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// Tensor Slice Tests
// ============================================================================

/// Helper to create Result<Tensor<u32, N>, Error> type descriptor.
fn create_result_tensor_u32_tydesc(rank: u32) -> (Box<rtdt::TyDesc>, Box<rtdt::TyDesc>, Box<rtdt::TyDesc>) {
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

    // Result<Tensor, Error> type descriptor.
    let tensor_align = std::mem::align_of::<rtdt::Tensor>() as u32;
    let error_align = std::mem::align_of::<rtdt::Error>() as u32;
    let payload_align = tensor_align.max(error_align);
    let tag_size = 1u32;
    let payload_offset = rtdt::layout::align_up(tag_size, payload_align);
    let payload_size = (std::mem::size_of::<rtdt::Tensor>().max(std::mem::size_of::<rtdt::Error>())) as u32;
    let total_size = rtdt::layout::align_up(payload_offset + payload_size, payload_align);

    let result_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Result,
        size: total_size,
        align: payload_align,
        type_info: rtdt::TyInfo {
            result: rtdt::TyInfoResult {
                ok_tydesc: &*tensor_tydesc as *const rtdt::TyDesc,
            },
        },
    });

    (result_tydesc, tensor_tydesc, element_tydesc)
}

/// Test slicing a 2D tensor to get a subregion.
#[test]
fn test_tensor_slice_2d_valid() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create 4x5 tensor: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, ...]
    let slice_data: Vec<u32> = (0..20).collect();
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[4, 5], &*list_tydesc, &*list_element_tydesc)
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Slice [1:3, 2:5] to get a 2x3 subregion.
    let ranges = vec![
        rtdt::SliceRange { start: 1, end: 3 },
        rtdt::SliceRange { start: 2, end: 5 },
    ];

    let (result_tydesc, _tensor_tydesc_for_result, _) = create_result_tensor_u32_tydesc(2);

    // Allocate result buffer.
    let result_size = result_tydesc.size as usize;
    let mut result_buffer = vec![0u8; result_size];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_slice_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            ranges.as_ptr(),
            result_buffer.as_mut_ptr(),
            &*result_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check result tag is Ok.
    let result_ptr = result_buffer.as_ptr() as *const rtdt::Result;
    let result_tag = unsafe { (*result_ptr).tag };
    assert_eq!(result_tag, rtdt::ResultTag::Ok);

    // Extract sliced tensor from result.
    let tensor_align = std::mem::align_of::<rtdt::Tensor>() as u32;
    let payload_offset = rtdt::layout::result_payload_offset(tensor_align);
    let sliced_tensor_ptr = unsafe {
        result_buffer.as_ptr().add(payload_offset as usize) as *const rtdt::Tensor
    };
    let sliced_tensor = unsafe { &*sliced_tensor_ptr };

    // Verify sliced tensor properties.
    assert!(!sliced_tensor.ptr_base.is_null());
    assert_eq!(sliced_tensor.offset_elems, 1 * 5 + 2); // offset = 7
    assert_eq!(sliced_tensor.capacity_elems, 20);
    assert!(!sliced_tensor.shape.is_null());
    assert!(!sliced_tensor.strides.is_null());

    // Verify new shape: [2, 3]
    let shape = unsafe { std::slice::from_raw_parts(sliced_tensor.shape, 2) };
    assert_eq!(shape[0], 2);
    assert_eq!(shape[1], 3);

    // Verify strides unchanged: [5, 1]
    let strides = unsafe { std::slice::from_raw_parts(sliced_tensor.strides, 2) };
    assert_eq!(strides[0], 5);
    assert_eq!(strides[1], 1);

    // Destroy sliced tensor.
    let mut sliced_tensor_copy = unsafe { ptr::read(sliced_tensor) };
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut sliced_tensor_copy as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test slicing with full range (identity operation).
#[test]
fn test_tensor_slice_2d_full_range() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    let slice_data: Vec<u32> = (0..6).collect();
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Slice with full range [0:2, 0:3].
    let ranges = vec![
        rtdt::SliceRange { start: 0, end: 2 },
        rtdt::SliceRange { start: 0, end: 3 },
    ];

    let (result_tydesc, _tensor_tydesc_for_result, _) = create_result_tensor_u32_tydesc(2);
    let result_size = result_tydesc.size as usize;
    let mut result_buffer = vec![0u8; result_size];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_slice_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            ranges.as_ptr(),
            result_buffer.as_mut_ptr(),
            &*result_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check result is Ok.
    let result_ptr = result_buffer.as_ptr() as *const rtdt::Result;
    assert_eq!(unsafe { (*result_ptr).tag }, rtdt::ResultTag::Ok);

    // Extract tensor.
    let tensor_align = std::mem::align_of::<rtdt::Tensor>() as u32;
    let payload_offset = rtdt::layout::result_payload_offset(tensor_align);
    let sliced_tensor = unsafe {
        &*(result_buffer.as_ptr().add(payload_offset as usize) as *const rtdt::Tensor)
    };

    // Verify shape unchanged: [2, 3]
    let shape = unsafe { std::slice::from_raw_parts(sliced_tensor.shape, 2) };
    assert_eq!(shape, &[2, 3]);

    // Verify offset is still 0.
    assert_eq!(sliced_tensor.offset_elems, 0);

    // Cleanup.
    let mut sliced_tensor_copy = unsafe { ptr::read(sliced_tensor) };
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut sliced_tensor_copy as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test slicing to a single element.
#[test]
fn test_tensor_slice_2d_single_element() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    let slice_data: Vec<u32> = (0..12).collect();
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[3, 4], &*list_tydesc, &*list_element_tydesc)
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Slice to single element [1:2, 2:3].
    let ranges = vec![
        rtdt::SliceRange { start: 1, end: 2 },
        rtdt::SliceRange { start: 2, end: 3 },
    ];

    let (result_tydesc, _tensor_tydesc_for_result, _) = create_result_tensor_u32_tydesc(2);
    let result_size = result_tydesc.size as usize;
    let mut result_buffer = vec![0u8; result_size];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_slice_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            ranges.as_ptr(),
            result_buffer.as_mut_ptr(),
            &*result_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let result_ptr = result_buffer.as_ptr() as *const rtdt::Result;
    assert_eq!(unsafe { (*result_ptr).tag }, rtdt::ResultTag::Ok);

    let tensor_align = std::mem::align_of::<rtdt::Tensor>() as u32;
    let payload_offset = rtdt::layout::result_payload_offset(tensor_align);
    let sliced_tensor = unsafe {
        &*(result_buffer.as_ptr().add(payload_offset as usize) as *const rtdt::Tensor)
    };

    // Verify shape: [1, 1]
    let shape = unsafe { std::slice::from_raw_parts(sliced_tensor.shape, 2) };
    assert_eq!(shape, &[1, 1]);

    // Verify offset: 1*4 + 2 = 6
    assert_eq!(sliced_tensor.offset_elems, 6);

    // Cleanup.
    let mut sliced_tensor_copy = unsafe { ptr::read(sliced_tensor) };
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut sliced_tensor_copy as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test slicing with invalid range (start >= end).
#[test]
fn test_tensor_slice_2d_invalid_range_start_ge_end() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    let slice_data: Vec<u32> = (0..6).collect();
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Invalid range: start >= end.
    let ranges = vec![
        rtdt::SliceRange { start: 1, end: 1 },
        rtdt::SliceRange { start: 0, end: 3 },
    ];

    let (result_tydesc, _tensor_tydesc_for_result, _) = create_result_tensor_u32_tydesc(2);
    let result_size = result_tydesc.size as usize;
    let mut result_buffer = vec![0u8; result_size];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_slice_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            ranges.as_ptr(),
            result_buffer.as_mut_ptr(),
            &*result_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check result tag is Err.
    let result_ptr = result_buffer.as_ptr() as *const rtdt::Result;
    assert_eq!(unsafe { (*result_ptr).tag }, rtdt::ResultTag::Err);

    // Extract error payload which contains the tensor directly.
    let tensor_align = std::mem::align_of::<rtdt::Tensor>() as u32;
    let payload_offset = rtdt::layout::result_payload_offset(tensor_align);

    // Error contains tensor value - extract and destroy it.
    let error_tensor_ptr = unsafe {
        result_buffer.as_ptr().add(payload_offset as usize) as *const rtdt::Tensor
    };
    let error_tensor = unsafe { &*error_tensor_ptr };

    // Verify it's the original tensor.
    assert!(!error_tensor.ptr_base.is_null());
    assert_eq!(error_tensor.offset_elems, 0);

    // Destroy the tensor from the error.
    let mut error_tensor_copy = unsafe { ptr::read(error_tensor) };
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut error_tensor_copy as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test slicing with out of bounds range (end > dimension).
#[test]
fn test_tensor_slice_2d_out_of_bounds() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    let slice_data: Vec<u32> = (0..6).collect();
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Out of bounds: end > dimension size.
    let ranges = vec![
        rtdt::SliceRange { start: 0, end: 2 },
        rtdt::SliceRange { start: 0, end: 5 }, // 5 > 3
    ];

    let (result_tydesc, _tensor_tydesc_for_result, _) = create_result_tensor_u32_tydesc(2);
    let result_size = result_tydesc.size as usize;
    let mut result_buffer = vec![0u8; result_size];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_slice_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            ranges.as_ptr(),
            result_buffer.as_mut_ptr(),
            &*result_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify result is Err.
    let result_ptr = result_buffer.as_ptr() as *const rtdt::Result;
    assert_eq!(unsafe { (*result_ptr).tag }, rtdt::ResultTag::Err);

    // Extract and destroy the tensor from error payload.
    let tensor_align = std::mem::align_of::<rtdt::Tensor>() as u32;
    let payload_offset = rtdt::layout::result_payload_offset(tensor_align);
    let error_tensor_ptr = unsafe {
        result_buffer.as_ptr().add(payload_offset as usize) as *const rtdt::Tensor
    };
    let mut error_tensor = unsafe { ptr::read(error_tensor_ptr) };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut error_tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test slicing a 3D tensor.
#[test]
fn test_tensor_slice_3d() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(3);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create 2x3x4 tensor.
    let slice_data: Vec<u32> = (0..24).collect();
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Slice [0:2, 1:3, 1:3] to get a 2x2x2 subregion.
    let ranges = vec![
        rtdt::SliceRange { start: 0, end: 2 },
        rtdt::SliceRange { start: 1, end: 3 },
        rtdt::SliceRange { start: 1, end: 3 },
    ];

    let (result_tydesc, _tensor_tydesc_for_result, _) = create_result_tensor_u32_tydesc(3);
    let result_size = result_tydesc.size as usize;
    let mut result_buffer = vec![0u8; result_size];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_slice_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            ranges.as_ptr(),
            result_buffer.as_mut_ptr(),
            &*result_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let result_ptr = result_buffer.as_ptr() as *const rtdt::Result;
    assert_eq!(unsafe { (*result_ptr).tag }, rtdt::ResultTag::Ok);

    let tensor_align = std::mem::align_of::<rtdt::Tensor>() as u32;
    let payload_offset = rtdt::layout::result_payload_offset(tensor_align);
    let sliced_tensor = unsafe {
        &*(result_buffer.as_ptr().add(payload_offset as usize) as *const rtdt::Tensor)
    };

    // Verify shape: [2, 2, 2]
    let shape = unsafe { std::slice::from_raw_parts(sliced_tensor.shape, 3) };
    assert_eq!(shape, &[2, 2, 2]);

    // Verify offset: 0*12 + 1*4 + 1 = 5
    assert_eq!(sliced_tensor.offset_elems, 5);

    // Verify strides unchanged: [12, 4, 1]
    let strides = unsafe { std::slice::from_raw_parts(sliced_tensor.strides, 3) };
    assert_eq!(strides, &[12, 4, 1]);

    // Cleanup.
    let mut sliced_tensor_copy = unsafe { ptr::read(sliced_tensor) };
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut sliced_tensor_copy as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// Tensor Reshape Tests
// ============================================================================

#[test]
fn test_tensor_reshape_2d_to_3d() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc_2d, element_tydesc) = create_tensor_u32_tydesc(2);
    let (tensor_tydesc_3d, _element_tydesc_3d) = create_tensor_u32_tydesc(3);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create [6, 10] row-major tensor (60 elements).
    let slice_data: Vec<u32> = (0..60).collect();
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[6, 10], &*list_tydesc, &*list_element_tydesc)
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
            rt,
            slice_data.as_ptr() as *const u8,
            slice_data.len() as u32,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut shape_list as *mut rtdt::List as *mut u8,
            &*list_tydesc as *const rtdt::TyDesc,
            rtdt::TensorLayout::RowMajor as u8,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc_2d as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Create new shape [3, 4, 5] (also 60 elements).
    let mut new_shape_list = unsafe {
        create_runtime_u32_list(rt, &[3, 4, 5], &*list_tydesc, &*list_element_tydesc)
    };

    // Create Result type descriptor.
    let (result_tydesc, _tensor_tydesc_for_result, _) = create_result_tensor_u32_tydesc(3);
    let result_size = result_tydesc.size as usize;
    let mut result_buffer = vec![0u8; result_size];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_reshape_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc_2d as *const rtdt::TyDesc,
            &mut new_shape_list as *mut rtdt::List as *mut u8,
            &*list_tydesc as *const rtdt::TyDesc,
            result_buffer.as_mut_ptr(),
            &*result_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check result tag is Ok.
    let result_ptr = result_buffer.as_ptr() as *const rtdt::Result;
    let result_tag = unsafe { (*result_ptr).tag };
    assert_eq!(result_tag, rtdt::ResultTag::Ok);

    // Extract reshaped tensor from result.
    let tensor_align = std::mem::align_of::<rtdt::Tensor>() as u32;
    let payload_offset = rtdt::layout::result_payload_offset(tensor_align);
    let reshaped_tensor_ptr = unsafe {
        result_buffer.as_ptr().add(payload_offset as usize) as *const rtdt::Tensor
    };
    let reshaped_tensor = unsafe { &*reshaped_tensor_ptr };

    // Verify new shape: [3, 4, 5]
    let shape = unsafe { std::slice::from_raw_parts(reshaped_tensor.shape, 3) };
    assert_eq!(shape, &[3, 4, 5]);

    // Verify strides (row-major: [20, 5, 1]).
    let strides = unsafe { std::slice::from_raw_parts(reshaped_tensor.strides, 3) };
    assert_eq!(strides, &[20, 5, 1]);

    // Verify offset is 0.
    assert_eq!(reshaped_tensor.offset_elems, 0);

    // Verify layout preserved.
    assert_eq!(reshaped_tensor.layout, rtdt::TensorLayout::RowMajor);

    // Clean up.
    let mut reshaped_tensor_copy = unsafe { ptr::read(reshaped_tensor) };
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut reshaped_tensor_copy as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc_3d as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_tensor_reshape_error_size_mismatch() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create [6, 10] row-major tensor (60 elements).
    let slice_data: Vec<u32> = (0..60).collect();
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[6, 10], &*list_tydesc, &*list_element_tydesc)
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Try to reshape to [7, 9] (63 elements, mismatch).
    let mut new_shape_list = unsafe {
        create_runtime_u32_list(rt, &[7, 9], &*list_tydesc, &*list_element_tydesc)
    };

    let (result_tydesc, _tensor_tydesc_for_result, _) = create_result_tensor_u32_tydesc(2);
    let result_size = result_tydesc.size as usize;
    let mut result_buffer = vec![0u8; result_size];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_reshape_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            &mut new_shape_list as *mut rtdt::List as *mut u8,
            &*list_tydesc as *const rtdt::TyDesc,
            result_buffer.as_mut_ptr(),
            &*result_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Result should be Err.
    let result_ptr = result_buffer.as_ptr() as *const rtdt::Result;
    let result_tag = unsafe { (*result_ptr).tag };
    assert_eq!(result_tag, rtdt::ResultTag::Err);

    // Extract and verify the original tensor from error.
    let tensor_align = std::mem::align_of::<rtdt::Tensor>() as u32;
    let payload_offset = rtdt::layout::result_payload_offset(tensor_align);
    let error_tensor_ptr = unsafe {
        result_buffer.as_ptr().add(payload_offset as usize) as *const rtdt::Tensor
    };
    let error_tensor = unsafe { &*error_tensor_ptr };

    // Verify it's the original tensor with shape [6, 10].
    let shape = unsafe { std::slice::from_raw_parts(error_tensor.shape, 2) };
    assert_eq!(shape, &[6, 10]);

    // Clean up.
    let mut error_tensor_copy = unsafe { ptr::read(error_tensor) };
    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut error_tensor_copy as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_tensor_reshape_error_non_contiguous() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (tensor_tydesc, element_tydesc) = create_tensor_u32_tydesc(2);
    let (list_tydesc, list_element_tydesc) = create_list_u32_tydesc();

    // Create [10, 20] row-major tensor.
    let slice_data: Vec<u32> = (0..200).collect();
    let mut shape_list = unsafe {
        create_runtime_u32_list(rt, &[10, 20], &*list_tydesc, &*list_element_tydesc)
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
        datalove_rt::c::dtlv_rti_tensor_create_from_slice_local(
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
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Slice the tensor to make it non-contiguous.
    let ranges = vec![
        rtdt::SliceRange { start: 2, end: 7 },
        rtdt::SliceRange { start: 5, end: 15 },
    ];

    let (slice_result_tydesc, _tensor_tydesc_for_result, _) = create_result_tensor_u32_tydesc(2);
    let slice_result_size = slice_result_tydesc.size as usize;
    let mut slice_result_buffer = vec![0u8; slice_result_size];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_slice_local(
            rt,
            &mut tensor as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            ranges.as_ptr(),
            slice_result_buffer.as_mut_ptr(),
            &*slice_result_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let slice_result_ptr = slice_result_buffer.as_ptr() as *const rtdt::Result;
    let slice_tag = unsafe { (*slice_result_ptr).tag };
    assert_eq!(slice_tag, rtdt::ResultTag::Ok);

    let tensor_align = std::mem::align_of::<rtdt::Tensor>() as u32;
    let slice_payload_offset = rtdt::layout::result_payload_offset(tensor_align);
    let sliced_tensor_ptr = unsafe {
        slice_result_buffer.as_ptr().add(slice_payload_offset as usize) as *const rtdt::Tensor
    };
    let sliced_tensor = unsafe { &*sliced_tensor_ptr };

    // Try to reshape the sliced (non-contiguous) tensor.
    let mut new_shape_list = unsafe {
        create_runtime_u32_list(rt, &[50], &*list_tydesc, &*list_element_tydesc)
    };

    let (reshape_result_tydesc, _tensor_tydesc_for_result2, _) = create_result_tensor_u32_tydesc(1);
    let reshape_result_size = reshape_result_tydesc.size as usize;
    let mut reshape_result_buffer = vec![0u8; reshape_result_size];

    // Copy sliced tensor to mutable location.
    let mut sliced_tensor_copy = unsafe { ptr::read(sliced_tensor) };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_reshape_local(
            rt,
            &mut sliced_tensor_copy as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
            &mut new_shape_list as *mut rtdt::List as *mut u8,
            &*list_tydesc as *const rtdt::TyDesc,
            reshape_result_buffer.as_mut_ptr(),
            &*reshape_result_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Result should be Err (non-contiguous).
    let reshape_result_ptr = reshape_result_buffer.as_ptr() as *const rtdt::Result;
    let reshape_tag = unsafe { (*reshape_result_ptr).tag };
    assert_eq!(reshape_tag, rtdt::ResultTag::Err);

    // Clean up error tensor.
    let error_payload_offset = rtdt::layout::result_payload_offset(tensor_align);
    let error_tensor_ptr = unsafe {
        reshape_result_buffer.as_ptr().add(error_payload_offset as usize) as *const rtdt::Tensor
    };
    let mut error_tensor_copy = unsafe { ptr::read(error_tensor_ptr) };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_tensor_destroy_local(
            rt,
            &mut error_tensor_copy as *mut rtdt::Tensor as *mut u8,
            &*tensor_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}
