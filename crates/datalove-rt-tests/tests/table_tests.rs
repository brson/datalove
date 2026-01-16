//! Tests for table runtime functions.

use datalove_rtdt as rtdt;
use std::cell::RefCell;
use std::ptr;

// ============================================================================
// Type Descriptor Arena
// ============================================================================

/// Arena for allocating type descriptors in tests.
struct TyDescArena {
    ptrs: RefCell<Vec<*mut rtdt::TyDesc>>,
    // Keep column info arrays alive.
    col_arrays: RefCell<Vec<Vec<rtdt::TyInfoTableColumn>>>,
    // Keep column name strings alive.
    col_names: RefCell<Vec<String>>,
}

impl TyDescArena {
    fn new() -> Self {
        Self {
            ptrs: RefCell::new(Vec::new()),
            col_arrays: RefCell::new(Vec::new()),
            col_names: RefCell::new(Vec::new()),
        }
    }

    fn alloc(&self, td: rtdt::TyDesc) -> *const rtdt::TyDesc {
        let ptr = Box::into_raw(Box::new(td));
        self.ptrs.borrow_mut().push(ptr);
        ptr
    }

    fn alloc_col_name(&self, name: &str) -> (*const u8, u32) {
        let s = name.to_string();
        let ptr = s.as_ptr();
        let len = s.len() as u32;
        self.col_names.borrow_mut().push(s);
        (ptr, len)
    }

    fn alloc_col_array(&self, cols: Vec<rtdt::TyInfoTableColumn>) -> *const rtdt::TyInfoTableColumn {
        let ptr = cols.as_ptr();
        self.col_arrays.borrow_mut().push(cols);
        ptr
    }
}

impl Drop for TyDescArena {
    fn drop(&mut self) {
        for &ptr in self.ptrs.borrow().iter() {
            unsafe { drop(Box::from_raw(ptr)); }
        }
    }
}

// ============================================================================
// Test Helper Functions
// ============================================================================

fn create_u32_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::U32,
        size: 4,
        align: 4,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing,
        },
    })
}

fn create_u64_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::U64,
        size: 8,
        align: 8,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing,
        },
    })
}

fn create_string_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::String,
        size: std::mem::size_of::<rtdt::String>() as u32,
        align: std::mem::align_of::<rtdt::String>() as u32,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing,
        },
    })
}

/// Create a Table<u32> type descriptor (single column named "x").
fn create_table_u32_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    let col_tydesc = create_u32_tydesc(arena);
    let (name_ptr, name_len) = arena.alloc_col_name("x");
    let columns = arena.alloc_col_array(vec![
        rtdt::TyInfoTableColumn {
            name: name_ptr,
            name_len,
            tydesc: col_tydesc,
        },
    ]);

    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Table,
        size: std::mem::size_of::<rtdt::Table>() as u32,
        align: std::mem::align_of::<rtdt::Table>() as u32,
        type_info: rtdt::TyInfo {
            table: rtdt::TyInfoTable {
                num_columns: 1,
                columns,
            },
        },
    })
}

/// Create a Table<u32, u64> type descriptor (two columns named "a", "b").
fn create_table_u32_u64_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    let col0_tydesc = create_u32_tydesc(arena);
    let col1_tydesc = create_u64_tydesc(arena);
    let (name0_ptr, name0_len) = arena.alloc_col_name("a");
    let (name1_ptr, name1_len) = arena.alloc_col_name("b");
    let columns = arena.alloc_col_array(vec![
        rtdt::TyInfoTableColumn {
            name: name0_ptr,
            name_len: name0_len,
            tydesc: col0_tydesc,
        },
        rtdt::TyInfoTableColumn {
            name: name1_ptr,
            name_len: name1_len,
            tydesc: col1_tydesc,
        },
    ]);

    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Table,
        size: std::mem::size_of::<rtdt::Table>() as u32,
        align: std::mem::align_of::<rtdt::Table>() as u32,
        type_info: rtdt::TyInfo {
            table: rtdt::TyInfoTable {
                num_columns: 2,
                columns,
            },
        },
    })
}

/// Create a tuple (u32,) type descriptor for pushing single-column rows.
fn create_tuple_u32_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    let u32_tydesc = create_u32_tydesc(arena);

    // Allocate fields array.
    let fields = Box::new([
        rtdt::TyInfoTupleField {
            offset: 0,
            tydesc: u32_tydesc,
        },
    ]);
    let fields_ptr = Box::into_raw(fields) as *const rtdt::TyInfoTupleField;

    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Tuple,
        size: 4,
        align: 4,
        type_info: rtdt::TyInfo {
            tuple: rtdt::TyInfoTuple {
                num_fields: 1,
                fields: fields_ptr,
            },
        },
    })
}

/// Create a tuple (u32, u64) type descriptor for pushing two-column rows.
fn create_tuple_u32_u64_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    let u32_tydesc = create_u32_tydesc(arena);
    let u64_tydesc = create_u64_tydesc(arena);

    // Layout: u32 at offset 0, padding, u64 at offset 8.
    let fields = Box::new([
        rtdt::TyInfoTupleField {
            offset: 0,
            tydesc: u32_tydesc,
        },
        rtdt::TyInfoTupleField {
            offset: 8,
            tydesc: u64_tydesc,
        },
    ]);
    let fields_ptr = Box::into_raw(fields) as *const rtdt::TyInfoTupleField;

    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Tuple,
        size: 16,
        align: 8,
        type_info: rtdt::TyInfo {
            tuple: rtdt::TyInfoTuple {
                num_fields: 2,
                fields: fields_ptr,
            },
        },
    })
}

// ============================================================================
// Basic Tests
// ============================================================================

#[test]
fn test_table_create_empty() {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let table_tydesc = create_table_u32_tydesc(&arena);

    let mut table = rtdt::Table {
        len: 0,
        capacity: 0,
        data: ptr::null(),
    };
    let table_ptr = &mut table as *mut rtdt::Table as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_table_create_local(rt, table_ptr, table_tydesc)
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert_eq!(table.len, 0);
    assert_eq!(table.capacity, 0);
    assert!(table.data.is_null());

    // Destroy.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_table_destroy_local(rt, table_ptr, table_tydesc)
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
}

#[test]
fn test_table_push_and_len() {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let table_tydesc = create_table_u32_tydesc(&arena);
    let row_tydesc = create_tuple_u32_tydesc(&arena);

    let mut table = rtdt::Table {
        len: 0,
        capacity: 0,
        data: ptr::null(),
    };
    let table_ptr = &mut table as *mut rtdt::Table as *mut u8;

    // Create table.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_table_create_local(rt, table_ptr, table_tydesc)
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push three rows.
    for i in 0u32..3 {
        #[repr(C)]
        struct Row { val: u32 }
        let row = Row { val: i * 10 };

        let status = unsafe {
            datalove_rt::c::dtlv_rti_table_push_row_local(
                rt,
                table_ptr,
                table_tydesc,
                &row as *const Row as *const u8,
                row_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Check length.
    let len = unsafe { datalove_rt::c::dtlv_rti_table_len(rt, table_ptr as *const u8, table_tydesc) };
    assert_eq!(len, 3);
    assert_eq!(table.len, 3);

    // Verify elements via get.
    for i in 0u32..3 {
        let elem_ptr = unsafe {
            datalove_rt::c::dtlv_rti_table_get_local(
                rt,
                table_ptr as *const u8,
                table_tydesc,
                i,
                0,
            )
        };
        assert!(!elem_ptr.is_null());
        let val = unsafe { *(elem_ptr as *const u32) };
        assert_eq!(val, i * 10);
    }

    // Destroy.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_table_destroy_local(rt, table_ptr, table_tydesc)
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
}

#[test]
fn test_table_two_columns() {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let table_tydesc = create_table_u32_u64_tydesc(&arena);
    let row_tydesc = create_tuple_u32_u64_tydesc(&arena);

    let mut table = rtdt::Table {
        len: 0,
        capacity: 0,
        data: ptr::null(),
    };
    let table_ptr = &mut table as *mut rtdt::Table as *mut u8;

    // Create table.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_table_create_local(rt, table_ptr, table_tydesc)
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push rows.
    #[repr(C)]
    struct Row { col0: u32, _pad: u32, col1: u64 }

    for i in 0u32..5 {
        let row = Row { col0: i, _pad: 0, col1: (i as u64) * 100 };

        let status = unsafe {
            datalove_rt::c::dtlv_rti_table_push_row_local(
                rt,
                table_ptr,
                table_tydesc,
                &row as *const Row as *const u8,
                row_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(table.len, 5);

    // Verify elements.
    for i in 0u32..5 {
        let col0_ptr = unsafe {
            datalove_rt::c::dtlv_rti_table_get_local(rt, table_ptr as *const u8, table_tydesc, i, 0)
        };
        let col1_ptr = unsafe {
            datalove_rt::c::dtlv_rti_table_get_local(rt, table_ptr as *const u8, table_tydesc, i, 1)
        };
        assert!(!col0_ptr.is_null());
        assert!(!col1_ptr.is_null());

        let col0_val = unsafe { *(col0_ptr as *const u32) };
        let col1_val = unsafe { *(col1_ptr as *const u64) };
        assert_eq!(col0_val, i);
        assert_eq!(col1_val, (i as u64) * 100);
    }

    // Destroy.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_table_destroy_local(rt, table_ptr, table_tydesc)
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
}

#[test]
fn test_table_get_out_of_bounds() {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let table_tydesc = create_table_u32_tydesc(&arena);
    let row_tydesc = create_tuple_u32_tydesc(&arena);

    let mut table = rtdt::Table {
        len: 0,
        capacity: 0,
        data: ptr::null(),
    };
    let table_ptr = &mut table as *mut rtdt::Table as *mut u8;

    unsafe { datalove_rt::c::dtlv_rti_table_create_local(rt, table_ptr, table_tydesc) };

    // Push one row.
    #[repr(C)]
    struct Row { val: u32 }
    let row = Row { val: 42 };
    unsafe {
        datalove_rt::c::dtlv_rti_table_push_row_local(
            rt,
            table_ptr,
            table_tydesc,
            &row as *const Row as *const u8,
            row_tydesc,
        )
    };

    // Valid access.
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_table_get_local(rt, table_ptr as *const u8, table_tydesc, 0, 0)
    };
    assert!(!ptr.is_null());

    // Out of bounds row.
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_table_get_local(rt, table_ptr as *const u8, table_tydesc, 1, 0)
    };
    assert!(ptr.is_null());

    // Out of bounds column.
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_table_get_local(rt, table_ptr as *const u8, table_tydesc, 0, 1)
    };
    assert!(ptr.is_null());

    unsafe { datalove_rt::c::dtlv_rti_table_destroy_local(rt, table_ptr, table_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
}

#[test]
fn test_table_clear() {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let table_tydesc = create_table_u32_tydesc(&arena);
    let row_tydesc = create_tuple_u32_tydesc(&arena);

    let mut table = rtdt::Table {
        len: 0,
        capacity: 0,
        data: ptr::null(),
    };
    let table_ptr = &mut table as *mut rtdt::Table as *mut u8;

    unsafe { datalove_rt::c::dtlv_rti_table_create_local(rt, table_ptr, table_tydesc) };

    // Push rows.
    #[repr(C)]
    struct Row { val: u32 }
    for i in 0..5 {
        let row = Row { val: i };
        unsafe {
            datalove_rt::c::dtlv_rti_table_push_row_local(
                rt, table_ptr, table_tydesc,
                &row as *const Row as *const u8, row_tydesc,
            )
        };
    }
    assert_eq!(table.len, 5);
    let old_capacity = table.capacity;

    // Clear.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_table_clear_local(rt, table_ptr, table_tydesc)
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Length is 0 but capacity preserved.
    assert_eq!(table.len, 0);
    assert_eq!(table.capacity, old_capacity);

    unsafe { datalove_rt::c::dtlv_rti_table_destroy_local(rt, table_ptr, table_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
}

#[test]
fn test_table_clone() {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let table_tydesc = create_table_u32_tydesc(&arena);
    let row_tydesc = create_tuple_u32_tydesc(&arena);

    let mut table = rtdt::Table {
        len: 0,
        capacity: 0,
        data: ptr::null(),
    };
    let table_ptr = &mut table as *mut rtdt::Table as *mut u8;

    unsafe { datalove_rt::c::dtlv_rti_table_create_local(rt, table_ptr, table_tydesc) };

    // Push rows.
    #[repr(C)]
    struct Row { val: u32 }
    for i in 0..3 {
        let row = Row { val: i * 10 };
        unsafe {
            datalove_rt::c::dtlv_rti_table_push_row_local(
                rt, table_ptr, table_tydesc,
                &row as *const Row as *const u8, row_tydesc,
            )
        };
    }

    // Clone.
    let mut cloned = rtdt::Table {
        len: 0,
        capacity: 0,
        data: ptr::null(),
    };
    let cloned_ptr = &mut cloned as *mut rtdt::Table as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            table_ptr as *const u8,
            table_tydesc,
            cloned_ptr,
            table_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify clone has same data.
    assert_eq!(cloned.len, table.len);
    assert_ne!(cloned.data, table.data); // Different allocation.

    for i in 0..3 {
        let elem_ptr = unsafe {
            datalove_rt::c::dtlv_rti_table_get_local(rt, cloned_ptr as *const u8, table_tydesc, i, 0)
        };
        let val = unsafe { *(elem_ptr as *const u32) };
        assert_eq!(val, i * 10);
    }

    // Destroy both.
    unsafe { datalove_rt::c::dtlv_rti_table_destroy_local(rt, table_ptr, table_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_table_destroy_local(rt, cloned_ptr, table_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
}

#[test]
fn test_table_eq() {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let table_tydesc = create_table_u32_tydesc(&arena);
    let row_tydesc = create_tuple_u32_tydesc(&arena);

    // Create two tables with same data.
    let mut table1 = rtdt::Table { len: 0, capacity: 0, data: ptr::null() };
    let mut table2 = rtdt::Table { len: 0, capacity: 0, data: ptr::null() };
    let table1_ptr = &mut table1 as *mut rtdt::Table as *mut u8;
    let table2_ptr = &mut table2 as *mut rtdt::Table as *mut u8;

    unsafe { datalove_rt::c::dtlv_rti_table_create_local(rt, table1_ptr, table_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_table_create_local(rt, table2_ptr, table_tydesc) };

    #[repr(C)]
    struct Row { val: u32 }
    for i in 0..3 {
        let row = Row { val: i };
        unsafe {
            datalove_rt::c::dtlv_rti_table_push_row_local(
                rt, table1_ptr, table_tydesc,
                &row as *const Row as *const u8, row_tydesc,
            );
            datalove_rt::c::dtlv_rti_table_push_row_local(
                rt, table2_ptr, table_tydesc,
                &row as *const Row as *const u8, row_tydesc,
            );
        };
    }

    // Should be equal.
    let eq = unsafe {
        datalove_rt::c::dtlv_rti_eq_local(
            rt,
            table1_ptr as *const u8, table_tydesc,
            table2_ptr as *const u8, table_tydesc,
        )
    };
    assert_eq!(eq, datalove_rt::c::RtEq::Equals);

    // Push another row to table2, now not equal.
    let row = Row { val: 99 };
    unsafe {
        datalove_rt::c::dtlv_rti_table_push_row_local(
            rt, table2_ptr, table_tydesc,
            &row as *const Row as *const u8, row_tydesc,
        )
    };

    let eq = unsafe {
        datalove_rt::c::dtlv_rti_eq_local(
            rt,
            table1_ptr as *const u8, table_tydesc,
            table2_ptr as *const u8, table_tydesc,
        )
    };
    assert_eq!(eq, datalove_rt::c::RtEq::NotEquals);

    unsafe { datalove_rt::c::dtlv_rti_table_destroy_local(rt, table1_ptr, table_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_table_destroy_local(rt, table2_ptr, table_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
}

#[test]
fn test_table_cmp() {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let table_tydesc = create_table_u32_tydesc(&arena);
    let row_tydesc = create_tuple_u32_tydesc(&arena);

    let mut table1 = rtdt::Table { len: 0, capacity: 0, data: ptr::null() };
    let mut table2 = rtdt::Table { len: 0, capacity: 0, data: ptr::null() };
    let table1_ptr = &mut table1 as *mut rtdt::Table as *mut u8;
    let table2_ptr = &mut table2 as *mut rtdt::Table as *mut u8;

    unsafe { datalove_rt::c::dtlv_rti_table_create_local(rt, table1_ptr, table_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_table_create_local(rt, table2_ptr, table_tydesc) };

    #[repr(C)]
    struct Row { val: u32 }

    // table1: [1, 2]
    // table2: [1, 3]
    for &v in &[1u32, 2] {
        let row = Row { val: v };
        unsafe {
            datalove_rt::c::dtlv_rti_table_push_row_local(
                rt, table1_ptr, table_tydesc,
                &row as *const Row as *const u8, row_tydesc,
            )
        };
    }
    for &v in &[1u32, 3] {
        let row = Row { val: v };
        unsafe {
            datalove_rt::c::dtlv_rti_table_push_row_local(
                rt, table2_ptr, table_tydesc,
                &row as *const Row as *const u8, row_tydesc,
            )
        };
    }

    // table1 < table2 (because 2 < 3 at row 1).
    let ord = unsafe {
        datalove_rt::c::dtlv_rti_cmp_local(
            rt,
            table1_ptr as *const u8, table_tydesc,
            table2_ptr as *const u8, table_tydesc,
        )
    };
    assert_eq!(ord, datalove_rt::c::RtOrdering::Less);

    unsafe { datalove_rt::c::dtlv_rti_table_destroy_local(rt, table1_ptr, table_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_table_destroy_local(rt, table2_ptr, table_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
}
