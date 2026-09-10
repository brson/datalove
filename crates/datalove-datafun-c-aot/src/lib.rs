//! AOT C code generation backend.
//!
//! Compiles datafun IR to C source code that can be compiled with any
//! C11-compatible compiler and linked with the datalove runtime.
//!
//! # Architecture
//!
//! The C AOT compiler uses a single-pass approach:
//!
//! 1. **Type descriptors**: Emit static TyDesc structures for all types used.
//! 2. **Functions**: Generate C functions for each IR function.
//! 3. **Entry point**: Generate main() that initializes runtime and runs the script.
//!
//! # Generated code structure
//!
//! For a script unit, the compiler generates:
//! - Type descriptor static data
//! - Module functions (prefixed with `__mod_N_`)
//! - Local functions (nested in script)
//! - `__script_body(void* rt)`: The script body taking a runtime handle.
//! - `main()`: Entry point that initializes runtime, runs body, cleans up.

mod codegen;
mod layout;
mod types;
mod tydesc;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write;

use datalove_datafun_ir::{
    CodeRef, FunctionRegistry, IrCodeUnit, IrModuleId, IrType,
};

pub use layout::FrameLayout;
pub use types::TypeLayout;

/// Errors during C AOT compilation.
#[derive(Debug)]
pub enum CAotError {
    /// Codegen error.
    Codegen(String),
    /// Unsupported feature.
    Unsupported(String),
}

/// Output from compiling a world to C.
///
/// Contains multiple C source files that should be compiled and linked together.
#[derive(Debug, Clone)]
pub struct CompilationOutput {
    /// List of (filename, content) pairs.
    /// Files are:
    /// - `mod_{module_id}_{name}.c` for each module
    /// - `script.c` for the script (contains local functions, script body, main)
    pub files: Vec<(String, String)>,
}

impl std::fmt::Display for CAotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CAotError::Codegen(msg) => write!(f, "codegen error: {}", msg),
            CAotError::Unsupported(msg) => write!(f, "unsupported: {}", msg),
        }
    }
}

impl std::error::Error for CAotError {}

/// C AOT compiler context.
///
/// Generates C source code from IR.
pub struct CAotCompiler {
    /// Next unique ID for type descriptors.
    next_tydesc_id: u32,
    /// Mapping from IrType to tydesc variable name.
    tydesc_names: HashMap<IrType, String>,
    /// One `extern` line per rider function the world can reach.
    ///
    /// Every file gets all of them. A declaration for a function the file does
    /// not call costs nothing, and working out which file calls what would
    /// have to walk the same instructions twice.
    native_decls: Vec<String>,
}

impl Default for CAotCompiler {
    fn default() -> Self {
        Self::new()
    }
}

impl CAotCompiler {
    /// Create a new C AOT compiler.
    pub fn new() -> Self {
        Self {
            next_tydesc_id: 0,
            tydesc_names: HashMap::new(),
            native_decls: Vec::new(),
        }
    }

    /// Compile a world (script + modules) to separate C source files.
    ///
    /// Returns a `CompilationOutput` containing:
    /// - One file per module: `mod_{module_id}_{name}.c`
    /// - One file for the script: `script.c`
    pub fn compile_world(
        &mut self,
        script_unit: &IrCodeUnit,
        registry: &FunctionRegistry,
    ) -> Result<CompilationOutput, CAotError> {
        let mut files = Vec::new();

        // Group module functions by module ID. Ordered, because both the file
        // list and the extern declarations below are emitted in this order.
        // A native unit has no body to emit: it is a name the linker resolves
        // to the rider, so it is declared and never defined.
        let mut modules_by_id: BTreeMap<IrModuleId, Vec<&IrCodeUnit>> = BTreeMap::new();
        let mut natives: Vec<&IrCodeUnit> = Vec::new();
        for ((module_id, _func_id), ir_unit) in registry.iter_module_code_units_with_ids() {
            if ir_unit.native_context().is_some() {
                natives.push(ir_unit);
            } else {
                modules_by_id.entry(module_id).or_default().push(ir_unit);
            }
        }
        self.native_decls = natives.iter()
            .filter_map(|unit| unit.native_context())
            .map(native_declaration)
            .collect();

        // Emit one file per module.
        for (module_id, units) in &modules_by_id {
            let module_file = self.compile_module_file(*module_id, units, registry)?;
            // Use first function name as module name hint.
            let module_name = units.first().map(|u| u.name.as_str()).unwrap_or("unknown");
            let filename = format!("mod_{}_{}.c", module_id.0, module_name);
            files.push((filename, module_file));
        }

        // Emit script file.
        let script_file = self.compile_script_file(script_unit, registry, &modules_by_id)?;
        files.push(("script.c".to_string(), script_file));

        Ok(CompilationOutput { files })
    }

    /// Compile a single module to a C source file.
    fn compile_module_file(
        &mut self,
        module_id: IrModuleId,
        units: &[&IrCodeUnit],
        registry: &FunctionRegistry,
    ) -> Result<String, CAotError> {
        let mut output = String::new();

        // Emit header.
        self.emit_header(&mut output)?;

        // Collect types used in this module.
        let mut types = BTreeSet::new();
        for unit in units {
            tydesc::collect_types_from_code_unit(unit, &mut types);
        }

        // Emit type descriptors.
        self.emit_tydescs(&mut output, &types)?;

        // Forward declare functions in this module.
        writeln!(output, "// Function declarations").unwrap();
        for unit in units {
            let func_name = format!("__mod_{}_{}", module_id.0, &unit.name);
            let sig = self.build_signature(unit);
            // Not static - needs to be visible to script.
            writeln!(output, "{} {}({});", sig.return_type, func_name, sig.params).unwrap();
        }
        writeln!(output).unwrap();

        // Emit module functions.
        for unit in units {
            self.emit_module_function(&mut output, module_id, unit, registry)?;
        }

        Ok(output)
    }

    /// Compile the script to a C source file.
    fn compile_script_file(
        &mut self,
        unit: &IrCodeUnit,
        registry: &FunctionRegistry,
        modules_by_id: &BTreeMap<IrModuleId, Vec<&IrCodeUnit>>,
    ) -> Result<String, CAotError> {
        let mut output = String::new();

        // Emit header.
        self.emit_header(&mut output)?;

        // Collect types used in script.
        let mut types = BTreeSet::new();
        tydesc::collect_types_from_script_unit(unit, &mut types);

        // Emit type descriptors.
        self.emit_tydescs(&mut output, &types)?;

        // Extern declarations for module functions.
        if !modules_by_id.is_empty() {
            writeln!(output, "// Extern declarations for module functions").unwrap();
            for (module_id, units) in modules_by_id {
                for ir_unit in units {
                    let func_name = format!("__mod_{}_{}", module_id.0, &ir_unit.name);
                    let sig = self.build_signature(ir_unit);
                    writeln!(output, "extern {} {}({});", sig.return_type, func_name, sig.params).unwrap();
                }
            }
            writeln!(output).unwrap();
        }

        // Forward declare local functions.
        if !unit.nested_units.is_empty() {
            writeln!(output, "// Local function declarations").unwrap();
            for nested in &unit.nested_units {
                let func_name = format!("__local_{}", &nested.name);
                let sig = self.build_signature(nested);
                writeln!(output, "static {} {}({});", sig.return_type, func_name, sig.params).unwrap();
            }
            writeln!(output).unwrap();
        }

        // Forward declare script body.
        writeln!(output, "static void __script_body(void* rt);").unwrap();
        writeln!(output).unwrap();

        // Emit local functions.
        for nested in &unit.nested_units {
            self.emit_local_function(&mut output, nested, unit, registry)?;
        }

        // Emit script body.
        self.emit_script_body(&mut output, unit, registry)?;

        // Emit main entry point.
        self.emit_main(&mut output)?;

        Ok(output)
    }

    /// Emit C header with includes and type definitions.
    fn emit_header(&self, out: &mut String) -> Result<(), CAotError> {
        writeln!(out, "// Generated by datalove-datafun-c-aot").unwrap();
        writeln!(out, "#include <stdint.h>").unwrap();
        writeln!(out, "#include <stddef.h>").unwrap();
        writeln!(out, "#include <string.h>").unwrap();
        // The float intrinsics lower to the C library's own names, so this is
        // the same header they come from anywhere else.
        writeln!(out, "#include <math.h>").unwrap();
        writeln!(out).unwrap();

        // Runtime type definitions (must match datalove-rtdt).
        writeln!(out, "// Runtime type definitions").unwrap();
        writeln!(out, "typedef uint8_t bool_t;").unwrap();

        // Index type depends on index-64 feature.
        #[cfg(not(feature = "index-64"))]
        {
            writeln!(out, "typedef uint32_t index_t;").unwrap();
            writeln!(out, "typedef int32_t offset_t;").unwrap();
        }
        #[cfg(feature = "index-64")]
        {
            writeln!(out, "typedef uint64_t index_t;").unwrap();
            writeln!(out, "typedef int64_t offset_t;").unwrap();
        }
        writeln!(out).unwrap();

        // Collection types.
        writeln!(out, "// Collection types (opaque, matching rtdt layout)").unwrap();
        writeln!(out, "typedef struct {{ void* data; index_t size; index_t capacity; }} dtlv_list_t;").unwrap();
        writeln!(out, "typedef struct {{ void* data; index_t size; index_t capacity; }} dtlv_string_t;").unwrap();
        writeln!(out, "typedef struct {{ void* root; index_t len; }} dtlv_map_t;").unwrap();
        writeln!(out, "typedef struct {{ void* root; index_t len; }} dtlv_set_t;").unwrap();
        writeln!(out, "typedef struct {{ void* data; int32_t size_and_sign; index_t capacity; }} dtlv_int_t;").unwrap();
        writeln!(out, "typedef struct {{ void* primary; void* secondary; }} dtlv_data_t;").unwrap();
        writeln!(out, "typedef struct {{ void* primary; void* secondary; }} dtlv_error_t;").unwrap();
        writeln!(out, "typedef struct {{ void* ptr_base; index_t capacity_elems; index_t offset_elems; void* shape; void* strides; uint8_t layout; }} dtlv_tensor_t;").unwrap();
        writeln!(out, "typedef struct {{ index_t len; index_t capacity; void* data; }} dtlv_table_t;").unwrap();
        writeln!(out).unwrap();

        // Type descriptor types.
        writeln!(out, "// Type descriptor types").unwrap();
        writeln!(out, "typedef struct dtlv_tydesc dtlv_tydesc_t;").unwrap();
        writeln!(out, "typedef struct {{ uint32_t offset; const dtlv_tydesc_t* tydesc; }} dtlv_tuple_field_t;").unwrap();
        writeln!(out, "typedef struct {{ const char* name; uint32_t name_len; uint32_t offset; const dtlv_tydesc_t* tydesc; }} dtlv_struct_field_t;").unwrap();
        writeln!(out, "typedef struct {{ const char* name; uint32_t name_len; uint32_t offset; const dtlv_tydesc_t* payload; }} dtlv_enum_variant_t;").unwrap();
        writeln!(out, "typedef struct {{ const char* name; uint32_t name_len; const dtlv_tydesc_t* tydesc; }} dtlv_table_column_t;").unwrap();
        writeln!(out).unwrap();

        // Dangling pointer macro for empty arrays (Rust expects aligned non-NULL pointer).
        writeln!(out, "// Dangling pointer for empty arrays (must be aligned, not NULL)").unwrap();
        writeln!(out, "#define DANGLING(T) ((const T*)_Alignof(T))").unwrap();
        writeln!(out).unwrap();

        writeln!(out, "typedef union {{").unwrap();
        writeln!(out, "    struct {{ }} nothing;").unwrap();
        writeln!(out, "    struct {{ uint32_t num_fields; const dtlv_tuple_field_t* fields; }} tuple;").unwrap();
        writeln!(out, "    struct {{ const dtlv_struct_field_t* fields; uint32_t num_fields; }} struct_;").unwrap();
        writeln!(out, "    struct {{ const dtlv_enum_variant_t* variants; uint32_t num_variants; }} enum_;").unwrap();
        writeln!(out, "    struct {{ const dtlv_tydesc_t* element_tydesc; }} list;").unwrap();
        writeln!(out, "    struct {{ const dtlv_tydesc_t* key_tydesc; const dtlv_tydesc_t* value_tydesc; }} map;").unwrap();
        writeln!(out, "    struct {{ const dtlv_tydesc_t* element_tydesc; }} set;").unwrap();
        writeln!(out, "    struct {{ const dtlv_tydesc_t* element_tydesc; uint32_t rank; }} tensor;").unwrap();
        writeln!(out, "    struct {{ uint32_t num_columns; const dtlv_table_column_t* columns; }} table;").unwrap();
        writeln!(out, "    struct {{ const dtlv_tydesc_t* inner_tydesc; uint32_t payload_offset; }} option;").unwrap();
        writeln!(out, "    struct {{ const dtlv_tydesc_t* ok_tydesc; uint32_t payload_offset; }} result;").unwrap();
        writeln!(out, "    struct {{ const char* name; uint32_t name_len; }} atom;").unwrap();
        writeln!(out, "    struct {{ const char* name; uint32_t name_len; const dtlv_tydesc_t* payload; }} term;").unwrap();
        writeln!(out, "}} dtlv_tyinfo_t;").unwrap();
        writeln!(out).unwrap();

        writeln!(out, "struct dtlv_tydesc {{").unwrap();
        writeln!(out, "    uint8_t type_tag;").unwrap();
        writeln!(out, "    uint32_t size;").unwrap();
        writeln!(out, "    uint32_t align;").unwrap();
        writeln!(out, "    dtlv_tyinfo_t type_info;").unwrap();
        writeln!(out, "}};").unwrap();
        writeln!(out).unwrap();

        // Runtime function declarations.
        writeln!(out, "// Runtime function declarations").unwrap();
        writeln!(out, "extern void* dtlv_rti_init(void);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_shutdown(void* rt);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_set_debug_mode(void* rt, uint8_t mode);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_debuglog_local(void* rt, const void* value_ref, const dtlv_tydesc_t* tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_any_destroy_local(void* rt, void* value, const dtlv_tydesc_t* tydesc);").unwrap();
        writeln!(out, "extern void* dtlv_rti_mem_alloc_raw_local(void* rt, uint32_t size, uint32_t align, index_t count);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_string_from_bytes(void* rt, const uint8_t* bytes, uint32_t len, void* result_out, const dtlv_tydesc_t* tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_list_create_local(void* rt, void* value_out, const dtlv_tydesc_t* tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_list_push_local(void* rt, void* list_mut, const dtlv_tydesc_t* list_tydesc, void* elem_in, const dtlv_tydesc_t* elem_tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_list_build_from_slice_local(void* rt, void* list_out, const dtlv_tydesc_t* elem_tydesc, const void* elems_ptr, index_t num_elems);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_btreeset_create_local(void* rt, void* value_out, const dtlv_tydesc_t* tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_btreeset_insert_local(void* rt, void* set_mut, const dtlv_tydesc_t* set_tydesc, void* elem_in, const dtlv_tydesc_t* elem_tydesc, void* bool_out);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_btreeset_build_from_sorted_slice_local(void* rt, void* set_out, const dtlv_tydesc_t* elem_tydesc, const void* elems_ptr, index_t num_elems);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_btreemap_create_local(void* rt, void* value_out, const dtlv_tydesc_t* tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_btreemap_insert_local(void* rt, void* map_mut, const dtlv_tydesc_t* map_tydesc, void* key_in, const dtlv_tydesc_t* key_tydesc, void* val_in, const dtlv_tydesc_t* val_tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_btreemap_build_from_sorted_slices_local(void* rt, void* map_out, const dtlv_tydesc_t* key_tydesc, const dtlv_tydesc_t* val_tydesc, const void* keys_ptr, const void* vals_ptr, index_t num_entries);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_tensor_init_local(void* rt, void* elem_data_in, index_t elem_count, const dtlv_tydesc_t* elem_tydesc, const index_t* shape_ptr, uint32_t rank, void* tensor_out, const dtlv_tydesc_t* tensor_tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_table_create_local(void* rt, void* value_out, const dtlv_tydesc_t* tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_table_push_row_local(void* rt, void* table_mut, const dtlv_tydesc_t* table_tydesc, const void* row_ref, const dtlv_tydesc_t* row_tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_table_build_from_rows_local(void* rt, void* table_out, const dtlv_tydesc_t* table_tydesc, const void* rows_ptr, const dtlv_tydesc_t* row_tydesc, index_t num_rows);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_int_add(void* rt, const void* a_in, const dtlv_tydesc_t* a_tydesc, const void* b_in, const dtlv_tydesc_t* b_tydesc, void* result_out, const dtlv_tydesc_t* result_tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_int_sub(void* rt, const void* a_in, const dtlv_tydesc_t* a_tydesc, const void* b_in, const dtlv_tydesc_t* b_tydesc, void* result_out, const dtlv_tydesc_t* result_tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_int_mul(void* rt, const void* a_in, const dtlv_tydesc_t* a_tydesc, const void* b_in, const dtlv_tydesc_t* b_tydesc, void* result_out, const dtlv_tydesc_t* result_tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_int_div_checked(void* rt, const void* a_in, const dtlv_tydesc_t* a_tydesc, const void* b_in, const dtlv_tydesc_t* b_tydesc, void* result_out, const dtlv_tydesc_t* result_tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_int_neg(void* rt, const void* a_in, const dtlv_tydesc_t* a_tydesc, void* result_out, const dtlv_tydesc_t* result_tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_int_from_fixed(void* rt, const void* src_in, const dtlv_tydesc_t* src_tydesc, void* result_out, const dtlv_tydesc_t* result_tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_int_from_limbs(void* rt, const uint32_t* limbs_ptr, uint32_t limb_count, uint8_t negative, void* result_out, const dtlv_tydesc_t* result_tydesc);").unwrap();
        writeln!(out, "extern int8_t dtlv_rti_cmp_local(void* rt, const void* a_ref, const dtlv_tydesc_t* a_tydesc, const void* b_ref, const dtlv_tydesc_t* b_tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_move_value_local(void* rt, const void* src_ref, const dtlv_tydesc_t* tydesc, void* dst_out);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_clone_local(void* rt, const void* src_ref, const dtlv_tydesc_t* src_tydesc, void* dst_out, const dtlv_tydesc_t* dst_tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_error_from_local(void* rt, void* inner_in, const dtlv_tydesc_t* inner_tydesc, void* dest_out);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_data_from_local(void* rt, void* inner_in, const dtlv_tydesc_t* inner_tydesc, void* dest_out);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_erase_local(void* rt, const void* src_in, const dtlv_tydesc_t* src_tydesc, void* dst_out, const dtlv_tydesc_t* dst_tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_reify_local(void* rt, const void* src_in, const dtlv_tydesc_t* src_tydesc, void* dst_out, const dtlv_tydesc_t* dst_tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_data_parts(const void* data_in, const void** value_out, const dtlv_tydesc_t** tydesc_out);").unwrap();
        // An operator on a type parameter bounded to `float`.
        writeln!(out, "extern uint8_t dtlv_rti_dyn_binop(void* rt, uint8_t op, const void* lhs, const dtlv_tydesc_t* lhs_td, const void* rhs, const dtlv_tydesc_t* rhs_td, void* out, const dtlv_tydesc_t* out_td);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_dyn_binop_checked(void* rt, uint8_t op, const void* lhs, const dtlv_tydesc_t* lhs_td, const void* rhs, const dtlv_tydesc_t* rhs_td, void* out, const dtlv_tydesc_t* out_td, bool_t* overflow_out);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_dyn_neg_checked(void* rt, const void* value, const dtlv_tydesc_t* value_td, void* out, const dtlv_tydesc_t* out_td, bool_t* overflow_out);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_list_get_erased_local(void* rt, const void* list_ref, const dtlv_tydesc_t* list_tydesc, index_t index, void* option_out, const dtlv_tydesc_t* option_tydesc);").unwrap();
        writeln!(out, "extern uint8_t dtlv_rti_clone_erased_local(void* rt, const void* src_in, const dtlv_tydesc_t* src_tydesc, void* dst_out, const dtlv_tydesc_t* dst_tydesc);").unwrap();
        writeln!(out).unwrap();

        // Rider functions, which the linker resolves against the native
        // component the way it does the runtime above.
        if !self.native_decls.is_empty() {
            writeln!(out, "// Native rider functions").unwrap();
            for decl in &self.native_decls {
                writeln!(out, "{}", decl).unwrap();
            }
            writeln!(out).unwrap();
        }

        // Tracking byte values.
        writeln!(out, "// Tracking byte values").unwrap();
        writeln!(out, "#define TRACK_UNINIT 0x00").unwrap();
        writeln!(out, "#define TRACK_LIVE   0x01").unwrap();
        writeln!(out, "#define TRACK_MOVED  0x02").unwrap();
        writeln!(out).unwrap();

        // RtOrdering values returned by dtlv_rti_cmp_local. Note these are
        // positive tags, not the C convention of negative/zero/positive.
        writeln!(out, "// RtOrdering values").unwrap();
        writeln!(out, "#define ORD_LESS    1").unwrap();
        writeln!(out, "#define ORD_EQUAL   2").unwrap();
        writeln!(out, "#define ORD_GREATER 3").unwrap();
        writeln!(out).unwrap();

        // Option/Result tag values.
        writeln!(out, "// Option/Result tag values").unwrap();
        writeln!(out, "#define OPTION_NONE 1").unwrap();
        writeln!(out, "#define OPTION_SOME 2").unwrap();
        writeln!(out, "#define RESULT_OK   1").unwrap();
        writeln!(out, "#define RESULT_ERR  2").unwrap();
        writeln!(out).unwrap();

        Ok(())
    }

    /// Emit type descriptors for all types.
    fn emit_tydescs(&mut self, out: &mut String, types: &BTreeSet<IrType>) -> Result<(), CAotError> {
        writeln!(out, "// Type descriptors").unwrap();

        // We need to emit types in dependency order, so primitive types first.
        // `sort_by_key` is stable, so ties keep the order they came in; the
        // types arrive from a `BTreeSet` so that order is the same every run.
        let mut sorted_types: Vec<_> = types.iter().collect();
        sorted_types.sort_by_key(|t| tydesc::type_depth(t));

        for ty in sorted_types {
            self.emit_tydesc(out, ty)?;
        }

        writeln!(out).unwrap();
        Ok(())
    }

    /// Get or create the tydesc variable name for a type.
    fn get_tydesc_name(&mut self, ty: &IrType) -> String {
        if let Some(name) = self.tydesc_names.get(ty) {
            return name.clone();
        }

        let name = format!("__tydesc_{}", self.next_tydesc_id);
        self.next_tydesc_id += 1;
        self.tydesc_names.insert(ty.clone(), name.clone());
        name
    }

    /// Emit a single type descriptor.
    fn emit_tydesc(&mut self, out: &mut String, ty: &IrType) -> Result<(), CAotError> {
        let name = self.get_tydesc_name(ty);
        tydesc::emit_tydesc(out, &name, ty, self)?;
        Ok(())
    }

    /// Build the C function signature for an IR code unit.
    fn build_signature(&self, unit: &IrCodeUnit) -> CFunctionSignature {
        let func_ctx = unit.function_context()
            .expect("build_signature requires function context");

        let mut params = String::from("void* rt");

        // Check if we need sret (structure return).
        let uses_sret = types::uses_sret(&func_ctx.return_type);
        if uses_sret {
            params.push_str(", void* __sret");
        }

        // User parameters are passed by pointer.
        for (i, _param_ty) in func_ctx.param_types.iter().enumerate() {
            write!(&mut params, ", void* p{}", i).unwrap();
        }

        // Then a descriptor for each parameter whose own type does not say what
        // arrives at it, in the order `descriptor_params` names them. A generic
        // function's type for a borrowed parameter reads `data` where its type
        // parameter stood, so the caller has to say what the value really is.
        for (i, _) in func_ctx.descriptor_params.iter().enumerate() {
            write!(&mut params, ", const dtlv_tydesc_t* d{}", i).unwrap();
        }
        // Then one for each shape the body builds a collection of, which no
        // value carries.
        for (i, _) in func_ctx.descriptor_shapes.iter().enumerate() {
            write!(&mut params, ", const dtlv_tydesc_t* s{}", i).unwrap();
        }

        let return_type = if func_ctx.return_type == IrType::Unit || uses_sret {
            "void".to_string()
        } else {
            types::ir_type_to_c(&func_ctx.return_type)
        };

        CFunctionSignature { return_type, params }
    }

    /// Emit a module function.
    fn emit_module_function(
        &mut self,
        out: &mut String,
        module_id: IrModuleId,
        unit: &IrCodeUnit,
        registry: &FunctionRegistry,
    ) -> Result<(), CAotError> {
        let func_name = format!("__mod_{}_{}", module_id.0, &unit.name);
        codegen::emit_function(out, &func_name, unit, Some(module_id), None, self, registry)?;
        Ok(())
    }

    /// Emit a local function.
    fn emit_local_function(
        &mut self,
        out: &mut String,
        unit: &IrCodeUnit,
        parent: &IrCodeUnit,
        registry: &FunctionRegistry,
    ) -> Result<(), CAotError> {
        let func_name = format!("__local_{}", &unit.name);
        // Pass parent so local function calls can be resolved.
        codegen::emit_function(out, &func_name, unit, None, Some(parent), self, registry)?;
        Ok(())
    }

    /// Emit the script body function.
    fn emit_script_body(
        &mut self,
        out: &mut String,
        unit: &IrCodeUnit,
        registry: &FunctionRegistry,
    ) -> Result<(), CAotError> {
        codegen::emit_script_body(out, unit, self, registry)?;
        Ok(())
    }

    /// Emit the main entry point.
    fn emit_main(&self, out: &mut String) -> Result<(), CAotError> {
        writeln!(out, "int main(void) {{").unwrap();
        writeln!(out, "    void* rt = dtlv_rti_init();").unwrap();
        writeln!(out, "    dtlv_rti_set_debug_mode(rt, 0); // Stderr").unwrap();
        writeln!(out, "    __script_body(rt);").unwrap();
        writeln!(out, "    dtlv_rti_shutdown(rt);").unwrap();
        writeln!(out, "    return 0;").unwrap();
        writeln!(out, "}}").unwrap();
        Ok(())
    }

    /// Resolve a code reference to a C function name.
    pub fn resolve_func_name(&self, code_ref: &CodeRef) -> String {
        match code_ref {
            CodeRef::Local(id) => format!("__local_{}", id.0),
            CodeRef::Module { module, id } => format!("__mod_{}_{}", module.0, id.0),
            CodeRef::External { unit, id } => format!("__ext_{}_{}", unit, id.0),
        }
    }

    /// Resolve a code reference to a C function name with the actual function name.
    pub fn resolve_func_name_with_registry(
        &self,
        code_ref: &CodeRef,
        current_unit: &IrCodeUnit,
        registry: &FunctionRegistry,
    ) -> String {
        match code_ref {
            CodeRef::Local(id) => {
                // Find the nested unit by ID.
                for nested in &current_unit.nested_units {
                    if nested.id == *id {
                        return format!("__local_{}", &nested.name);
                    }
                }
                format!("__local_{}", id.0)
            }
            CodeRef::Module { module, id } => {
                if let Some(ir_unit) = registry.get_module_function_as_unit(*module, *id) {
                    format!("__mod_{}_{}", module.0, &ir_unit.name)
                } else {
                    format!("__mod_{}_{}", module.0, id.0)
                }
            }
            CodeRef::External { unit, id } => format!("__ext_{}_{}", unit, id.0),
        }
    }
}

/// C function signature components.
struct CFunctionSignature {
    return_type: String,
    params: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compiler_creation() {
        let compiler = CAotCompiler::new();
        assert_eq!(compiler.next_tydesc_id, 0);
    }
}

/// The `extern` line declaring one rider function.
///
/// Every parameter is a pointer and a descriptor, the result is written
/// through a pair given the same way, and the status says whether it was.
fn native_declaration(ctx: &datalove_datafun_ir::NativeContext) -> String {
    let mut params = String::from("void* rt");
    for i in 0..ctx.param_types.len() {
        params.push_str(&format!(
            ", void* a{i}, const dtlv_tydesc_t* t{i}"));
    }
    params.push_str(", void* result_out, const dtlv_tydesc_t* result_tydesc");
    format!("extern uint8_t {}({});", ctx.symbol, params)
}
