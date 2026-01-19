//! Generation context for tracking scope during worldfile generation.

use crate::config::WorldGenConfig;

/// A variable in scope.
#[derive(Clone, Debug)]
pub struct Variable {
    pub name: String,
    pub type_hint: String,
    pub is_mutable: bool,
}

/// A function signature.
#[derive(Clone, Debug)]
pub struct FunctionSig {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub return_type: Option<String>,
}

/// A type alias.
#[derive(Clone, Debug)]
pub struct TypeAlias {
    pub name: String,
    pub type_hint: String,
}

/// A module in the worldfile.
#[derive(Clone, Debug)]
pub struct ModuleInfo {
    pub library: String,
    pub package: String,
    pub module: String,
    pub functions: Vec<FunctionSig>,
    pub type_aliases: Vec<TypeAlias>,
}

impl ModuleInfo {
    /// Get the full module path (e.g., "sys/std/u32").
    pub fn path(&self) -> String {
        format!("{}/{}/{}", self.library, self.package, self.module)
    }

    /// Get the module alias for imports (e.g., "u32").
    pub fn alias(&self) -> &str {
        &self.module
    }
}

/// Generation context tracking what's in scope.
#[derive(Clone, Debug)]
pub struct GenContext {
    /// Variables in scope.
    pub variables: Vec<Variable>,

    /// Functions available in this module.
    pub functions: Vec<FunctionSig>,

    /// Type aliases defined in this module.
    pub type_aliases: Vec<TypeAlias>,

    /// Imported functions from other modules.
    pub imported_functions: Vec<FunctionSig>,

    /// Expected return type for current function.
    pub return_type: Option<String>,

    /// Current loop nesting depth.
    pub loop_depth: usize,

    /// Current control flow nesting depth.
    pub control_flow_depth: usize,
}

impl GenContext {
    pub fn new() -> Self {
        GenContext {
            variables: Vec::new(),
            functions: Vec::new(),
            type_aliases: Vec::new(),
            imported_functions: Vec::new(),
            return_type: None,
            loop_depth: 0,
            control_flow_depth: 0,
        }
    }

    /// Get all callable functions (local + imported, with shadowing).
    ///
    /// Local functions shadow imported functions with the same name.
    pub fn callable_functions(&self) -> impl Iterator<Item = &FunctionSig> {
        // Collect local function names for shadowing check.
        let local_names: std::collections::HashSet<_> =
            self.functions.iter().map(|f| f.name.as_str()).collect();

        // Return local functions + non-shadowed imports.
        self.functions.iter().chain(
            self.imported_functions
                .iter()
                .filter(move |f| !local_names.contains(f.name.as_str())),
        )
    }

    /// Find variables of a specific type.
    pub fn variables_of_type(&self, type_hint: &str) -> Vec<&Variable> {
        self.variables
            .iter()
            .filter(|v| v.type_hint == type_hint)
            .collect()
    }

    /// Find mutable variables.
    pub fn mutable_variables(&self) -> Vec<&Variable> {
        self.variables
            .iter()
            .filter(|v| v.is_mutable)
            .collect()
    }

    /// Check if we've reached max control flow depth.
    pub fn at_max_depth(&self, config: &WorldGenConfig) -> bool {
        self.control_flow_depth >= config.max_control_flow_depth
    }
}

impl Default for GenContext {
    fn default() -> Self {
        Self::new()
    }
}
