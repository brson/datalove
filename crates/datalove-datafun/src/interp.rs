//! Tree-walking interpreter core.

use rmx::prelude::*;
use std::collections::HashMap;
use bct::text::InternedText;
use datalove_rt as rt;
use datalove_rtdt as rtdt;

use crate::ast::*;
use crate::value::Value;
use crate::type_table::TypeTable;

/// Interpreter execution result.
pub type InterpResult = Result<Value, InterpError>;

/// Interpreter error.
#[derive(Debug)]
pub enum InterpError {
    /// Type error during execution.
    TypeError(String),

    /// Unresolved name.
    UnresolvedName(String),

    /// Division by zero.
    DivisionByZero,

    /// Arithmetic overflow.
    ArithmeticOverflow,

    /// Not yet implemented.
    NotImplemented(String),

    /// Runtime error.
    RuntimeError(String),

    /// Return value from function (not a real error, used for control flow).
    Return(Value),

    /// Early return with None from try-option operator (? on None).
    ReturnNone,

    /// Early return with Error from try-result operator (! on Err).
    ReturnError(Value),

    /// Stack overflow (recursion too deep).
    StackOverflow,
}

/// Maximum call stack depth to prevent stack overflow.
pub const MAX_CALL_DEPTH: usize = 1000;

/// Interpreter context.
///
/// Holds the runtime allocator and variable/function bindings.
pub struct InterpContext<'db> {
    /// Salsa database.
    pub db: &'db dyn crate::Db,

    /// Runtime allocator.
    pub rt: Box<rt::rt_local::RtLocal>,

    /// Type table.
    pub type_table: TypeTable,

    /// Type descriptor table for instantiate2.
    pub tydesc_table: crate::datalit::tydesc_table::TyDescTable<'db>,

    /// Variable bindings (name -> value).
    pub variables: HashMap<InternedText<'db>, Value>,

    /// Function definitions (name -> definition).
    pub functions: HashMap<InternedText<'db>, StmtFun<'db>>,

    /// Module function definitions (module_alias -> (function_name -> definition)).
    /// Used for resolving import statements.
    module_functions: HashMap<InternedText<'db>, HashMap<InternedText<'db>, StmtFun<'db>>>,

    /// Current call stack depth (for recursion protection).
    pub call_depth: usize,

    /// Expected return type for the current function (for automatic coercion).
    pub expected_return_type: Option<crate::datalit::tycheck::TypeAndHeap<'db>>,
}

impl<'db> InterpContext<'db> {
    /// Create a new interpreter context.
    pub fn new(db: &'db dyn crate::Db, type_table: TypeTable) -> Self {
        Self {
            db,
            rt: rt::rt_local::RtLocal::new(),
            type_table,
            tydesc_table: crate::datalit::tydesc_table::TyDescTable::new(db),
            variables: HashMap::new(),
            functions: HashMap::new(),
            module_functions: HashMap::new(),
            call_depth: 0,
            expected_return_type: None,
        }
    }

    /// Create interpreter context with package world support.
    ///
    /// This pre-loads all module functions from the package world,
    /// enabling import statements to work.
    pub fn with_package_world(
        db: &'db dyn crate::Db,
        type_table: TypeTable,
        script: &crate::ast::Script<'db>,
        package_world: crate::package::PackageWorld,
        typecheck_result: &crate::tycheck::PackageWorldTypecheckResult<'db>,
    ) -> Self {
        let mut ctx = Self::new(db, type_table);

        // Build module alias map from require statements.
        let graph = typecheck_result.graph(db);
        let alias_map = crate::tycheck::build_script_module_alias_map(db, *script, package_world, graph);

        // For each required module, parse it and collect function definitions.
        for (module_alias, package_module) in alias_map {
            let source = package_module.text(db);
            let module_script = crate::parser::parse(db, source);

            let mut module_funcs = HashMap::new();
            for statement in module_script.statements(db) {
                if let crate::ast::Statement::Fun(fun) = statement {
                    let name = fun.name(db);
                    module_funcs.insert(name, *fun);
                }
            }

            ctx.module_functions.insert(module_alias, module_funcs);
        }

        ctx
    }

    /// Execute a script.
    pub fn execute(&mut self, script: Script<'db>) -> Result<(), InterpError> {
        // First pass: collect function definitions.
        for statement in script.statements(self.db) {
            if let Statement::Fun(fun) = statement {
                let name = fun.name(self.db);
                self.functions.insert(name, *fun);
            }
        }

        // Second pass: execute statements in order.
        for statement in script.statements(self.db) {
            self.exec_stmt(statement)?;
        }

        Ok(())
    }

    /// Execute a statement.
    pub fn exec_stmt(&mut self, statement: &Statement<'db>) -> Result<(), InterpError> {
        match statement {
            Statement::Let(stmt) => {
                let name = stmt.name(self.db);
                let value_expr = stmt.value(self.db);

                // If type hint is provided, convert it and pass to evaluator.
                let expected_type = stmt.type_hint(self.db)
                    .and_then(|type_hint| {
                        crate::eval_datalit::convert_type_hint_tracked(self.db, type_hint)
                    });

                let value = crate::eval_datafun::eval_expr_with_expected(self, value_expr, expected_type)?;
                self.variables.insert(name, value);

                Ok(())
            }

            Statement::Fun(_) => {
                // Functions are already collected in the first pass.
                Ok(())
            }

            Statement::Ret(stmt) => {
                // Evaluate the return value with expected type for automatic coercion.
                let expected = self.expected_return_type;
                let value = crate::eval_datafun::eval_expr_with_expected(self, stmt.value(self.db), expected)?;
                Err(InterpError::Return(value))
            }

            Statement::Require(_) => {
                // Require statements are processed during context creation.
                // No runtime action needed.
                Ok(())
            }

            Statement::Import(stmt) => {
                let module_name = stmt.module_name(self.db);
                let item_name = stmt.item_name(self.db);

                // Look up the module in module_functions.
                if let Some(module_funcs) = self.module_functions.get(&module_name) {
                    // Look up the function in the module.
                    if let Some(func) = module_funcs.get(&item_name) {
                        // Add the function to the local function table.
                        self.functions.insert(item_name, *func);
                        Ok(())
                    } else {
                        Err(InterpError::UnresolvedName(
                            format!("{}.{}", module_name.as_str(self.db), item_name.as_str(self.db))
                        ))
                    }
                } else {
                    Err(InterpError::UnresolvedName(
                        format!("module {} (not required)", module_name.as_str(self.db))
                    ))
                }
            }

            Statement::If(stmt) => {
                let condition = stmt.condition(self.db);
                let then_binding = stmt.then_binding(self.db);
                let else_binding = stmt.else_binding(self.db);
                let then_body = stmt.then_body(self.db);
                let else_body = stmt.else_body(self.db);

                // Check if we're doing Option/Result destructuring or boolean condition.
                if then_binding.is_some() || else_binding.is_some() {
                    // Option/Result destructuring path.
                    self.exec_if_destructuring(
                        condition,
                        then_binding,
                        else_binding,
                        then_body,
                        else_body.as_ref(),
                    )
                } else {
                    // Boolean condition path.
                    // Evaluate condition.
                    let condition_value = crate::eval_datafun::eval_expr(self, condition)?;

                    // Condition must be a bool.
                    let condition_bool = match condition_value {
                        Value::Bool(b) => b,
                        _ => {
                            return Err(InterpError::TypeError(
                                "condition must be bool".to_string()
                            ));
                        }
                    };

                    // Execute appropriate branch.
                    if condition_bool {
                        for stmt in then_body {
                            self.exec_stmt(stmt)?;
                        }
                    } else if let Some(else_stmts) = else_body {
                        for stmt in else_stmts {
                            self.exec_stmt(stmt)?;
                        }
                    }

                    Ok(())
                }
            }

            Statement::ParseError(err) => {
                let message = err.message(self.db);
                Err(InterpError::RuntimeError(
                    format!("Parse error: {}", message.as_str(self.db)),
                ))
            }
        }
    }

    /// Execute an if statement with Option/Result destructuring.
    fn exec_if_destructuring(
        &mut self,
        condition: ExprFun<'db>,
        then_binding: Option<InternedText<'db>>,
        else_binding: Option<InternedText<'db>>,
        then_body: &[Statement<'db>],
        else_body: Option<&Vec<Statement<'db>>>,
    ) -> Result<(), InterpError> {
        use rtdt::OptionTag;
        use rtdt::ResultTag;

        // Evaluate the condition to get the Option/Result value.
        let condition_value = crate::eval_datafun::eval_expr(self, condition)?;

        // Get the type descriptor from the value itself.
        let condition_tydesc = condition_value.tydesc();
        if condition_tydesc.is_null() {
            return Err(InterpError::TypeError(
                format!("Cannot determine type of condition value: {:?}", condition_value)
            ));
        }

        // Check if it's Option or Result.
        let type_tag = unsafe { (*condition_tydesc).type_tag };

        match type_tag {
            rtdt::TyTag::Option => {
                // Extract the tag from the Option value.
                let option_ptr = match condition_value {
                    Value::Option { ptr, .. } => ptr,
                    _ => {
                        return Err(InterpError::TypeError(
                            "Expected Option value for if-destructuring".to_string()
                        ));
                    }
                };

                let tag = unsafe { *(option_ptr as *const u8) as u8 };
                let option_tag = if tag == OptionTag::Some as u8 {
                    OptionTag::Some
                } else {
                    OptionTag::None
                };

                match option_tag {
                    OptionTag::Some => {
                        // Extract payload and execute then branch.
                        let layout = unsafe { rtdt::layout::compute_option_layout(condition_tydesc) };
                        let payload_ptr = unsafe { option_ptr.add(layout.payload_offset as usize) };

                        // Get the inner type descriptor.
                        let inner_tydesc = unsafe { (*condition_tydesc).type_info.option.inner_tydesc };

                        // Create a Value for the payload.
                        let payload_value = Self::value_from_ptr(&mut self.rt, payload_ptr, inner_tydesc)?;

                        // Bind the payload if there's a then_binding.
                        if let Some(binding_name) = then_binding {
                            self.variables.insert(binding_name, payload_value);
                        }

                        // Execute then branch.
                        for stmt in then_body {
                            self.exec_stmt(stmt)?;
                        }

                        // Remove the binding.
                        if let Some(binding_name) = then_binding {
                            if let Some(mut old_value) = self.variables.remove(&binding_name) {
                                unsafe { old_value.free(&mut self.rt); }
                            }
                        }
                    }
                    OptionTag::None => {
                        // Execute else branch if it exists.
                        if let Some(else_stmts) = else_body {
                            for stmt in else_stmts {
                                self.exec_stmt(stmt)?;
                            }
                        }
                    }
                }

                Ok(())
            }

            rtdt::TyTag::Result => {
                // Extract the tag from the Result value.
                let result_ptr = match condition_value {
                    Value::Result { ptr, .. } => ptr,
                    _ => {
                        return Err(InterpError::TypeError(
                            "Expected Result value for if-destructuring".to_string()
                        ));
                    }
                };

                let tag = unsafe { *(result_ptr as *const u8) as u8 };
                let result_tag = if tag == ResultTag::Ok as u8 {
                    ResultTag::Ok
                } else {
                    ResultTag::Err
                };

                match result_tag {
                    ResultTag::Ok => {
                        // Extract Ok payload and execute then branch.
                        let layout = unsafe { rtdt::layout::compute_result_layout(condition_tydesc) };
                        let payload_ptr = unsafe { result_ptr.add(layout.payload_offset as usize) };

                        // Get the Ok type descriptor.
                        let ok_tydesc = unsafe { (*condition_tydesc).type_info.result.ok_tydesc };

                        // Create a Value for the payload.
                        let payload_value = Self::value_from_ptr(&mut self.rt, payload_ptr, ok_tydesc)?;

                        // Bind the payload if there's a then_binding.
                        if let Some(binding_name) = then_binding {
                            self.variables.insert(binding_name, payload_value);
                        }

                        // Execute then branch.
                        for stmt in then_body {
                            self.exec_stmt(stmt)?;
                        }

                        // Remove the binding.
                        if let Some(binding_name) = then_binding {
                            if let Some(mut old_value) = self.variables.remove(&binding_name) {
                                unsafe { old_value.free(&mut self.rt); }
                            }
                        }
                    }
                    ResultTag::Err => {
                        // Extract Err payload and execute else branch.
                        if let Some(else_stmts) = else_body {
                            let layout = unsafe { rtdt::layout::compute_result_layout(condition_tydesc) };
                            let payload_ptr = unsafe { result_ptr.add(layout.payload_offset as usize) };

                            // The error payload is of type Error (16 bytes containing tydesc + value_ptr).
                            // We need to move it out of the Result to avoid cloning the inner value.
                            // Error is stored inline in Result, so we copy the Error struct itself
                            // and transfer ownership of what it points to.
                            let error_ptr = payload_ptr as *const rtdt::Error;

                            // Allocate a new Error on the heap and copy the Error struct.
                            let error_tydesc = unsafe {
                                // Get Error type descriptor from error type table.
                                // For now we'll get it from the error itself.
                                // TODO: This might need the proper Error tydesc from type table.
                                std::ptr::null() as *const rtdt::TyDesc
                            };

                            let error_size = std::mem::size_of::<rtdt::Error>();
                            let error_align = std::mem::align_of::<rtdt::Error>();
                            let new_error_ptr = unsafe {
                                self.rt.alloc.alloc(error_size as u32, error_align as u32, 1)
                            };

                            if new_error_ptr.is_null() {
                                return Err(InterpError::RuntimeError(
                                    "Failed to allocate Error in destructuring".to_string()
                                ));
                            }

                            // Copy the Error struct (transfers ownership of inner value_ptr).
                            unsafe {
                                std::ptr::copy_nonoverlapping(
                                    error_ptr as *const u8,
                                    new_error_ptr,
                                    error_size
                                );
                            }

                            // Create Value::Error pointing to the moved Error.
                            let error_value = Value::Error {
                                ptr: new_error_ptr as *mut rtdt::Error,
                                tydesc: error_tydesc,
                            };

                            // Now we need to prevent the Result from freeing the Error's inner value.
                            // Zero out the Error in the Result so it won't double-free.
                            unsafe {
                                std::ptr::write_bytes(payload_ptr, 0, error_size);
                            }

                            // Bind the error if there's an else_binding.
                            if let Some(binding_name) = else_binding {
                                self.variables.insert(binding_name, error_value);
                            }

                            // Execute else branch.
                            for stmt in else_stmts {
                                self.exec_stmt(stmt)?;
                            }

                            // Remove the binding.
                            if let Some(binding_name) = else_binding {
                                if let Some(mut old_value) = self.variables.remove(&binding_name) {
                                    unsafe { old_value.free(&mut self.rt); }
                                }
                            }
                        }
                    }
                }

                Ok(())
            }

            _ => {
                Err(InterpError::TypeError(
                    format!("If-destructuring requires Option or Result type, got {:?}", type_tag)
                ))
            }
        }
    }

    /// Create a Value from a pointer and type descriptor.
    ///
    /// This clones the value at the given pointer into a new allocation.
    pub(crate) fn value_from_ptr(
        rt: &mut rt::rt_local::RtLocal,
        ptr: *const u8,
        tydesc: *const rtdt::TyDesc
    ) -> Result<Value, InterpError> {
        if tydesc.is_null() {
            return Err(InterpError::TypeError(
                "Cannot create value from null type descriptor".to_string()
            ));
        }

        let type_tag = unsafe { (*tydesc).type_tag };

        match type_tag {
            rtdt::TyTag::Bool => {
                let value = unsafe { *(ptr as *const bool) };
                Ok(Value::Bool(value))
            }
            rtdt::TyTag::U8 => {
                let value = unsafe { *(ptr as *const u8) };
                Ok(Value::U32(value as u32))
            }
            rtdt::TyTag::I8 => {
                let value = unsafe { *(ptr as *const i8) };
                Ok(Value::U32((value as i32) as u32))
            }
            rtdt::TyTag::U16 => {
                let value = unsafe { *(ptr as *const u16) };
                Ok(Value::U32(value as u32))
            }
            rtdt::TyTag::I16 => {
                let value = unsafe { *(ptr as *const i16) };
                Ok(Value::U32((value as i32) as u32))
            }
            rtdt::TyTag::U32 => {
                let value = unsafe { *(ptr as *const u32) };
                Ok(Value::U32(value))
            }
            rtdt::TyTag::I32 => {
                let value = unsafe { *(ptr as *const i32) };
                Ok(Value::U32(value as u32))
            }
            rtdt::TyTag::U64 => {
                let value = unsafe { *(ptr as *const u64) };
                Ok(Value::U32(value as u32))
            }
            rtdt::TyTag::I64 => {
                let value = unsafe { *(ptr as *const i64) };
                Ok(Value::U32(value as u32))
            }
            rtdt::TyTag::F32 => {
                let value = unsafe { *(ptr as *const f32) };
                Ok(Value::F32(value))
            }
            rtdt::TyTag::F64 => {
                let value = unsafe { *(ptr as *const f64) };
                Ok(Value::F32(value as f32))
            }
            rtdt::TyTag::Int => {
                let mut new_value = unsafe { Value::alloc_int(rt, tydesc) };
                let status = unsafe {
                    rt::clone::clone_value(
                        rt as *mut _ as rt::LocalRtHandle,
                        ptr,
                        tydesc,
                        new_value.as_mut_ptr(),
                    )
                };
                if status != rt::RtStatus::Ok {
                    return Err(InterpError::RuntimeError(
                        "Failed to clone Int value".to_string()
                    ));
                }
                Ok(new_value)
            }
            rtdt::TyTag::String => {
                let mut new_value = unsafe { Value::alloc_string(rt, tydesc) };
                let status = unsafe {
                    rt::clone::clone_value(
                        rt as *mut _ as rt::LocalRtHandle,
                        ptr,
                        tydesc,
                        new_value.as_mut_ptr(),
                    )
                };
                if status != rt::RtStatus::Ok {
                    return Err(InterpError::RuntimeError(
                        "Failed to clone String value".to_string()
                    ));
                }
                Ok(new_value)
            }
            rtdt::TyTag::Tuple => {
                let mut new_value = unsafe { Value::alloc_tuple(rt, tydesc) };
                let status = unsafe {
                    rt::clone::clone_value(
                        rt as *mut _ as rt::LocalRtHandle,
                        ptr,
                        tydesc,
                        new_value.as_mut_ptr(),
                    )
                };
                if status != rt::RtStatus::Ok {
                    return Err(InterpError::RuntimeError(
                        "Failed to clone Tuple value".to_string()
                    ));
                }
                Ok(new_value)
            }
            rtdt::TyTag::Struct => {
                let mut new_value = unsafe { Value::alloc_struct(rt, tydesc) };
                let status = unsafe {
                    rt::clone::clone_value(
                        rt as *mut _ as rt::LocalRtHandle,
                        ptr,
                        tydesc,
                        new_value.as_mut_ptr(),
                    )
                };
                if status != rt::RtStatus::Ok {
                    return Err(InterpError::RuntimeError(
                        "Failed to clone Struct value".to_string()
                    ));
                }
                Ok(new_value)
            }
            rtdt::TyTag::Enum => {
                let mut new_value = unsafe { Value::alloc_enum(rt, tydesc) };
                let status = unsafe {
                    rt::clone::clone_value(
                        rt as *mut _ as rt::LocalRtHandle,
                        ptr,
                        tydesc,
                        new_value.as_mut_ptr(),
                    )
                };
                if status != rt::RtStatus::Ok {
                    return Err(InterpError::RuntimeError(
                        "Failed to clone Enum value".to_string()
                    ));
                }
                Ok(new_value)
            }
            rtdt::TyTag::List => {
                let mut new_value = unsafe { Value::alloc_list(rt, tydesc) };
                let status = unsafe {
                    rt::clone::clone_value(
                        rt as *mut _ as rt::LocalRtHandle,
                        ptr,
                        tydesc,
                        new_value.as_mut_ptr(),
                    )
                };
                if status != rt::RtStatus::Ok {
                    return Err(InterpError::RuntimeError(
                        "Failed to clone List value".to_string()
                    ));
                }
                Ok(new_value)
            }
            rtdt::TyTag::Map => {
                let mut new_value = unsafe { Value::alloc_map(rt, tydesc) };
                let status = unsafe {
                    rt::clone::clone_value(
                        rt as *mut _ as rt::LocalRtHandle,
                        ptr,
                        tydesc,
                        new_value.as_mut_ptr(),
                    )
                };
                if status != rt::RtStatus::Ok {
                    return Err(InterpError::RuntimeError(
                        "Failed to clone Map value".to_string()
                    ));
                }
                Ok(new_value)
            }
            rtdt::TyTag::Set => {
                let mut new_value = unsafe { Value::alloc_set(rt, tydesc) };
                let status = unsafe {
                    rt::clone::clone_value(
                        rt as *mut _ as rt::LocalRtHandle,
                        ptr,
                        tydesc,
                        new_value.as_mut_ptr(),
                    )
                };
                if status != rt::RtStatus::Ok {
                    return Err(InterpError::RuntimeError(
                        "Failed to clone Set value".to_string()
                    ));
                }
                Ok(new_value)
            }
            rtdt::TyTag::Option => {
                let mut new_value = unsafe { Value::alloc_option(rt, tydesc) };
                let status = unsafe {
                    rt::clone::clone_value(
                        rt as *mut _ as rt::LocalRtHandle,
                        ptr,
                        tydesc,
                        new_value.as_mut_ptr(),
                    )
                };
                if status != rt::RtStatus::Ok {
                    return Err(InterpError::RuntimeError(
                        "Failed to clone Option value".to_string()
                    ));
                }
                Ok(new_value)
            }
            rtdt::TyTag::Result => {
                let mut new_value = unsafe { Value::alloc_result(rt, tydesc) };
                let status = unsafe {
                    rt::clone::clone_value(
                        rt as *mut _ as rt::LocalRtHandle,
                        ptr,
                        tydesc,
                        new_value.as_mut_ptr(),
                    )
                };
                if status != rt::RtStatus::Ok {
                    return Err(InterpError::RuntimeError(
                        "Failed to clone Result value".to_string()
                    ));
                }
                Ok(new_value)
            }
            _ => {
                Err(InterpError::NotImplemented(
                    format!("Cloning {:?} values in if-destructuring not yet implemented", type_tag)
                ))
            }
        }
    }

    /// Look up a variable.
    pub fn lookup_variable(&self, name: InternedText<'db>) -> Result<&Value, InterpError> {
        self.variables
            .get(&name)
            .ok_or_else(|| InterpError::UnresolvedName(name.as_str(self.db).to_string()))
    }

    /// Pretty-print a variable's value.
    pub fn pretty_print_variable(&mut self, name: InternedText<'db>) -> Result<String, InterpError> {
        // Look up the variable first to check it exists.
        if !self.variables.contains_key(&name) {
            return Err(InterpError::UnresolvedName(name.as_str(self.db).to_string()));
        }

        // Get the value (need to work around borrow checker).
        let value = self.variables.get(&name).unwrap();
        value.pretty_print(&mut self.rt, &mut self.tydesc_table)
    }

    /// Update the type table.
    ///
    /// Used by the REPL to incrementally update the type table
    /// without recreating the entire context.
    pub fn update_type_table(&mut self, type_table: crate::type_table::TypeTable) {
        self.type_table = type_table;
    }
}

impl<'db> Drop for InterpContext<'db> {
    fn drop(&mut self) {
        // Free all values in the context.
        for (_, mut value) in self.variables.drain() {
            unsafe {
                value.free(&mut self.rt);
            }
        }
    }
}
