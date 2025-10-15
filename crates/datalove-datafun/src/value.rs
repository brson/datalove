//! Runtime value representation for the interpreter.
//!
//! Values use rtdt layouts and own their allocations.

use rmx::prelude::*;
use datalove_rt as rt;
use datalove_rtdt as rtdt;

/// A runtime value in the interpreter.
///
/// Values either inline small data or own heap-allocated rtdt pointers.
#[derive(Debug)]
pub enum Value {
    /// Boolean value (inline).
    Bool(bool),

    /// U32 value (inline).
    U32(u32),

    /// F32 value (inline).
    F32(f32),

    /// Bigint (heap-allocated).
    Int {
        ptr: *mut rtdt::Int,
        tydesc: *const rtdt::TyDesc,
    },

    /// String (heap-allocated).
    String {
        ptr: *mut rtdt::String,
        tydesc: *const rtdt::TyDesc,
    },

    /// Tuple (heap-allocated).
    Tuple {
        ptr: *mut u8,
        tydesc: *const rtdt::TyDesc,
    },

    /// Struct (heap-allocated).
    Struct {
        ptr: *mut u8,
        tydesc: *const rtdt::TyDesc,
    },

    /// Enum (heap-allocated).
    Enum {
        ptr: *mut u8,
        tydesc: *const rtdt::TyDesc,
    },

    /// List (heap-allocated).
    List {
        ptr: *mut rtdt::List,
        tydesc: *const rtdt::TyDesc,
    },

    /// Map (heap-allocated).
    Map {
        ptr: *mut rtdt::Map,
        tydesc: *const rtdt::TyDesc,
    },

    /// Set (heap-allocated).
    Set {
        ptr: *mut rtdt::Set,
        tydesc: *const rtdt::TyDesc,
    },

    /// Option (heap-allocated).
    Option {
        ptr: *mut u8,
        tydesc: *const rtdt::TyDesc,
    },

    /// Result (heap-allocated).
    Result {
        ptr: *mut u8,
        tydesc: *const rtdt::TyDesc,
    },

    /// Data (heap-allocated).
    Data {
        ptr: *mut rtdt::Data,
        tydesc: *const rtdt::TyDesc,
    },

    /// Error (heap-allocated).
    Error {
        ptr: *mut rtdt::Error,
        tydesc: *const rtdt::TyDesc,
    },
}

impl Value {
    /// Create a bool value.
    pub fn from_bool(value: bool) -> Self {
        Value::Bool(value)
    }

    /// Create a u32 value.
    pub fn from_u32(value: u32) -> Self {
        Value::U32(value)
    }

    /// Create an f32 value.
    pub fn from_f32(value: f32) -> Self {
        Value::F32(value)
    }

    /// Allocate an Int (bigint) value.
    pub unsafe fn alloc_int(
        rt: &mut rt::alloc::LocalRt,
        tydesc: *const rtdt::TyDesc,
    ) -> Self {
        let size = unsafe { (*tydesc).size };
        let align = unsafe { (*tydesc).align };
        let ptr = unsafe { rt.alloc(size, align, 1) as *mut rtdt::Int };

        Value::Int { ptr, tydesc }
    }

    /// Allocate a String value.
    pub unsafe fn alloc_string(
        rt: &mut rt::alloc::LocalRt,
        tydesc: *const rtdt::TyDesc,
    ) -> Self {
        let size = unsafe { (*tydesc).size };
        let align = unsafe { (*tydesc).align };
        let ptr = unsafe { rt.alloc(size, align, 1) as *mut rtdt::String };

        // Initialize to empty string.
        unsafe {
            (*ptr).data = std::ptr::null();
            (*ptr).size = 0;
            (*ptr).capacity = 0;
        }

        Value::String { ptr, tydesc }
    }

    /// Allocate a Tuple value.
    pub unsafe fn alloc_tuple(
        rt: &mut rt::alloc::LocalRt,
        tydesc: *const rtdt::TyDesc,
    ) -> Self {
        let size = unsafe { (*tydesc).size };
        let align = unsafe { (*tydesc).align };
        let ptr = unsafe { rt.alloc(size, align, 1) };

        Value::Tuple { ptr, tydesc }
    }

    /// Allocate a Struct value.
    pub unsafe fn alloc_struct(
        rt: &mut rt::alloc::LocalRt,
        tydesc: *const rtdt::TyDesc,
    ) -> Self {
        let size = unsafe { (*tydesc).size };
        let align = unsafe { (*tydesc).align };
        let ptr = unsafe { rt.alloc(size, align, 1) };

        Value::Struct { ptr, tydesc }
    }

    /// Allocate an Enum value.
    pub unsafe fn alloc_enum(
        rt: &mut rt::alloc::LocalRt,
        tydesc: *const rtdt::TyDesc,
    ) -> Self {
        let size = unsafe { (*tydesc).size };
        let align = unsafe { (*tydesc).align };
        let ptr = unsafe { rt.alloc(size, align, 1) };

        Value::Enum { ptr, tydesc }
    }

    /// Allocate a List value.
    pub unsafe fn alloc_list(
        rt: &mut rt::alloc::LocalRt,
        tydesc: *const rtdt::TyDesc,
    ) -> Self {
        let size = unsafe { (*tydesc).size };
        let align = unsafe { (*tydesc).align };
        let ptr = unsafe { rt.alloc(size, align, 1) as *mut rtdt::List };

        // Initialize to empty list.
        unsafe {
            (*ptr).data = std::ptr::null();
            (*ptr).size = 0;
            (*ptr).capacity = 0;
        }

        Value::List { ptr, tydesc }
    }

    /// Allocate a Map value.
    pub unsafe fn alloc_map(
        rt: &mut rt::alloc::LocalRt,
        tydesc: *const rtdt::TyDesc,
    ) -> Self {
        let size = unsafe { (*tydesc).size };
        let align = unsafe { (*tydesc).align };
        let ptr = unsafe { rt.alloc(size, align, 1) as *mut rtdt::Map };

        // Initialize to empty map.
        unsafe {
            (*ptr).root = std::ptr::null();
            (*ptr).len = 0;
        }

        Value::Map { ptr, tydesc }
    }

    /// Allocate a Set value.
    pub unsafe fn alloc_set(
        rt: &mut rt::alloc::LocalRt,
        tydesc: *const rtdt::TyDesc,
    ) -> Self {
        let size = unsafe { (*tydesc).size };
        let align = unsafe { (*tydesc).align };
        let ptr = unsafe { rt.alloc(size, align, 1) as *mut rtdt::Set };

        // Initialize to empty set.
        unsafe {
            (*ptr).root = std::ptr::null();
            (*ptr).len = 0;
        }

        Value::Set { ptr, tydesc }
    }

    /// Allocate an Option value.
    pub unsafe fn alloc_option(
        rt: &mut rt::alloc::LocalRt,
        tydesc: *const rtdt::TyDesc,
    ) -> Self {
        let size = unsafe { (*tydesc).size };
        let align = unsafe { (*tydesc).align };
        let ptr = unsafe { rt.alloc(size, align, 1) };

        Value::Option { ptr, tydesc }
    }

    /// Allocate a Result value.
    pub unsafe fn alloc_result(
        rt: &mut rt::alloc::LocalRt,
        tydesc: *const rtdt::TyDesc,
    ) -> Self {
        let size = unsafe { (*tydesc).size };
        let align = unsafe { (*tydesc).align };
        let ptr = unsafe { rt.alloc(size, align, 1) };

        Value::Result { ptr, tydesc }
    }

    /// Get the type descriptor for this value.
    ///
    /// For inline values (bool, u32, f32), returns null since they don't store a tydesc.
    /// For heap-allocated values, returns the stored tydesc pointer.
    pub fn tydesc(&self) -> *const rtdt::TyDesc {
        match self {
            Value::Bool(_) | Value::U32(_) | Value::F32(_) => {
                // Inline values don't have a stored tydesc.
                // Callers should handle this case.
                std::ptr::null()
            }
            Value::Int { tydesc, .. }
            | Value::String { tydesc, .. }
            | Value::Tuple { tydesc, .. }
            | Value::Struct { tydesc, .. }
            | Value::Enum { tydesc, .. }
            | Value::List { tydesc, .. }
            | Value::Map { tydesc, .. }
            | Value::Set { tydesc, .. }
            | Value::Option { tydesc, .. }
            | Value::Result { tydesc, .. }
            | Value::Data { tydesc, .. }
            | Value::Error { tydesc, .. } => *tydesc,
        }
    }

    /// Get a mutable pointer to the value's data.
    ///
    /// This is used as the destination for clone operations.
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        match self {
            Value::Bool(b) => b as *mut bool as *mut u8,
            Value::U32(n) => n as *mut u32 as *mut u8,
            Value::F32(f) => f as *mut f32 as *mut u8,
            Value::Int { ptr, .. } => *ptr as *mut u8,
            Value::String { ptr, .. } => *ptr as *mut u8,
            Value::Tuple { ptr, .. } => *ptr,
            Value::Struct { ptr, .. } => *ptr,
            Value::Enum { ptr, .. } => *ptr,
            Value::List { ptr, .. } => *ptr as *mut u8,
            Value::Map { ptr, .. } => *ptr as *mut u8,
            Value::Set { ptr, .. } => *ptr as *mut u8,
            Value::Option { ptr, .. } => *ptr,
            Value::Result { ptr, .. } => *ptr,
            Value::Data { ptr, .. } => *ptr as *mut u8,
            Value::Error { ptr, .. } => *ptr as *mut u8,
        }
    }

    /// Free this value using the runtime allocator.
    pub unsafe fn free(&mut self, rt: &mut rt::alloc::LocalRt) {
        match self {
            Value::Bool(_) | Value::U32(_) | Value::F32(_) => {
                // Inline values don't need freeing.
            }
            Value::Int { ptr, tydesc } => {
                if !ptr.is_null() && !tydesc.is_null() {
                    let size = unsafe { (**tydesc).size };
                    let align = unsafe { (**tydesc).align };
                    unsafe { rt.free(size, align, 1, *ptr as *mut u8) };
                }
            }
            Value::String { ptr, tydesc } => {
                if !ptr.is_null() && !tydesc.is_null() {
                    let size = unsafe { (**tydesc).size };
                    let align = unsafe { (**tydesc).align };
                    unsafe { rt.free(size, align, 1, *ptr as *mut u8) };
                }
            }
            Value::List { ptr, tydesc } => {
                if !ptr.is_null() && !tydesc.is_null() {
                    let size = unsafe { (**tydesc).size };
                    let align = unsafe { (**tydesc).align };
                    unsafe { rt.free(size, align, 1, *ptr as *mut u8) };
                }
            }
            Value::Map { ptr, tydesc } => {
                if !ptr.is_null() && !tydesc.is_null() {
                    let size = unsafe { (**tydesc).size };
                    let align = unsafe { (**tydesc).align };
                    unsafe { rt.free(size, align, 1, *ptr as *mut u8) };
                }
            }
            Value::Set { ptr, tydesc } => {
                if !ptr.is_null() && !tydesc.is_null() {
                    let size = unsafe { (**tydesc).size };
                    let align = unsafe { (**tydesc).align };
                    unsafe { rt.free(size, align, 1, *ptr as *mut u8) };
                }
            }
            Value::Data { ptr, tydesc } => {
                if !ptr.is_null() && !tydesc.is_null() {
                    let size = unsafe { (**tydesc).size };
                    let align = unsafe { (**tydesc).align };
                    unsafe { rt.free(size, align, 1, *ptr as *mut u8) };
                }
            }
            Value::Error { ptr, tydesc } => {
                if !ptr.is_null() && !tydesc.is_null() {
                    let size = unsafe { (**tydesc).size };
                    let align = unsafe { (**tydesc).align };
                    unsafe { rt.free(size, align, 1, *ptr as *mut u8) };
                }
            }
            Value::Tuple { ptr, tydesc } => {
                if !ptr.is_null() && !tydesc.is_null() {
                    let size = unsafe { (**tydesc).size };
                    let align = unsafe { (**tydesc).align };
                    unsafe { rt.free(size, align, 1, *ptr) };
                }
            }
            Value::Struct { ptr, tydesc } => {
                if !ptr.is_null() && !tydesc.is_null() {
                    let size = unsafe { (**tydesc).size };
                    let align = unsafe { (**tydesc).align };
                    unsafe { rt.free(size, align, 1, *ptr) };
                }
            }
            Value::Enum { ptr, tydesc } => {
                if !ptr.is_null() && !tydesc.is_null() {
                    let size = unsafe { (**tydesc).size };
                    let align = unsafe { (**tydesc).align };
                    unsafe { rt.free(size, align, 1, *ptr) };
                }
            }
            Value::Option { ptr, tydesc } => {
                if !ptr.is_null() && !tydesc.is_null() {
                    let size = unsafe { (**tydesc).size };
                    let align = unsafe { (**tydesc).align };
                    unsafe { rt.free(size, align, 1, *ptr) };
                }
            }
            Value::Result { ptr, tydesc } => {
                if !ptr.is_null() && !tydesc.is_null() {
                    let size = unsafe { (**tydesc).size };
                    let align = unsafe { (**tydesc).align };
                    unsafe { rt.free(size, align, 1, *ptr) };
                }
            }
        }
    }
}
