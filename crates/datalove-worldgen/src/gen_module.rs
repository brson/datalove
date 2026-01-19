//! Module generation.

use rand::Rng;
use crate::config::WorldGenConfig;
use crate::context::{GenContext, ModuleInfo, TypeAlias, FunctionSig};
use crate::gen_type::gen_type_alias;
use crate::gen_function::{gen_function_signature, gen_function};

/// Generate a module name.
fn gen_module_name<R: Rng>(rng: &mut R, index: usize) -> String {
    let prefixes = ["util", "core", "data", "math", "str", "io"];
    let prefix = prefixes[rng.gen_range(0..prefixes.len())];
    format!("{}_{}", prefix, index)
}

/// Plan the module graph (DAG of dependencies).
pub fn plan_module_graph<R: Rng>(
    rng: &mut R,
    config: &WorldGenConfig,
) -> Vec<ModuleInfo> {
    let module_count = rng.gen_range(config.module_count.0..=config.module_count.1);
    let mut modules = Vec::new();

    for i in 0..module_count {
        let module_name = gen_module_name(rng, i);
        modules.push(ModuleInfo {
            library: "local".to_string(),
            package: "gen".to_string(),
            module: module_name,
            functions: Vec::new(),
            type_aliases: Vec::new(),
        });
    }

    modules
}

/// Generate type alias definitions for a module.
pub fn gen_module_type_aliases<R: Rng>(
    rng: &mut R,
    config: &WorldGenConfig,
) -> Vec<TypeAlias> {
    let count = rng.gen_range(config.type_aliases_per_module.0..=config.type_aliases_per_module.1);
    let mut aliases = Vec::new();

    for i in 0..count {
        let name = format!("Type{}", i);
        let type_hint = gen_type_alias(rng, &name, config);
        // Extract the type from "type Name: type" format.
        let parts: Vec<&str> = type_hint.splitn(2, ": ").collect();
        let actual_type = if parts.len() == 2 {
            parts[1].to_string()
        } else {
            "@u32".to_string()
        };
        aliases.push(TypeAlias {
            name,
            type_hint: actual_type,
        });
    }

    aliases
}

/// Generate function signatures for a module (first pass).
pub fn gen_module_function_sigs<R: Rng>(
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &GenContext,
) -> Vec<FunctionSig> {
    let count = rng.gen_range(config.functions_per_module.0..=config.functions_per_module.1);
    let mut sigs = Vec::new();

    for i in 0..count {
        let name = format!("fn{}", i);
        let sig = gen_function_signature(rng, &name, config, ctx);
        sigs.push(sig);
    }

    sigs
}

/// Generate a complete module.
pub fn gen_module<R: Rng>(
    rng: &mut R,
    info: &ModuleInfo,
    config: &WorldGenConfig,
    prior_modules: &[ModuleInfo],
) -> String {
    let mut lines = Vec::new();

    // Generate require/import for prior modules.
    for prior in prior_modules {
        if !prior.functions.is_empty() && rng.gen_bool(0.5) {
            lines.push(format!("require module {}", prior.path()));

            // Import some functions.
            for func in &prior.functions {
                if rng.gen_bool(0.7) {
                    lines.push(format!("import {}.{}", prior.alias(), func.name));
                }
            }
            lines.push(String::new());
        }
    }

    // Build module context.
    let mut ctx = GenContext::new();
    ctx.type_aliases = info.type_aliases.clone();
    ctx.functions = info.functions.clone();

    // Add imported functions from prior modules.
    for prior in prior_modules {
        for func in &prior.functions {
            // Check if we imported it.
            let import_line = format!("import {}.{}", prior.alias(), func.name);
            if lines.contains(&import_line) {
                ctx.imported_functions.push(func.clone());
            }
        }
    }

    // Generate type alias statements.
    for alias in &info.type_aliases {
        lines.push(format!("type {}: {}", alias.name, alias.type_hint));
    }
    if !info.type_aliases.is_empty() {
        lines.push(String::new());
    }

    // Generate function definitions.
    for sig in &info.functions {
        let func_def = gen_function(rng, sig, config, &ctx);
        lines.push(func_def);
        lines.push(String::new());
    }

    // Remove trailing empty line.
    while lines.last().map(|s| s.is_empty()).unwrap_or(false) {
        lines.pop();
    }

    lines.join("\n")
}
