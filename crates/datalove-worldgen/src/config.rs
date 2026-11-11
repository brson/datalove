//! Configuration for worldfile generation.

use rmx::prelude::*;

use bct::random::SeededRng;
use datalove_datalit::ast_gen::AstGenConfig;

/// Configuration for worldfile generation.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct WorldGenConfig {
    /// Number of modules to generate (min, max).
    pub module_count: (usize, usize),

    /// Number of functions per module (min, max).
    pub functions_per_module: (usize, usize),

    /// Number of statements per function (min, max).
    pub statements_per_function: (usize, usize),

    /// Number of generic functions per module (min, max).
    pub generics_per_module: (usize, usize),

    /// Probability of a statement calling a generic function (0-100 percent).
    pub generic_call_probability: u32,

    /// Number of type aliases per module (min, max).
    pub type_aliases_per_module: (usize, usize),

    /// Number of enums per module (min, max). An enum is what a `match` takes
    /// apart, and it has to be named for the match to name its variants.
    pub enums_per_module: (usize, usize),

    /// Number of consts per module (min, max).
    pub consts_per_module: (usize, usize),

    /// Probability of binding a const in a body, or of naming one where one
    /// has the wanted type.
    pub const_probability: u32,

    /// Probability of writing a `match` where there is something to match on.
    pub match_probability: u32,

    /// Number of statements in script (min, max).
    pub script_statements: (usize, usize),

    /// Probability that a script statement acts on a binding an earlier
    /// statement made: overwrites, updates, takes or revives it, or binds
    /// its name again. In a session the earlier statement is often in
    /// another unit, which is what the engine's reach has to follow. Zero
    /// draws nothing, so it leaves the rest of what a seed makes alone.
    pub binding_access_probability: u32,

    /// Probability that a script item is an expression unit rather than a
    /// statement: a live binding's name, or an expression of a simple type.
    /// Each is a unit of its own; a worldfile written as one fragment leaves
    /// them out. Zero draws nothing.
    pub expression_unit_probability: u32,

    /// Whether generated code shows what it binds: a script statement
    /// `debuglog`s each binding it made, and a function each binding it
    /// still holds before it returns. What is not shown is not compared
    /// between engines. Off draws nothing.
    pub observe_bindings: bool,

    /// Whether the script ends by calling each function it may import that
    /// it has not called yet, where it has the arguments to, so that the
    /// bodies run. Off draws nothing.
    pub call_every_function: bool,

    /// Whether the script's top level may return early with an error, as a
    /// function returning a result may: checked arithmetic, fallible
    /// indexing and unwrapping with `!`. A unit is checked as a function
    /// returning `!()`, so it can. Off draws nothing.
    pub fallible_script: bool,

    /// Whether a worldfile's script is one fragment rather than a section
    /// for each unit. The AOT backends take only one fragment. A session is
    /// cut into units either way.
    pub one_fragment: bool,

    /// Maximum nesting depth for control flow.
    pub max_control_flow_depth: usize,

    /// Probability of generating an if statement (0-100 percent).
    pub if_probability: u32,

    /// Probability of generating a loop statement (0-100 percent).
    pub loop_probability: u32,

    /// Probability that a loop body goes back round early, under a guard.
    pub continue_probability: u32,

    /// Probability that a `loop` counts its rounds and breaks after a few,
    /// rather than at the end of its first, which is as far as the osr
    /// engine's threshold has to be passed for it to enter one. Zero draws
    /// nothing.
    pub counted_loop_probability: u32,

    /// Probability that an `if` destructures an option or a result rather than
    /// testing a condition, where there is one in scope to destructure.
    pub if_binding_probability: u32,

    /// Probability that a statement is a `for` over a collection in scope,
    /// where there is one.
    pub for_probability: u32,

    /// Probability that a parameter is passed by something other than `in`
    /// (0-100 percent). Which of the other three is then chosen evenly.
    pub param_mode_probability: u32,

    /// Probability of reaching for a field of something in scope, or an
    /// element of a collection in scope, where one has the wanted type.
    pub projection_probability: u32,

    /// Probability that a function's return type is an option or a result
    /// over what it would otherwise have been. What a function returns is what
    /// says whether its body may write anything that early-returns.
    pub fallible_return_probability: u32,

    /// Probability of using a type alias instead of a structural type (0-100 percent).
    pub type_alias_usage_probability: u32,

    /// Probability of calling another function instead of a literal (0-100 percent).
    pub function_call_probability: u32,

    /// Probability of generating an arithmetic expression for floats/bigints (0-100 percent).
    pub arithmetic_probability: u32,

    /// Probability of generating a debuglog statement (0-100 percent).
    pub debuglog_probability: u32,

    /// Configuration for datalit expression generation.
    pub type_config: AstGenConfig,
}

impl WorldGenConfig {
    /// Check probability as bool using rng.
    pub fn check_probability(&self, rng: &mut SeededRng, pct: u32) -> bool {
        rng.random_range(0..100) < pct
    }
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
            generics_per_module: (1, 3),
            generic_call_probability: 35,
            type_aliases_per_module: (0, 2),
            enums_per_module: (1, 2),
            consts_per_module: (0, 2),
            const_probability: 30,
            match_probability: 40,
            script_statements: (1, 5),
            binding_access_probability: 0,
            expression_unit_probability: 0,
            one_fragment: false,
            observe_bindings: false,
            call_every_function: false,
            fallible_script: false,
            max_control_flow_depth: 2,
            if_probability: 30,
            loop_probability: 20,
            continue_probability: 40,
            counted_loop_probability: 0,
            if_binding_probability: 40,
            for_probability: 15,
            param_mode_probability: 35,
            projection_probability: 40,
            fallible_return_probability: 35,
            type_alias_usage_probability: 30,
            function_call_probability: 30,
            arithmetic_probability: 30,
            debuglog_probability: 20,
            type_config,
        }
    }
}
