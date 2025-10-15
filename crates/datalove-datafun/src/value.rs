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

    /// Pretty-print this value using the runtime pretty printer.
    ///
    /// Returns a string representation in valid datalit syntax.
    pub fn pretty_print(&self, rt: &mut rt::alloc::LocalRt) -> Result<String, crate::interp::InterpError> {
        unsafe {
            // Get runtime handle.
            let rt_handle = rt as *mut _ as rt::LocalRtHandle;

            // Create string type descriptor.
            let string_tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::String,
                size: std::mem::size_of::<rtdt::String>() as u32,
                align: std::mem::align_of::<rtdt::String>() as u32,
                type_info: rtdt::TyInfo {
                    nothing: rtdt::TyInfoNothing,
                },
            };

            // Create output string.
            let mut output_string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            let status = rt::dtlv_rti_string_create_local(
                rt_handle,
                output_string.as_mut_ptr() as *mut u8,
                &string_tydesc,
            );

            if status != rt::RtStatus::Ok {
                return Err(crate::interp::InterpError::RuntimeError(
                    "Failed to create output string".to_string(),
                ));
            }

            let mut output_string = output_string.assume_init();

            // Get value pointer and type descriptor.
            let (value_ptr, tydesc_ptr) = match self {
                Value::Bool(b) => {
                    // Create inline bool tydesc.
                    let tydesc = rtdt::TyDesc {
                        type_tag: rtdt::TyTag::Bool,
                        size: 1,
                        align: 1,
                        type_info: rtdt::TyInfo {
                            nothing: rtdt::TyInfoNothing,
                        },
                    };
                    (b as *const bool as *const u8, &tydesc as *const rtdt::TyDesc)
                }
                Value::U32(n) => {
                    // Create inline u32 tydesc.
                    let tydesc = rtdt::TyDesc {
                        type_tag: rtdt::TyTag::U32,
                        size: 4,
                        align: 4,
                        type_info: rtdt::TyInfo {
                            nothing: rtdt::TyInfoNothing,
                        },
                    };
                    (n as *const u32 as *const u8, &tydesc as *const rtdt::TyDesc)
                }
                Value::F32(f) => {
                    // Create inline f32 tydesc.
                    let tydesc = rtdt::TyDesc {
                        type_tag: rtdt::TyTag::F32,
                        size: 4,
                        align: 4,
                        type_info: rtdt::TyInfo {
                            nothing: rtdt::TyInfoNothing,
                        },
                    };
                    (f as *const f32 as *const u8, &tydesc as *const rtdt::TyDesc)
                }
                Value::Int { ptr, tydesc } => (*ptr as *const u8, *tydesc),
                Value::String { ptr, tydesc } => (*ptr as *const u8, *tydesc),
                Value::Tuple { ptr, tydesc } => (*ptr as *const u8, *tydesc),
                Value::Struct { ptr, tydesc } => (*ptr as *const u8, *tydesc),
                Value::Enum { ptr, tydesc } => (*ptr as *const u8, *tydesc),
                Value::List { ptr, tydesc } => (*ptr as *const u8, *tydesc),
                Value::Map { ptr, tydesc } => (*ptr as *const u8, *tydesc),
                Value::Set { ptr, tydesc } => (*ptr as *const u8, *tydesc),
                Value::Option { ptr, tydesc } => (*ptr as *const u8, *tydesc),
                Value::Result { ptr, tydesc } => (*ptr as *const u8, *tydesc),
                Value::Data { ptr, tydesc } => (*ptr as *const u8, *tydesc),
                Value::Error { ptr, tydesc } => (*ptr as *const u8, *tydesc),
            };

            // Pretty-print value.
            let status = rt::dtlv_rti_pretty_print_local(
                rt_handle,
                value_ptr,
                tydesc_ptr,
                &mut output_string as *mut rtdt::String as *mut u8,
                &string_tydesc,
            );

            if status != rt::RtStatus::Ok {
                rt::dtlv_rti_string_destroy_local(
                    rt_handle,
                    &mut output_string as *mut rtdt::String as *mut u8,
                    &string_tydesc,
                );
                return Err(crate::interp::InterpError::RuntimeError(
                    "Failed to pretty-print value".to_string(),
                ));
            }

            // Extract string contents.
            let result = if output_string.data.is_null() || output_string.size == 0 {
                String::new()
            } else {
                let bytes = std::slice::from_raw_parts(output_string.data, output_string.size as usize);
                String::from_utf8_lossy(bytes).to_string()
            };

            // Cleanup.
            rt::dtlv_rti_string_destroy_local(
                rt_handle,
                &mut output_string as *mut rtdt::String as *mut u8,
                &string_tydesc,
            );

            Ok(result)
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
