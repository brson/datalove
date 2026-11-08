//! Worldfile assembly and entry point.

use rmx::prelude::*;
use rand::{Rng, SeedableRng};
use datalove_datalit::Database;
use crate::config::WorldGenConfig;
use crate::gen_module::{plan_module_graph, gen_module_type_aliases, gen_module_function_sigs, gen_module_generic_sigs, gen_module};
use crate::gen_script::{gen_script, gen_script_items};

/// Generate a complete worldfile from a seed.
///
/// Returns the worldfile as a string that can be parsed and typechecked.
pub fn gen_worldfile_seeded(seed: u64, config: WorldGenConfig) -> String {
    let db = Database::default();
    gen_worldfile_tracked(&db, seed, config)
}

/// A generated REPL session: the modules it requires, and its script units.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GenSession {
    /// The module sections, as worldfile text.
    pub library: String,
    /// The script units, in order.
    pub units: Vec<String>,
}

/// Generate a session from a seed: a worldfile's script cut into units.
pub fn gen_session_seeded(seed: u64, config: WorldGenConfig) -> GenSession {
    let db = Database::default();
    gen_session_tracked(&db, seed, config)
}

#[salsa::tracked(returns(clone))]
fn gen_session_tracked<'db>(
    db: &'db dyn salsa::Database,
    seed: u64,
    config: WorldGenConfig,
) -> GenSession {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let modules = gen_modules(db, &mut rng, &config);
    let library = modules.1.join("\n\n");
    let items = gen_script_items(db, &mut rng, &config, &modules.0);
    let mut units = Vec::new();
    let mut items = items.into_iter().peekable();
    while items.peek().is_some() {
        let count = rng.gen_range(1..=3);
        units.push(items.by_ref().take(count).collect::<Vec<_>>().join("\n"));
    }
    GenSession { library, units }
}

/// Internal tracked function for worldfile generation.
///
/// Must be tracked to allow creation of Salsa tracked structs.
#[salsa::tracked(returns(clone))]
fn gen_worldfile_tracked<'db>(
    db: &'db dyn salsa::Database,
    seed: u64,
    config: WorldGenConfig,
) -> String {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    gen_worldfile_inner(db, &mut rng, &config)
}

/// Generate a complete worldfile.
fn gen_worldfile_inner<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
) -> String {
    let (modules, mut sections) = gen_modules(db, rng, config);

    // Generate script section.
    let script_source = gen_script(db, rng, config, &modules);
    sections.push(format!(
        "----------\nscriptunit-fragment\n----------\n\n{}",
        script_source
    ));

    sections.join("\n\n")
}

/// Generate the modules, and their worldfile sections.
fn gen_modules<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
) -> (Vec<crate::context::ModuleInfo<'db>>, Vec<String>) {
    let mut sections = Vec::new();

    // Plan module graph.
    let mut modules = plan_module_graph(rng, config);

    // First pass: generate type aliases and function signatures for all modules.
    for (index, module) in modules.iter_mut().enumerate() {
        module.type_aliases = gen_module_type_aliases(db, rng, config);
        module.enums = crate::gen_enum::gen_module_enums(db, rng, config, index);
        module.consts = crate::gen_const::gen_module_consts(db, rng, config, index);
        module.generics = gen_module_generic_sigs(rng, config, index);
        module.functions = gen_module_function_sigs(db, rng, config);
    }

    // Second pass: generate complete module source.
    for i in 0..modules.len() {
        let prior_modules: Vec<_> = modules[..i].to_vec();
        let module = &modules[i];

        let source = gen_module(db, rng, module, config, &prior_modules);

        sections.push(format!(
            "----------\nmodule {}\n----------\n\n{}",
            module.path(),
            source
        ));
    }

    (modules, sections)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gen_worldfile_deterministic() {
        let config = WorldGenConfig::default();

        let wf1 = gen_worldfile_seeded(42, config.C());
        let wf2 = gen_worldfile_seeded(42, config);

        assert_eq!(wf1, wf2, "Same seed should produce identical worldfiles");
    }

    #[test]
    fn test_gen_worldfile_different_seeds() {
        let config = WorldGenConfig::default();

        let wf1 = gen_worldfile_seeded(1, config.C());
        let wf2 = gen_worldfile_seeded(2, config);

        assert_ne!(wf1, wf2, "Different seeds should produce different worldfiles");
    }

    #[test]
    fn test_gen_worldfile_has_structure() {
        let config = WorldGenConfig::default();
        let wf = gen_worldfile_seeded(123, config);

        // Should have module sections.
        assert!(wf.contains("----------"), "Should have separators");
        assert!(wf.contains("module local/"), "Should have module header");
        assert!(wf.contains("fun "), "Should have function definitions");
        assert!(wf.contains("end fun"), "Should have function end");
    }

    #[test]
    fn test_gen_worldfile_no_panic() {
        let config = WorldGenConfig::default();

        // Test 100 different seeds.
        for seed in 0..100 {
            let _wf = gen_worldfile_seeded(seed, config.C());
        }
    }
}
