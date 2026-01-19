//! Configuration for worldfile generation.

use datalove_datalit::ast_gen::AstGenConfig;

/// Configuration for worldfile generation.
#[derive(Clone, Debug)]
pub struct WorldGenConfig {
    /// Number of modules to generate (min, max).
    pub module_count: (usize, usize),

    /// Number of functions per module (min, max).
    pub functions_per_module: (usize, usize),

    /// Number of statements per function (min, max).
    pub statements_per_function: (usize, usize),

    /// Number of type aliases per module (min, max).
    pub type_aliases_per_module: (usize, usize),

    /// Number of statements in script (min, max).
    pub script_statements: (usize, usize),

    /// Maximum nesting depth for control flow.
    pub max_control_flow_depth: usize,

    /// Probability of generating an if statement (0.0-1.0).
    pub if_probability: f64,

    /// Probability of generating a loop statement (0.0-1.0).
    pub loop_probability: f64,

    /// Probability of using a type alias instead of a structural type.
    pub type_alias_usage_probability: f64,

    /// Probability of calling another function instead of a literal.
    pub function_call_probability: f64,

    /// Configuration for datalit expression generation.
    pub type_config: AstGenConfig,
}

impl Default for WorldGenConfig {
    fn default() -> Self {
        let mut type_config = AstGenConfig::default();
        // Use small non-negative numbers for predictable behavior.
        type_config.numeric_strategy = datalove_datalit::ast_gen::NumericStrategy::SmallNonNegative;
        // Reduce depth for simpler types.
        type_config.max_depth = 2;
        // Smaller collections.
        type_config.max_collection_size = 3;

        WorldGenConfig {
            module_count: (1, 3),
            functions_per_module: (1, 3),
            statements_per_function: (1, 5),
            type_aliases_per_module: (0, 2),
            script_statements: (1, 5),
            max_control_flow_depth: 2,
            if_probability: 0.3,
            loop_probability: 0.2,
            type_alias_usage_probability: 0.3,
            function_call_probability: 0.3,
            type_config,
        }
    }
}
