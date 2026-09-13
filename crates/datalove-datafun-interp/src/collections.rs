//! Collection creation operations for the IR interpreter.
//!
//! Handles list, set, map, tensor, and table construction.
//! All runtime calls are infallible.

use datalove_datafun_ir::Operand;
use datalove_rtdt as rtdt;
use datalove_rtdt::TyDescRef;

use crate::frame::{Frame, FrameStore};
use crate::value::Destination;
use crate::IrInterpreter;

impl IrInterpreter {
    /// Execute ListNew: create a list from operands.
    /// Build a list described by `list_tydesc`, then move it into the wrapper.
    pub(crate) fn execute_list_new_erased(
        &mut self,
        elements: &[Operand],
        dest: Destination,
        list_tydesc: *const rtdt::TyDesc,
        frame: &Frame,
        frames: &FrameStore,
    ) {
        let rt_handle = self.runtime.handle();
        let mut temp = std::mem::MaybeUninit::<rtdt::List>::uninit();
        let temp_ptr = temp.as_mut_ptr() as *mut u8;
        unsafe {
            datalove_rt::c::dtlv_rti_list_create_local(rt_handle, temp_ptr, list_tydesc);
        }
        // Pushed rather than copied in. The list holds its elements as what
        // they really are, and an element here is in the erased shape -- a
        // `data` for a bare type parameter, a tuple of them for a tuple of
        // parameters -- so it has to be converted on the way in. The copy the
        // concrete path does would put a wrapper where a value belongs.
        for elem_op in elements {
            let elem = self.read_operand(elem_op, frame, frames);
            unsafe {
                datalove_rt::c::dtlv_rti_list_push_erased_local(
                    rt_handle, temp_ptr, list_tydesc, elem.ptr, elem.tydesc);
            }
        }
        let status = unsafe {
            datalove_rt::c::dtlv_rti_data_from_local(
                self.runtime.handle(), temp_ptr, list_tydesc, dest.ptr)
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok,
            "wrapping a freshly built list");
    }

    pub(crate) fn execute_list_new(
        &mut self,
        elements: &[Operand],
        dest: Destination,
        frame: &Frame,
        frames: &FrameStore,
    ) {
        let rt_handle = self.runtime.handle();
        let list_ptr = dest.ptr;
        let list_tydesc = dest.tydesc;

        // Get element tydesc from list tydesc.
        let list_tydesc_ref = unsafe { TyDescRef::from_ptr(list_tydesc) };
        let element_tydesc = list_tydesc_ref.list_element_ty().as_ptr();
        let element_size = unsafe { (*element_tydesc).size as usize };

        // Create empty list at dest.
        unsafe {
            datalove_rt::c::dtlv_rti_list_create_local(rt_handle, list_ptr, list_tydesc);
        }

        if elements.is_empty() {
            return;
        }

        // Reserve capacity for all elements.
        unsafe {
            datalove_rt::c::dtlv_rti_list_reserve_local(
                rt_handle,
                list_ptr,
                list_tydesc,
                elements.len() as rtdt::IndexRepr,
            );
        }

        // Copy each element into the list's data buffer.
        for (i, elem_op) in elements.iter().enumerate() {
            let elem_val = self.read_operand(elem_op, frame, frames);

            // Get pointer to element slot in list's data buffer.
            let data_ptr = unsafe { (*(list_ptr as *const rtdt::List)).data as *mut u8 };
            let elem_dest_ptr = unsafe { data_ptr.add(i * element_size) };

            // Copy element value into list.
            unsafe {
                std::ptr::copy_nonoverlapping(elem_val.ptr, elem_dest_ptr, element_size);
            }

            // Update list size.
            unsafe {
                let list = list_ptr as *mut rtdt::List;
                (*list).size = rtdt::Index((i + 1) as rtdt::IndexRepr);
            }
        }
    }

    /// Execute SetNew: create a set from operands.
    /// Build a set described by `set_tydesc`, then move it into the wrapper.
    ///
    /// The same two steps as the list: a set built over a type parameter has a
    /// `data` for its destination, so it is made in a temporary the descriptor
    /// describes and moved in.
    pub(crate) fn execute_set_new_erased(
        &mut self,
        elements: &[Operand],
        dest: Destination,
        set_tydesc: *const rtdt::TyDesc,
        frame: &Frame,
        frames: &FrameStore,
    ) {
        let rt_handle = self.runtime.handle();
        let mut temp = std::mem::MaybeUninit::<rtdt::Set>::uninit();
        let temp_ptr = temp.as_mut_ptr() as *mut u8;
        unsafe {
            datalove_rt::c::dtlv_rti_btreeset_create_local(rt_handle, temp_ptr, set_tydesc);
        }
        // Inserted rather than copied in; see `execute_list_new_erased`.
        for elem_op in elements {
            let elem = self.read_operand(elem_op, frame, frames);
            let mut added = false;
            unsafe {
                datalove_rt::c::dtlv_rti_btreeset_insert_erased_local(
                    rt_handle, temp_ptr, set_tydesc, elem.ptr, elem.tydesc,
                    &mut added as *mut bool as *mut u8);
            }
        }
        let status = unsafe {
            datalove_rt::c::dtlv_rti_data_from_local(
                self.runtime.handle(), temp_ptr, set_tydesc, dest.ptr)
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok,
            "wrapping a freshly built set");
    }

    pub(crate) fn execute_set_new(
        &mut self,
        elements: &[Operand],
        dest: Destination,
        frame: &Frame,
        frames: &FrameStore,
    ) {
        let rt_handle = self.runtime.handle();

        // Inserted one at a time rather than sorted and built in bulk.
        //
        // A set holds each element once, and building from a sorted run takes
        // the run as given: two equal elements written in a literal both went
        // in and the set came out holding a duplicate, which is not a set. The
        // insert is what knows an element is already there, and what destroys
        // the one it did not take.
        unsafe {
            datalove_rt::c::dtlv_rti_btreeset_create_local(rt_handle, dest.ptr, dest.tydesc);
        }
        for elem_op in elements {
            let elem = self.read_operand(elem_op, frame, frames);
            let mut added = false;
            unsafe {
                datalove_rt::c::dtlv_rti_btreeset_insert_local(
                    rt_handle, dest.ptr, dest.tydesc, elem.ptr, elem.tydesc,
                    &mut added as *mut bool as *mut u8);
            }
        }
    }

    /// Execute MapNew: create a map from key-value pairs.
    /// Build a map described by `map_tydesc`, then move it into the wrapper.
    pub(crate) fn execute_map_new_erased(
        &mut self,
        entries: &[(Operand, Operand)],
        dest: Destination,
        map_tydesc: *const rtdt::TyDesc,
        frame: &Frame,
        frames: &FrameStore,
    ) {
        let rt_handle = self.runtime.handle();
        let mut temp = std::mem::MaybeUninit::<rtdt::Map>::uninit();
        let temp_ptr = temp.as_mut_ptr() as *mut u8;
        unsafe {
            datalove_rt::c::dtlv_rti_btreemap_create_local(rt_handle, temp_ptr, map_tydesc);
        }
        // Inserted rather than copied in; see `execute_list_new_erased`.
        for (key_op, val_op) in entries {
            let key = self.read_operand(key_op, frame, frames);
            let val = self.read_operand(val_op, frame, frames);
            unsafe {
                datalove_rt::c::dtlv_rti_btreemap_insert_erased_local(
                    rt_handle, temp_ptr, map_tydesc,
                    key.ptr, key.tydesc, val.ptr, val.tydesc);
            }
        }
        let status = unsafe {
            datalove_rt::c::dtlv_rti_data_from_local(
                self.runtime.handle(), temp_ptr, map_tydesc, dest.ptr)
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok,
            "wrapping a freshly built map");
    }

    pub(crate) fn execute_map_new(
        &mut self,
        entries: &[(Operand, Operand)],
        dest: Destination,
        frame: &Frame,
        frames: &FrameStore,
    ) {
        let rt_handle = self.runtime.handle();

        // Inserted one at a time rather than sorted and built in bulk; see
        // `execute_set_new`. A map holds each key once, and a literal naming
        // one twice built a map with the key in it twice. The insert is what
        // knows the key is there already, and what lets go of the value it
        // replaced.
        unsafe {
            datalove_rt::c::dtlv_rti_btreemap_create_local(rt_handle, dest.ptr, dest.tydesc);
        }
        for (key_op, val_op) in entries {
            let key = self.read_operand(key_op, frame, frames);
            let val = self.read_operand(val_op, frame, frames);
            unsafe {
                datalove_rt::c::dtlv_rti_btreemap_insert_local(
                    rt_handle, dest.ptr, dest.tydesc,
                    key.ptr, key.tydesc, val.ptr, val.tydesc);
            }
        }
    }

    /// Execute TensorNew: create a tensor from shape and elements.
    pub(crate) fn execute_tensor_new(
        &mut self,
        shape: &[u32],
        elements: &[Operand],
        dest: Destination,
        frame: &Frame,
        frames: &FrameStore,
    ) {
        let rt_handle = self.runtime.handle();
        let tensor_ptr = dest.ptr;

        // Get element tydesc from tensor tydesc.
        let tensor_tydesc_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
        let element_tydesc = tensor_tydesc_ref.tensor_element_ty().as_ptr();
        let element_size = unsafe { (*element_tydesc).size as usize };

        let rank = shape.len();
        let total_elems: usize = shape.iter().map(|&d| d as usize).product();

        // Allocate tensor data array.
        let data_ptr = if total_elems > 0 {
            let array_ptr = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_local(
                    rt_handle,
                    element_tydesc,
                    total_elems as rtdt::IndexRepr,
                )
            };

            // Copy each element into the data buffer.
            for (i, elem_op) in elements.iter().enumerate() {
                let elem_val = self.read_operand(elem_op, frame, frames);
                let elem_dest = unsafe { array_ptr.add(i * element_size) };
                unsafe {
                    std::ptr::copy_nonoverlapping(elem_val.ptr, elem_dest, element_size);
                }
            }
            array_ptr
        } else {
            std::ptr::null_mut()
        };

        // Allocate shape array.
        let shape_ptr = if rank > 0 {
            let shape_array = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                    rt_handle,
                    rtdt::INDEX_SIZE,
                    rtdt::INDEX_ALIGN,
                    rank as rtdt::IndexRepr,
                ) as *mut rtdt::IndexRepr
            };
            for (i, &dim) in shape.iter().enumerate() {
                unsafe {
                    *shape_array.add(i) = dim as rtdt::IndexRepr;
                }
            }
            shape_array as *const rtdt::Index
        } else {
            std::ptr::null()
        };

        // Allocate and compute strides array.
        let strides_ptr = if rank > 0 {
            let strides_array = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                    rt_handle,
                    rtdt::INDEX_SIZE,
                    rtdt::INDEX_ALIGN,
                    rank as rtdt::IndexRepr,
                ) as *mut rtdt::IndexRepr
            };

            // Compute strides for row-major layout.
            for i in 0..rank {
                let stride = shape[i + 1..rank].iter().map(|&d| d as rtdt::IndexRepr).product::<rtdt::IndexRepr>();
                unsafe {
                    *strides_array.add(i) = if stride == 0 { 1 } else { stride };
                }
            }

            strides_array as *const rtdt::Index
        } else {
            std::ptr::null()
        };

        // Fill in the Tensor struct.
        unsafe {
            let tensor = tensor_ptr as *mut rtdt::Tensor;
            (*tensor).ptr_base = data_ptr;
            (*tensor).capacity_elems = rtdt::Index(total_elems as rtdt::IndexRepr);
            (*tensor).offset_elems = rtdt::Index::ZERO;
            (*tensor).shape = shape_ptr;
            (*tensor).strides = strides_ptr;
            (*tensor).layout = rtdt::TensorLayout::RowMajor;
        }
    }

    /// Execute TableNew: create a table from row tuples.
    pub(crate) fn execute_table_new(
        &mut self,
        rows: &[Operand],
        dest: Destination,
        frame: &Frame,
        frames: &FrameStore,
    ) {
        let rt_handle = self.runtime.handle();
        let table_ptr = dest.ptr;
        let table_tydesc = dest.tydesc;

        // Create empty table at dest.
        unsafe {
            datalove_rt::c::dtlv_rti_table_create_local(rt_handle, table_ptr, table_tydesc);
        }

        // Push each row (rows are tuples).
        for row_op in rows {
            let row_val = self.read_operand(row_op, frame, frames);

            unsafe {
                datalove_rt::c::dtlv_rti_table_push_row_local(
                    rt_handle,
                    table_ptr,
                    table_tydesc,
                    row_val.ptr,
                    row_val.tydesc,
                );
            }
        }
    }
}
