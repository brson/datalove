//! Table operations for columnar storage.
//!
//! Tables store data in column-major order: all values for column 0,
//! then all values for column 1, etc. This layout is cache-friendly
//! for column-oriented operations.

use crate::c::{LocalRtHandle, RtStatus};
use crate::impls::rt_local;
use datalove_rtdt as rtdt;

/// Collect column type descriptors from a table tydesc into a Vec.
pub fn collect_column_tydescs<'a>(tydesc: rtdt::TyDescRef<'a>) -> Vec<&'a rtdt::TyDesc> {
    tydesc.table_column_tydescs().map(|col| col.tydesc().as_ref()).collect()
}

/// Compute pointer to an element in a table's columnar data.
///
/// # Safety
/// The caller must ensure data pointer is valid for the given capacity,
/// and that row < capacity and col < num_columns.
#[inline]
pub unsafe fn element_ptr(
    data: *const u8,
    column_tydescs: &[&rtdt::TyDesc],
    row: rtdt::IndexRepr,
    col: usize,
    capacity: rtdt::IndexRepr,
) -> *const u8 {
    let col_offset = rtdt::layout::table_column_offset(column_tydescs, col, capacity);
    let elem_size = column_tydescs[col].size;
    unsafe { data.add(col_offset + (row as usize * elem_size as usize)) }
}

/// Compute mutable pointer to an element in a table's columnar data.
#[inline]
pub unsafe fn element_ptr_mut(
    data: *mut u8,
    column_tydescs: &[&rtdt::TyDesc],
    row: rtdt::IndexRepr,
    col: usize,
    capacity: rtdt::IndexRepr,
) -> *mut u8 {
    unsafe { element_ptr(data, column_tydescs, row, col, capacity) as *mut u8 }
}

/// Create an empty table.
pub unsafe fn table_create_impl(
    _rt: LocalRtHandle,
    value_out: *mut u8,
    _tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        let table = &mut *(value_out as *mut rtdt::Table);
        table.len = rtdt::Index::ZERO;
        table.capacity = rtdt::Index::ZERO;
        table.data = std::ptr::null();
    }
    RtStatus::Ok
}

/// Destroy a table and all its elements.
pub unsafe fn table_destroy_impl(
    rt: LocalRtHandle,
    value_in: *mut u8,
    tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        let table = &*(value_in as *const rtdt::Table);
        let table_ptr = value_in as *mut rtdt::Table;

        // Early return only if no buffer allocated.
        if table.data.is_null() {
            (*table_ptr).len = rtdt::Index::ZERO;
            (*table_ptr).capacity = rtdt::Index::ZERO;
            return RtStatus::Ok;
        }

        let column_tydescs = collect_column_tydescs(tydesc);

        // Destroy elements column by column, row by row.
        for (col, col_info) in tydesc.table_column_tydescs().enumerate() {
            let col_tydesc = col_info.tydesc().as_ptr();
            for row in 0..table.len.0 {
                let elem = element_ptr_mut(
                    table.data as *mut u8,
                    &column_tydescs,
                    row,
                    col,
                    table.capacity.0,
                );
                let status = crate::impls::destroy::any_destroy_local(rt, elem, col_tydesc);
                if status != RtStatus::Ok {
                    return status;
                }
            }
        }

        // Free the data buffer.
        // Re-obtain rt_ref after recursive calls to satisfy Stacked Borrows.
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        // Freed on the strength of the pointer, not of the capacity: a table
        // of zero-sized rows, or of no rows at all, holds a buffer of no bytes,
        // and the allocator hands those out and takes them back like any other.
        let alloc_size = rtdt::layout::table_data_allocation_size(&column_tydescs, table.capacity.0);
        let alloc_align = rtdt::layout::table_data_alignment(&column_tydescs);
        rt_ref.alloc.free(alloc_size, alloc_align, 1, table.data as *mut u8);

        // Clear the table fields.
        (*table_ptr).data = std::ptr::null();
        (*table_ptr).len = rtdt::Index::ZERO;
        (*table_ptr).capacity = rtdt::Index::ZERO;

        RtStatus::Ok
    }
}

/// Push a row to a table.
///
/// The row is passed as a tuple with one field per column, and the tuple is
/// consumed: `TableNew` marks its operands moved and emits no drop for them,
/// so each field is copied into its column rather than cloned there. Cloning
/// left the tuple owning a second copy that nothing would ever free, which for
/// a column holding anything on the heap was one leak per row.
pub unsafe fn table_push_row_impl(
    rt: LocalRtHandle,
    table_mut: *mut u8,
    tydesc: rtdt::TyDescRef,
    row_ref: *const u8,
    row_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        let table = &mut *(table_mut as *mut rtdt::Table);
        let column_tydescs = collect_column_tydescs(tydesc);
        let num_columns = column_tydescs.len();

        // Verify row tuple has correct number of fields.
        debug_assert_eq!(row_tydesc.type_tag(), rtdt::TyTag::Tuple);
        let row_info = row_tydesc.tuple_info();
        debug_assert_eq!(row_info.num_fields() as usize, num_columns);

        // Check if we need to grow.
        if table.len >= table.capacity {
            let new_capacity = if table.capacity == rtdt::Index::ZERO { 4 } else { table.capacity.0 * 2 };
            let status = table_grow(rt, table, &column_tydescs, new_capacity);
            if status != RtStatus::Ok {
                return status;
            }
        }

        // Move each tuple field into the corresponding column position.
        for (col, field) in row_tydesc.iter_tuple_fields().enumerate() {
            let field_ptr = row_ref.add(field.offset() as usize);
            let dst = element_ptr_mut(
                table.data as *mut u8,
                &column_tydescs,
                table.len.0,
                col,
                table.capacity.0,
            );
            let size = field.tydesc().size() as usize;
            std::ptr::copy_nonoverlapping(field_ptr, dst, size);
        }

        table.len += rtdt::Index::ONE;
        RtStatus::Ok
    }
}

/// Grow a table's data buffer to a new capacity.
unsafe fn table_grow(
    rt: LocalRtHandle,
    table: &mut rtdt::Table,
    column_tydescs: &[&rtdt::TyDesc],
    new_capacity: rtdt::IndexRepr,
) -> RtStatus {
    unsafe {
        if column_tydescs.is_empty() {
            // Zero-column table needs no allocation.
            table.capacity = rtdt::Index(new_capacity);
            return RtStatus::Ok;
        }

        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);

        let new_alloc_size = rtdt::layout::table_data_allocation_size(column_tydescs, new_capacity);
        let alloc_align = rtdt::layout::table_data_alignment(column_tydescs);

        let new_data = rt_ref.alloc.alloc(new_alloc_size, alloc_align, 1);
        if new_data.is_null() {
            return RtStatus::Error;
        }

        // Copy existing data column by column if there was old data.
        if !table.data.is_null() {
            if table.len > rtdt::Index::ZERO {
                for col in 0..column_tydescs.len() {
                    let col_tydesc = column_tydescs[col];
                    let elem_size = col_tydesc.size as usize;

                    for row in 0..table.len.0 {
                        let src = element_ptr(table.data, column_tydescs, row, col, table.capacity.0);
                        let dst = element_ptr_mut(new_data, column_tydescs, row, col, new_capacity);
                        std::ptr::copy_nonoverlapping(src, dst, elem_size);
                    }
                }
            }

            // Freed whether or not there were rows to carry over: an empty
            // table still holds a buffer, of no bytes.
            let old_alloc_size = rtdt::layout::table_data_allocation_size(column_tydescs, table.capacity.0);
            rt_ref.alloc.free(old_alloc_size, alloc_align, 1, table.data as *mut u8);
        }

        table.data = new_data;
        table.capacity = rtdt::Index(new_capacity);
        RtStatus::Ok
    }
}

/// Get a pointer to an element at (row, col).
///
/// Returns null if row or col is out of bounds.
pub unsafe fn table_get_element_ptr(
    table_ref: *const u8,
    tydesc: rtdt::TyDescRef,
    row: rtdt::IndexRepr,
    col: u32,
) -> *const u8 {
    unsafe {
        let table = &*(table_ref as *const rtdt::Table);
        let column_tydescs = collect_column_tydescs(tydesc);

        if row >= table.len.0 || col as usize >= column_tydescs.len() {
            return std::ptr::null();
        }

        if table.data.is_null() {
            return std::ptr::null();
        }

        element_ptr(table.data, &column_tydescs, row, col as usize, table.capacity.0)
    }
}

/// Set an element at (row, col).
///
/// Returns Error if row or col is out of bounds.
pub unsafe fn table_set_element(
    rt: LocalRtHandle,
    table_mut: *mut u8,
    tydesc: rtdt::TyDescRef,
    row: rtdt::IndexRepr,
    col: u32,
    value_ref: *const u8,
    value_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        let table = &*(table_mut as *const rtdt::Table);
        let column_tydescs = collect_column_tydescs(tydesc);

        if row >= table.len.0 || col as usize >= column_tydescs.len() {
            return RtStatus::Error;
        }

        if table.data.is_null() {
            return RtStatus::Error;
        }

        let dst = element_ptr_mut(
            table.data as *mut u8,
            &column_tydescs,
            row,
            col as usize,
            table.capacity.0,
        );

        // Destroy old value.
        let col_tydesc = column_tydescs[col as usize];
        let status = crate::impls::destroy::any_destroy_local(rt, dst, col_tydesc);
        if status != RtStatus::Ok {
            return status;
        }

        // Clone new value into place.
        crate::impls::clone::clone_value(rt, value_ref, value_tydesc, dst)
    }
}

/// Clear a table, destroying all elements but keeping capacity.
pub unsafe fn table_clear_impl(
    rt: LocalRtHandle,
    table_mut: *mut u8,
    tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        let table = &*(table_mut as *const rtdt::Table);
        let table_ptr = table_mut as *mut rtdt::Table;

        if table.data.is_null() || table.len == rtdt::Index::ZERO {
            (*table_ptr).len = rtdt::Index::ZERO;
            return RtStatus::Ok;
        }

        let column_tydescs = collect_column_tydescs(tydesc);

        // Destroy elements.
        for (col, col_info) in tydesc.table_column_tydescs().enumerate() {
            let col_tydesc = col_info.tydesc().as_ptr();
            for row in 0..table.len.0 {
                let elem = element_ptr_mut(
                    table.data as *mut u8,
                    &column_tydescs,
                    row,
                    col,
                    table.capacity.0,
                );
                let status = crate::impls::destroy::any_destroy_local(rt, elem, col_tydesc);
                if status != RtStatus::Ok {
                    return status;
                }
            }
        }

        (*table_ptr).len = rtdt::Index::ZERO;
        RtStatus::Ok
    }
}

/// Get the length (number of rows) of a table.
pub unsafe fn table_len(table_ref: *const u8) -> rtdt::Index {
    unsafe {
        let table = &*(table_ref as *const rtdt::Table);
        table.len
    }
}

/// Build a table by moving rows from a contiguous array of row tuples.
///
/// The rows are stored in row-major order (array of tuples) in the source buffer.
/// Elements are moved (not cloned) into the table's columnar storage.
/// After this call, the source buffer is consumed and should not be destroyed.
pub unsafe fn table_build_from_rows_impl(
    rt: LocalRtHandle,
    table_out: *mut u8,
    table_tydesc: rtdt::TyDescRef,
    rows_ptr: *mut u8,
    row_tydesc: rtdt::TyDescRef,
    num_rows: rtdt::IndexRepr,
) -> RtStatus {
    unsafe {
        let table = &mut *(table_out as *mut rtdt::Table);
        let column_tydescs = collect_column_tydescs(table_tydesc);
        let num_columns = column_tydescs.len();

        // Verify row tuple has correct number of fields.
        debug_assert_eq!(row_tydesc.type_tag(), rtdt::TyTag::Tuple);
        let row_info = row_tydesc.tuple_info();
        debug_assert_eq!(row_info.num_fields() as usize, num_columns);

        if num_rows == 0 || num_columns == 0 {
            // Initialize empty table.
            table.len = rtdt::Index::ZERO;
            table.capacity = rtdt::Index::ZERO;
            table.data = std::ptr::null();
            return RtStatus::Ok;
        }

        // Allocate table buffer with exact capacity.
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let alloc_size = rtdt::layout::table_data_allocation_size(&column_tydescs, num_rows);
        let alloc_align = rtdt::layout::table_data_alignment(&column_tydescs);

        // Allocated even where the rows come to nothing, which they do when
        // every column is zero-sized. The allocator answers a request for no
        // bytes with a block like any other, and a table that skipped it was
        // left holding a null buffer with rows in it -- a shape everything
        // that reads a table takes for the empty one, so the rows went
        // missing on the way through a clone or a print. A list of the same
        // zero-sized element has always allocated.
        let data = {
            let ptr = rt_ref.alloc.alloc(alloc_size, alloc_align, 1);
            if ptr.is_null() {
                return RtStatus::Error;
            }
            ptr
        };

        table.data = data;
        table.capacity = rtdt::Index::new(num_rows);

        // Move each row's fields into the columnar storage.
        let row_size = row_tydesc.size() as usize;
        for row in 0..num_rows {
            let row_ptr = rows_ptr.add((row as usize) * row_size);

            for (col, field) in row_tydesc.iter_tuple_fields().enumerate() {
                let field_ptr = row_ptr.add(field.offset() as usize);
                let dst = element_ptr_mut(data, &column_tydescs, row, col, num_rows);
                let elem_size = column_tydescs[col].size as usize;

                // Move (memcpy) the field value - source is consumed.
                std::ptr::copy_nonoverlapping(field_ptr, dst, elem_size);
            }
        }

        table.len = rtdt::Index::new(num_rows);
        RtStatus::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rt() -> Box<rt_local::RtLocal> {
        rt_local::RtLocal::new()
    }

    fn make_u32_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::U32,
            size: 4,
            align: 4,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing { unused: 0 },
            },
        }
    }

    #[test]
    fn test_table_create_empty() {
        let mut rt = make_rt();
        let u32_tydesc = make_u32_tydesc();
        let u32_tydesc_ptr = &u32_tydesc as *const _;

        let col_name = b"x";
        let columns: [rtdt::TyInfoTableColumn; 1] = [
            rtdt::TyInfoTableColumn {
                name: col_name.as_ptr(),
                name_len: col_name.len() as u32,
                tydesc: u32_tydesc_ptr,
            },
        ];
        let table_tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::Table,
            size: std::mem::size_of::<rtdt::Table>() as u32,
            align: std::mem::align_of::<rtdt::Table>() as u32,
            type_info: rtdt::TyInfo {
                table: rtdt::TyInfoTable {
                    num_columns: 1,
                    columns: columns.as_ptr(),
                },
            },
        };

        let mut table = std::mem::MaybeUninit::<rtdt::Table>::uninit();
        let table_ty = unsafe { rtdt::TyDescRef::from_ptr(&table_tydesc) };

        unsafe {
            let status = table_create_impl(
                Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                table.as_mut_ptr() as *mut u8,
                table_ty,
            );
            assert_eq!(status, RtStatus::Ok);

            let table = table.assume_init();
            assert_eq!(table.len, rtdt::Index::ZERO);
            assert_eq!(table.capacity, rtdt::Index::ZERO);
            assert!(table.data.is_null());
        }
    }

    #[test]
    fn test_element_ptr_calculation() {
        let td1 = rtdt::TyDesc {
            type_tag: rtdt::TyTag::U32,
            size: 4,
            align: 4,
            type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
        };
        let td2 = rtdt::TyDesc {
            type_tag: rtdt::TyTag::U64,
            size: 8,
            align: 8,
            type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
        };

        let tydescs: Vec<&rtdt::TyDesc> = vec![&td1, &td2];

        // With capacity 4:
        // Column 0: u32 at offset 0, 4 elements = 16 bytes.
        // Column 1: u64 needs 8-byte alignment, aligns 16 -> 16, starts at 16.
        let base = 0x1000 as *const u8;
        let capacity: rtdt::IndexRepr = 4;

        unsafe {
            // First row, first column.
            let ptr = element_ptr(base, &tydescs, 0, 0, capacity);
            assert_eq!(ptr as usize, 0x1000);

            // Second row, first column.
            let ptr = element_ptr(base, &tydescs, 1, 0, capacity);
            assert_eq!(ptr as usize, 0x1004);

            // First row, second column.
            let ptr = element_ptr(base, &tydescs, 0, 1, capacity);
            assert_eq!(ptr as usize, 0x1010); // 0x1000 + 16

            // Second row, second column.
            let ptr = element_ptr(base, &tydescs, 1, 1, capacity);
            assert_eq!(ptr as usize, 0x1018); // 0x1010 + 8
        }
    }
}
