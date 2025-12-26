//! Interpreter context and module function table.
//!
//! - [`InterpContext`]: Runtime, module graph, call stack, tydesc table
//! - [`ModuleFunctionTableGraph`]: Function definitions by module

use rmx::std::collections::HashMap;
use bct::text::InternedText;

use crate::module_graph::{ModuleId, ModuleGraph, ModuleGraphTypecheckResult};
use crate::ast;

use super::{InterpError, StackFrame};

/// Main interpreter state.
pub struct InterpContext<'db> {
    pub(super) db: &'db dyn crate::Db,
    pub(super) runtime: datalove_rt::rust::Runtime,
    pub(super) tydesc_table: datalove_datalit::tydesc_table::TyDescTable<'db>,
    /// Call stack for frame-based execution.
    pub(super) call_stack: Vec<StackFrame<'db>>,
    /// Module function table for ModuleGraph mode.
    pub(super) module_functions_graph: ModuleFunctionTableGraph<'db>,
    /// Current module ID for module-internal function calls.
    pub(super) current_module_id: Option<ModuleId>,
    /// Typecheck result for the module graph.
    pub(super) module_graph_typecheck: Option<ModuleGraphTypecheckResult<'db>>,
    /// Expression types from typechecking, indexed by ExprFun salsa ID.
    pub(super) expr_types: Vec<Option<crate::tycheck::TypeAndHeap<'db>>>,
    /// Resolved call targets from typechecking, indexed by ExprFunctionCall salsa ID.
    pub(super) call_targets: Vec<Option<crate::tycheck::ResolvedCallTarget<'db>>>,
}

/// Function table: maps names to definitions and source modules.
pub struct ModuleFunctionTableGraph<'db> {
    imported_functions: HashMap<InternedText<'db>, (ast::StmtFun<'db>, ModuleId)>,
    module_all_functions: HashMap<ModuleId, HashMap<InternedText<'db>, ast::StmtFun<'db>>>,
}

impl<'db> InterpContext<'db> {
    /// Create context from a successful ModuleGraphTypecheckResult.
    pub fn new_with_module_graph(
        db: &'db dyn crate::Db,
        typecheck_result: ModuleGraphTypecheckResult<'db>,
    ) -> Result<InterpContext<'db>, InterpError> {
        // Check for typecheck errors.
        if !typecheck_result.is_ok(db) {
            let all_errors: Vec<_> = typecheck_result.all_errors(db).into_iter().cloned().collect();
            return Err(InterpError::TypecheckErrors(all_errors));
        }

        let graph = typecheck_result.graph(db);
        let module_functions_graph = ModuleFunctionTableGraph::build_from_graph(db, graph);

        // Initialize expr_types and call_targets from module graph typecheck.
        let expr_types = typecheck_result.expr_types(db).clone();
        let call_targets = typecheck_result.call_targets(db).clone();

        Ok(InterpContext {
            db,
            runtime: datalove_rt::rust::Runtime::new(),
            tydesc_table: datalove_datalit::tydesc_table::TyDescTable::new(db),
            call_stack: Vec::new(),
            module_functions_graph,
            current_module_id: None,
            module_graph_typecheck: Some(typecheck_result),
            expr_types,
            call_targets,
        })
    }

    /// Get expression type from typechecking.
    pub fn get_expr_type(&self, expr: ast::ExprFun<'db>) -> Option<crate::tycheck::TypeAndHeap<'db>> {
        use salsa::plumbing::AsId;
        let id = expr.as_id();
        let index = id.index() as usize;
        self.expr_types.get(index).copied().flatten()
    }

    /// Get resolved call target from typechecking.
    pub fn get_call_target(&self, call: ast::ExprFunctionCall<'db>) -> Option<crate::tycheck::ResolvedCallTarget<'db>> {
        use salsa::plumbing::AsId;
        let id = call.as_id();
        let index = id.index() as usize;
        self.call_targets.get(index).copied().flatten()
    }

    pub fn runtime_handle(&self) -> datalove_rt::c::LocalRtHandle {
        self.runtime.handle()
    }

    pub fn module_function_graph(&self) -> &ModuleFunctionTableGraph<'db> {
        &self.module_functions_graph
    }

    /// Pretty-print a value to a string.
    pub fn pretty_print_value(&mut self, value: &super::Value) -> Result<String, InterpError> {
        use datalove_rt as rt;
        use datalove_rt::rtdt;

        unsafe {
            let rt_handle = self.runtime.handle();
            let string_tydesc = self.tydesc_table.get_or_create(&crate::datalit::tycheck::Type::String);

            let mut output_string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            let status = rt::c::dtlv_rti_string_create_local(
                rt_handle,
                output_string.as_mut_ptr() as *mut u8,
                string_tydesc,
            );

            if status != rt::c::RtStatus::Ok {
                return Err(InterpError::RuntimeError(
                    "Failed to create output string".to_string(),
                ));
            }

            let mut output_string = output_string.assume_init();

            let status = rt::c::dtlv_rti_pretty_print_local(
                rt_handle,
                value.ptr,
                value.tydesc,
                &mut output_string as *mut rtdt::String as *mut u8,
                string_tydesc,
            );

            if status != rt::c::RtStatus::Ok {
                rt::c::dtlv_rti_string_destroy_local(
                    rt_handle,
                    &mut output_string as *mut rtdt::String as *mut u8,
                    string_tydesc,
                );
                return Err(InterpError::RuntimeError(
                    "Failed to pretty-print value".to_string(),
                ));
            }

            let result = if output_string.data.is_null() || output_string.size == 0 {
                String::new()
            } else {
                let bytes = std::slice::from_raw_parts(output_string.data, output_string.size as usize);
                String::from_utf8_lossy(bytes).to_string()
            };

            rt::c::dtlv_rti_string_destroy_local(
                rt_handle,
                &mut output_string as *mut rtdt::String as *mut u8,
                string_tydesc,
            );

            Ok(result)
        }
    }
}

impl<'db> ModuleFunctionTableGraph<'db> {
    pub fn new() -> ModuleFunctionTableGraph<'db> {
        ModuleFunctionTableGraph {
            imported_functions: HashMap::new(),
            module_all_functions: HashMap::new(),
        }
    }

    /// Build from a ModuleGraph, extracting all function definitions.
    pub fn build_from_graph(
        db: &'db dyn crate::Db,
        graph: ModuleGraph,
    ) -> ModuleFunctionTableGraph<'db> {
        let mut table = ModuleFunctionTableGraph::new();

        for module in graph.iter_modules(db) {
            let module_id = module.id(db);
            let source = module.source(db);

            let parse_result = crate::parser::parse(db, source);
            let parsed = parse_result.script(db);

            let mut module_funcs = HashMap::new();
            for statement in parsed.statements(db) {
                if let ast::Statement::Fun(func) = statement {
                    let func_name = func.name(db);
                    module_funcs.insert(func_name, *func);
                }
            }
            table.module_all_functions.insert(module_id, module_funcs);
        }

        table
    }

    /// Look up an imported function by name.
    pub fn get(&self, name: InternedText<'db>) -> Option<(ast::StmtFun<'db>, ModuleId)> {
        self.imported_functions.get(&name).copied()
    }

    /// Get all functions defined in a module.
    pub fn get_module_functions(&self, module_id: ModuleId) -> Option<&HashMap<InternedText<'db>, ast::StmtFun<'db>>> {
        self.module_all_functions.get(&module_id)
    }
}
