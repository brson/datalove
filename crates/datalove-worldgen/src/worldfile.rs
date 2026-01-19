//! Worldfile assembly and entry point.

use rand::{Rng, SeedableRng};
use datalove_datalit::Database;
use crate::config::WorldGenConfig;
use crate::gen_module::{plan_module_graph, gen_module_type_aliases, gen_module_function_sigs, gen_module};
use crate::gen_script::gen_script;

/// Generate a complete worldfile from a seed.
///
/// Returns the worldfile as a string that can be parsed and typechecked.
pub fn gen_worldfile_seeded(seed: u64, config: WorldGenConfig) -> String {
    let db = Database::default();
    gen_worldfile_tracked(&db, seed, config)
}

/// Internal tracked function for worldfile generation.
///
/// Must be tracked to allow creation of Salsa tracked structs.
#[salsa::tracked]
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
    let mut sections = Vec::new();

    // Plan module graph.
    let mut modules = plan_module_graph(rng, config);

    // First pass: generate type aliases and function signatures for all modules.
    for module in &mut modules {
        module.type_aliases = gen_module_type_aliases(db, rng, config);
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

    // Generate script section.
    let script_source = gen_script(db, rng, config, &modules);
    sections.push(format!(
        "----------\nscript\n----------\n\n{}",
        script_source
    ));

    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gen_worldfile_deterministic() {
        let config = WorldGenConfig::default();

        let wf1 = gen_worldfile_seeded(42, config.clone());
        let wf2 = gen_worldfile_seeded(42, config);

        assert_eq!(wf1, wf2, "Same seed should produce identical worldfiles");
    }

    #[test]
    fn test_gen_worldfile_different_seeds() {
        let config = WorldGenConfig::default();

        let wf1 = gen_worldfile_seeded(1, config.clone());
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
            let _wf = gen_worldfile_seeded(seed, config.clone());
        }
    }
}
