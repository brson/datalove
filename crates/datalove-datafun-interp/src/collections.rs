//! Collection creation operations for the IR interpreter.
//!
//! Handles list, set, map, and tensor construction.

use datalove_datafun_ir::Operand;
use datalove_rtdt as rtdt;
use datalove_rtdt::TyDescRef;

use crate::error::InterpError;
use crate::frame::{Frame, FrameStore};
use crate::value::{Destination, Value};
use crate::IrInterpreter;

impl IrInterpreter {
    /// Execute ListNew: create a list from operands.
    pub(crate) fn execute_list_new(
        &mut self,
        elements: &[Operand],
        dest: Destination,
        frame: &Frame,
        frames: &FrameStore,
    ) -> Result<(), InterpError> {
        use datalove_rt::c::RtStatus;

        let rt_handle = self.runtime.handle();
        let list_ptr = dest.ptr;
        let list_tydesc = dest.tydesc;

        // Get element tydesc from list tydesc.
        let list_tydesc_ref = unsafe { TyDescRef::from_ptr(list_tydesc) };
        let element_tydesc = list_tydesc_ref.list_element_ty().as_ptr();
        let element_size = unsafe { (*element_tydesc).size as usize };

        // Create empty list at dest.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_create_local(rt_handle, list_ptr, list_tydesc)
        };
        if status != RtStatus::Ok {
            return Err(InterpError::RuntimeError(
                "Failed to create list".to_string(),
            ));
        }

        if elements.is_empty() {
            return Ok(());
        }

        // Reserve capacity for all elements.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_reserve_local(
                rt_handle,
                list_ptr,
                list_tydesc,
                elements.len() as rtdt::UsizeRepr,
            )
        };
        if status != RtStatus::Ok {
            unsafe {
                datalove_rt::c::dtlv_rti_list_destroy_local(rt_handle, list_ptr, list_tydesc);
            }
            return Err(InterpError::RuntimeError(
                "Failed to reserve list capacity".to_string(),
            ));
        }

        // Copy each element into the list's data buffer.
        for (i, elem_op) in elements.iter().enumerate() {
            let elem_val = self.read_operand(elem_op, frame, frames)?;

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
                (*list).size = rtdt::Usize((i + 1) as rtdt::UsizeRepr);
            }
        }

        Ok(())
    }

    /// Execute SetNew: create a set from operands.
    pub(crate) fn execute_set_new(
        &mut self,
        elements: &[Operand],
        dest: Destination,
        frame: &Frame,
        frames: &FrameStore,
    ) -> Result<(), InterpError> {
        use datalove_rt::c::RtStatus;

        let rt_handle = self.runtime.handle();
        let set_ptr = dest.ptr;
        let set_tydesc = dest.tydesc;

        // Get element tydesc from set tydesc.
        let set_tydesc_ref = unsafe { TyDescRef::from_ptr(set_tydesc) };
        let element_tydesc = set_tydesc_ref.set_element_ty().as_ptr();
        let element_size = unsafe { (*element_tydesc).size as usize };
        let element_align = unsafe { (*element_tydesc).align };

        if elements.is_empty() {
            // Create empty set.
            let status = unsafe {
                datalove_rt::c::dtlv_rti_btreeset_create_local(rt_handle, set_ptr, set_tydesc)
            };
            if status != RtStatus::Ok {
                return Err(InterpError::RuntimeError(
                    "Failed to create empty set".to_string(),
                ));
            }
            return Ok(());
        }

        // Read all element values.
        let mut elem_values: Vec<Value> = elements
            .iter()
            .map(|op| self.read_operand(op, frame, frames))
            .collect::<Result<_, _>>()?;

        // Sort elements using runtime comparison.
        elem_values.sort_by(|a, b| unsafe {
            let result = datalove_rt::c::dtlv_rti_cmp_total_local(
                rt_handle,
                a.ptr,
                element_tydesc,
                b.ptr,
                element_tydesc,
            );
            match result {
                datalove_rt::c::RtOrdering::Less => std::cmp::Ordering::Less,
                datalove_rt::c::RtOrdering::Greater => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            }
        });

        // Allocate temporary buffer for sorted elements.
        let buffer_size = (elem_values.len() * element_size) as u32;
        let buffer = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt_handle, buffer_size, element_align, 1)
        };
        if buffer.is_null() {
            return Err(InterpError::RuntimeError(
                "Failed to allocate set buffer".to_string(),
            ));
        }

        // Copy elements into buffer.
        for (i, value) in elem_values.iter().enumerate() {
            unsafe {
                let elem_dest = buffer.add(i * element_size);
                std::ptr::copy_nonoverlapping(value.ptr, elem_dest, element_size);
            }
        }

        // Build B-tree from sorted buffer.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_build_from_sorted_slice_local(
                rt_handle,
                set_ptr,
                element_tydesc,
                buffer,
                elem_values.len() as rtdt::UsizeRepr,
            )
        };

        // Free temporary buffer.
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_raw_local(
                rt_handle,
                buffer_size,
                element_align,
                1,
                buffer,
            );
        }

        if status != RtStatus::Ok {
            return Err(InterpError::RuntimeError(
                "Failed to build set B-tree".to_string(),
            ));
        }

        Ok(())
    }

    /// Execute MapNew: create a map from key-value pairs.
    pub(crate) fn execute_map_new(
        &mut self,
        entries: &[(Operand, Operand)],
        dest: Destination,
        frame: &Frame,
        frames: &FrameStore,
    ) -> Result<(), InterpError> {
        use datalove_rt::c::RtStatus;

        let rt_handle = self.runtime.handle();
        let map_ptr = dest.ptr;
        let map_tydesc = dest.tydesc;

        // Get key and value tydescs from map tydesc.
        let map_tydesc_ref = unsafe { TyDescRef::from_ptr(map_tydesc) };
        let key_tydesc = map_tydesc_ref.map_key_ty().as_ptr();
        let value_tydesc = map_tydesc_ref.map_value_ty().as_ptr();
        let key_size = unsafe { (*key_tydesc).size as usize };
        let value_size = unsafe { (*value_tydesc).size as usize };
        let key_align = unsafe { (*key_tydesc).align };
        let value_align = unsafe { (*value_tydesc).align };

        if entries.is_empty() {
            // Create empty map.
            let status = unsafe {
                datalove_rt::c::dtlv_rti_btreemap_create_local(rt_handle, map_ptr, map_tydesc)
            };
            if status != RtStatus::Ok {
                return Err(InterpError::RuntimeError(
                    "Failed to create empty map".to_string(),
                ));
            }
            return Ok(());
        }

        // Read all key-value pairs.
        let mut kv_pairs: Vec<(Value, Value)> = entries
            .iter()
            .map(|(k_op, v_op)| {
                let k = self.read_operand(k_op, frame, frames)?;
                let v = self.read_operand(v_op, frame, frames)?;
                Ok((k, v))
            })
            .collect::<Result<_, InterpError>>()?;

        // Sort by key using runtime comparison.
        kv_pairs.sort_by(|a, b| unsafe {
            let result = datalove_rt::c::dtlv_rti_cmp_total_local(
                rt_handle,
                a.0.ptr,
                key_tydesc,
                b.0.ptr,
                key_tydesc,
            );
            match result {
                datalove_rt::c::RtOrdering::Less => std::cmp::Ordering::Less,
                datalove_rt::c::RtOrdering::Greater => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            }
        });

        // Allocate temporary buffers for keys and values.
        let keys_buffer_size = (kv_pairs.len() * key_size) as u32;
        let values_buffer_size = (kv_pairs.len() * value_size) as u32;

        let keys_buffer = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt_handle, keys_buffer_size, key_align, 1)
        };
        if keys_buffer.is_null() {
            return Err(InterpError::RuntimeError(
                "Failed to allocate keys buffer".to_string(),
            ));
        }

        let values_buffer = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                rt_handle,
                values_buffer_size,
                value_align,
                1,
            )
        };
        if values_buffer.is_null() {
            unsafe {
                datalove_rt::c::dtlv_rti_mem_free_raw_local(
                    rt_handle,
                    keys_buffer_size,
                    key_align,
                    1,
                    keys_buffer,
                );
            }
            return Err(InterpError::RuntimeError(
                "Failed to allocate values buffer".to_string(),
            ));
        }

        // Copy keys and values into buffers.
        for (i, (key, value)) in kv_pairs.iter().enumerate() {
            unsafe {
                let key_dest = keys_buffer.add(i * key_size);
                let value_dest = values_buffer.add(i * value_size);
                std::ptr::copy_nonoverlapping(key.ptr, key_dest, key_size);
                std::ptr::copy_nonoverlapping(value.ptr, value_dest, value_size);
            }
        }

        // Build B-tree from sorted slices.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_build_from_sorted_slices_local(
                rt_handle,
                map_ptr,
                key_tydesc,
                value_tydesc,
                keys_buffer,
                values_buffer,
                kv_pairs.len() as rtdt::UsizeRepr,
            )
        };

        // Free temporary buffers.
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_raw_local(
                rt_handle,
                keys_buffer_size,
                key_align,
                1,
                keys_buffer,
            );
            datalove_rt::c::dtlv_rti_mem_free_raw_local(
                rt_handle,
                values_buffer_size,
                value_align,
                1,
                values_buffer,
            );
        }

        if status != RtStatus::Ok {
            return Err(InterpError::RuntimeError(
                "Failed to build map B-tree".to_string(),
            ));
        }

        Ok(())
    }

    /// Execute TensorNew: create a tensor from shape and elements.
    pub(crate) fn execute_tensor_new(
        &mut self,
        shape: &[u32],
        elements: &[Operand],
        dest: Destination,
        frame: &Frame,
        frames: &FrameStore,
    ) -> Result<(), InterpError> {
        let rt_handle = self.runtime.handle();
        let tensor_ptr = dest.ptr;
        let tensor_tydesc = dest.tydesc;

        // Get element tydesc from tensor tydesc.
        let tensor_tydesc_ref = unsafe { TyDescRef::from_ptr(tensor_tydesc) };
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
                    total_elems as rtdt::UsizeRepr,
                )
            };
            if array_ptr.is_null() {
                return Err(InterpError::RuntimeError(
                    "Failed to allocate tensor data".to_string(),
                ));
            }

            // Copy each element into the data buffer.
            for (i, elem_op) in elements.iter().enumerate() {
                let elem_val = self.read_operand(elem_op, frame, frames)?;
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
                    rank as rtdt::UsizeRepr,
                ) as *mut rtdt::UsizeRepr
            };
            if shape_array.is_null() {
                // Cleanup data if allocated.
                if !data_ptr.is_null() {
                    unsafe {
                        datalove_rt::c::dtlv_rti_mem_free_local(
                            rt_handle,
                            element_tydesc,
                            total_elems as rtdt::UsizeRepr,
                            data_ptr,
                        );
                    }
                }
                return Err(InterpError::RuntimeError(
                    "Failed to allocate tensor shape".to_string(),
                ));
            }
            for (i, &dim) in shape.iter().enumerate() {
                unsafe {
                    *shape_array.add(i) = dim as rtdt::UsizeRepr;
                }
            }
            shape_array as *const rtdt::Usize
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
                    rank as rtdt::UsizeRepr,
                ) as *mut rtdt::UsizeRepr
            };
            if strides_array.is_null() {
                // Cleanup.
                if !data_ptr.is_null() {
                    unsafe {
                        datalove_rt::c::dtlv_rti_mem_free_local(
                            rt_handle,
                            element_tydesc,
                            total_elems as rtdt::UsizeRepr,
                            data_ptr,
                        );
                    }
                }
                if !shape_ptr.is_null() {
                    unsafe {
                        datalove_rt::c::dtlv_rti_mem_free_raw_local(
                            rt_handle,
                            (rank as u32) * rtdt::INDEX_SIZE,
                            rtdt::INDEX_ALIGN,
                            1,
                            shape_ptr as *mut u8,
                        );
                    }
                }
                return Err(InterpError::RuntimeError(
                    "Failed to allocate tensor strides".to_string(),
                ));
            }

            // Compute strides for row-major layout.
            for i in 0..rank {
                let stride = shape[i + 1..rank].iter().map(|&d| d as rtdt::UsizeRepr).product::<rtdt::UsizeRepr>();
                unsafe {
                    *strides_array.add(i) = if stride == 0 { 1 } else { stride };
                }
            }

            strides_array as *const rtdt::Usize
        } else {
            std::ptr::null()
        };

        // Fill in the Tensor struct.
        unsafe {
            let tensor = tensor_ptr as *mut rtdt::Tensor;
            (*tensor).ptr_base = data_ptr;
            (*tensor).capacity_elems = rtdt::Usize(total_elems as rtdt::UsizeRepr);
            (*tensor).offset_elems = rtdt::Usize::ZERO;
            (*tensor).shape = shape_ptr;
            (*tensor).strides = strides_ptr;
            (*tensor).layout = rtdt::TensorLayout::RowMajor;
        }

        Ok(())
    }

    /// Execute TableNew: create a table from row tuples.
    pub(crate) fn execute_table_new(
        &mut self,
        rows: &[Operand],
        dest: Destination,
        frame: &Frame,
        frames: &FrameStore,
    ) -> Result<(), InterpError> {
        use datalove_rt::c::RtStatus;

        let rt_handle = self.runtime.handle();
        let table_ptr = dest.ptr;
        let table_tydesc = dest.tydesc;

        // Create empty table at dest.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_table_create_local(rt_handle, table_ptr, table_tydesc)
        };
        if status != RtStatus::Ok {
            return Err(InterpError::RuntimeError(
                "Failed to create table".to_string(),
            ));
        }

        // Push each row (rows are tuples).
        for row_op in rows {
            let row_val = self.read_operand(row_op, frame, frames)?;

            let status = unsafe {
                datalove_rt::c::dtlv_rti_table_push_row_local(
                    rt_handle,
                    table_ptr,
                    table_tydesc,
                    row_val.ptr,
                    row_val.tydesc,
                )
            };
            if status != RtStatus::Ok {
                unsafe {
                    datalove_rt::c::dtlv_rti_table_destroy_local(
                        rt_handle,
                        table_ptr,
                        table_tydesc,
                    );
                }
                return Err(InterpError::RuntimeError(
                    "Failed to push row to table".to_string(),
                ));
            }
        }

        Ok(())
    }
}
