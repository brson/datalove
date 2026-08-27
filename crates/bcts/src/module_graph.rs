//! Module graph abstraction.
//!
//! Provides a package-agnostic view of modules for compilation.
//! `ModuleGraph` represents a dependency-ordered collection of modules.

use rmx::prelude::*;
use rmx::std::collections::{BTreeMap, BTreeSet};
use crate::input::Source;

/// Opaque module identifier.
///
/// Modules are identified by their path string (e.g., "sys/std/u32"), which
/// is what interning means: the same path yields the same id, wherever it is
/// asked for. As an input it did not, and every caller had to route through
/// whoever happened to construct the id first.
///
/// `revisions = usize::MAX` keeps a module id interned for the life of the
/// database, which is what lets the handle drop the database lifetime. There
/// are as many of these as there are modules, so nothing is lost by holding
/// them, and callers pass module ids around too freely for a borrow to be
/// practical.
#[salsa::interned(revisions = usize::MAX, unsafe(no_lifetime))]
#[derive(Debug, Ord, PartialOrd)]
pub struct ModuleId {
    /// Module path (e.g., "sys/std/u32").
    #[returns(ref)]
    pub path: String,
}

/// A module in the graph: an identifier and the source behind it.
///
/// Interned, so building the same module twice is the same module. It is never
/// mutated - editing a module edits its `Source`, whose handle does not change
/// - so there is nothing an input would give it beyond a fresh identity each
/// time it is built, which is the thing to avoid.
///
/// Lifetime-free because `ModuleGraph` is an input and an input's fields
/// cannot borrow the database. See `ModuleId` for what that costs.
#[salsa::interned(revisions = usize::MAX, unsafe(no_lifetime))]
#[derive(Debug)]
pub struct Module {
    /// Module identifier.
    #[returns(copy)]
    pub id: ModuleId,
    /// Module source text.
    #[returns(copy)]
    pub source: Source,
}

/// The module graph: a dependency-ordered collection of modules.
///
/// Contains all modules in topological order (dependencies before dependents).
/// Function-level imports are resolved by the typechecker, not stored here.
///
/// Interned, so a graph built twice from the same modules is one graph. As an
/// input it was not, and a caller wanting a stable graph across edits had to
/// hold on to the first one and drive its setters - carefully, since a setter
/// marks an input changed whether or not the value differs.
#[salsa::interned(revisions = usize::MAX, unsafe(no_lifetime))]
pub struct ModuleGraph {
    /// Modules in dependency order (dependencies come first).
    #[returns(ref)]
    pub modules: Vec<Module>,

    /// Module lookup by ID.
    #[returns(ref)]
    pub module_by_id: BTreeMap<ModuleId, Module>,

    /// Direct dependencies per module (for ordering verification).
    #[returns(ref)]
    pub dependencies: BTreeMap<ModuleId, BTreeSet<ModuleId>>,
}

impl ModuleGraph {
    /// Get a module by its ID.
    pub fn get_module(&self, db: &dyn salsa::Database, id: ModuleId) -> Option<Module> {
        self.module_by_id(db).get(&id).copied()
    }

    /// Iterate modules in dependency order.
    pub fn iter_modules<'db>(&self, db: &'db dyn salsa::Database) -> impl Iterator<Item = Module> + 'db {
        self.modules(db).iter().copied()
    }
}

/// Builder for constructing a ModuleGraph.
pub struct ModuleGraphBuilder<'db> {
    db: &'db dyn salsa::Database,
    modules: Vec<Module>,
    module_by_id: BTreeMap<ModuleId, Module>,
    dependencies: BTreeMap<ModuleId, BTreeSet<ModuleId>>,
}

impl<'db> ModuleGraphBuilder<'db> {
    /// Create a new builder.
    pub fn new(db: &'db dyn salsa::Database) -> Self {
        Self {
            db,
            modules: Vec::new(),
            module_by_id: BTreeMap::new(),
            dependencies: BTreeMap::new(),
        }
    }

    /// Add a module to the graph.
    ///
    /// Modules must be added in dependency order (dependencies first).
    pub fn add_module(
        &mut self,
        path: impl Into<String>,
        source: Source,
    ) -> ModuleId {
        let id = ModuleId::new(self.db, path.into());
        let module = Module::new(self.db, id, source);
        self.modules.push(module);
        self.module_by_id.insert(id, module);
        self.dependencies.insert(id, BTreeSet::new());
        id
    }

    /// Add a dependency between modules.
    pub fn add_dependency(&mut self, module_id: ModuleId, depends_on: ModuleId) {
        if let Some(deps) = self.dependencies.get_mut(&module_id) {
            deps.insert(depends_on);
        }
    }

    /// Build the final ModuleGraph.
    pub fn build(self) -> ModuleGraph {
        ModuleGraph::new(
            self.db,
            self.modules,
            self.module_by_id,
            self.dependencies,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_graph_builder() {
        let db = crate::Database::default();
        let mut builder = ModuleGraphBuilder::new(&db);

        // Add modules in dependency order.
        let base = builder.add_module("sys/std/base", Source::new(&db, S("// base")));
        let math = builder.add_module("sys/std/math", Source::new(&db, S("// math")));

        // math depends on base.
        builder.add_dependency(math, base);

        let graph = builder.build();

        // Verify structure.
        assert_eq!(graph.modules(&db).len(), 2);
        assert!(graph.get_module(&db, base).is_some());
        assert!(graph.get_module(&db, math).is_some());

        // Verify dependencies.
        let math_deps = graph.dependencies(&db).get(&math).unwrap();
        assert!(math_deps.contains(&base));
    }

    /// The same path is the same module id, however it is reached.
    ///
    /// This is why the id is interned rather than an input. As an input each
    /// `new` handed back a distinct id for the same path, so anything that
    /// built one independently - a synthetic rider path, a second graph
    /// builder - got an id that compared unequal to everyone else's.
    #[test]
    fn module_id_is_the_path() {
        let db = crate::Database::default();

        let a = ModuleId::new(&db, "sys/std/u32".to_string());
        let b = ModuleId::new(&db, "sys/std/u32".to_string());
        assert_eq!(a, b, "the same path has to give the same id");
        assert_eq!(a.path(&db), "sys/std/u32");

        let other = ModuleId::new(&db, "sys/std/i32".to_string());
        assert_ne!(a, other, "different paths stay different ids");
    }

    /// An id built far from the graph still matches the one in it.
    #[test]
    fn module_id_matches_across_builders() {
        let db = crate::Database::default();

        let mut builder = ModuleGraphBuilder::new(&db);
        let source = Source::new(&db, "let x = 1".to_string());
        let in_graph = builder.add_module("local/test/a", source);
        let graph = builder.build();

        // Rebuild the id from the path alone, as a caller holding only a path
        // would have to.
        let from_path = ModuleId::new(&db, "local/test/a".to_string());
        assert_eq!(in_graph, from_path);
        assert!(
            graph.get_module(&db, from_path).is_some(),
            "an id built from the path alone should find its module",
        );
    }

    /// Ids keep working as map keys.
    #[test]
    fn module_id_is_usable_as_a_map_key() {
        use rmx::std::collections::BTreeMap;

        let db = crate::Database::default();
        let mut map = BTreeMap::new();
        map.insert(ModuleId::new(&db, "b".to_string()), 2);
        map.insert(ModuleId::new(&db, "a".to_string()), 1);

        // Re-derived keys find their entries.
        assert_eq!(map.get(&ModuleId::new(&db, "a".to_string())), Some(&1));
        assert_eq!(map.get(&ModuleId::new(&db, "b".to_string())), Some(&2));
        assert_eq!(map.len(), 2, "the same path must not insert twice");
    }
}
