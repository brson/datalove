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
}

/// Interpreter context.
///
/// Holds the runtime allocator and variable/function bindings.
pub struct InterpContext<'db> {
    /// Salsa database.
    pub db: &'db dyn crate::Db,

    /// Runtime allocator.
    pub rt: Box<rt::alloc::LocalRt>,

    /// Type table.
    pub type_table: TypeTable,

    /// Variable bindings (name -> value).
    pub variables: HashMap<InternedText<'db>, Value>,

    /// Function definitions (name -> definition).
    pub functions: HashMap<InternedText<'db>, StmtFun<'db>>,
}

impl<'db> InterpContext<'db> {
    /// Create a new interpreter context.
    pub fn new(db: &'db dyn crate::Db, type_table: TypeTable) -> Self {
        Self {
            db,
            rt: rt::alloc::LocalRt::new(),
            type_table,
            variables: HashMap::new(),
            functions: HashMap::new(),
        }
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

                let value = crate::eval_datafun::eval_expr(self, value_expr)?;
                self.variables.insert(name, value);

                Ok(())
            }

            Statement::Fun(_) => {
                // Functions are already collected in the first pass.
                Ok(())
            }

            Statement::Ret(_) => {
                // Return statements are handled by function execution.
                Err(InterpError::RuntimeError(
                    "Return statement outside of function".to_string(),
                ))
            }

            Statement::Require(_) => {
                // TODO: implement module loading.
                Err(InterpError::NotImplemented("require statement".to_string()))
            }

            Statement::ParseError(err) => {
                let message = err.message(self.db);
                Err(InterpError::RuntimeError(
                    format!("Parse error: {}", message.as_str(self.db)),
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
        value.pretty_print(&mut self.rt)
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
