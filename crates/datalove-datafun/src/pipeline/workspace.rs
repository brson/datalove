//! Workspace descriptor: immutable snapshot of all compilation inputs.
//!
//! A [`WorkspaceDescriptor`] declares the complete set of package libraries,
//! modules, riders, and compiler options for a compilation session. It can be
//! diffed against a previous snapshot to produce a [`WorkspaceDelta`] for
//! incremental recompilation.
//!
//! The descriptor is pure data -- it does not perform compilation. Drivers
//! (CLI, REPL, LSP) build descriptors and feed them to
//! [`ModuleCompilationPipeline`](super::ModuleCompilationPipeline).

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use datalove_datafun_pkg::package_load::{PackageWorld, Package};
use datalove_datafun_pkg::package_load_worldfile::WorldfileSection;

/// Immutable snapshot of all compilation inputs.
///
/// Cheaply cloneable (inner data is Arc'd).
/// Two descriptors can be diffed to produce a [`WorkspaceDelta`].
#[derive(Clone, Debug)]
pub struct WorkspaceDescriptor {
    /// System package library (e.g. sys/std, sys/collections).
    /// None when --no-sys.
    pub system_library: Option<PackageLibrary>,

    /// User package libraries (e.g. local/).
    /// Ordered; earlier libraries shadow later ones on name collision.
    pub user_libraries: Vec<PackageLibrary>,

    /// Compiler options that affect all compilation.
    pub options: CompilerOptions,
}

/// A named collection of packages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageLibrary {
    /// Library name (e.g. "sys", "local").
    pub name: String,

    /// Packages in this library, keyed by package name.
    pub packages: BTreeMap<String, PackageDescriptor>,
}

/// A package containing modules and an optional native rider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageDescriptor {
    /// Package name (e.g. "std", "myapp").
    pub name: String,

    /// Modules in this package, keyed by module name.
    pub modules: BTreeMap<String, ModuleDescriptor>,

    /// Native rider for this package, if any.
    pub rider: Option<RiderDescriptor>,
}

/// A single module's source and origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleDescriptor {
    /// Module name (e.g. "list", "main").
    pub name: String,

    /// Source text of the module.
    pub source: Arc<str>,

    /// Filesystem path, if loaded from disk.
    /// Used for diagnostics and file-watching, not compilation.
    pub origin: Option<PathBuf>,
}

/// Native rider interface and crate location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RiderDescriptor {
    /// Interface source text (.dli content).
    pub interface_source: Arc<str>,

    /// Path to the rider Cargo crate directory.
    /// None for synthetic or inline riders.
    pub crate_dir: Option<PathBuf>,
}

/// Compiler options that affect all compilation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompilerOptions {
    pub const_inlining: bool,
    pub skip_specialization: bool,
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self {
            const_inlining: true,
            skip_specialization: false,
        }
    }
}

/// Fully qualified module path: library/package/module.
pub type ModulePath = String;

/// Changes between two workspace snapshots.
#[derive(Clone, Debug, Default)]
pub struct WorkspaceDelta {
    pub modules_added: Vec<(ModulePath, ModuleDescriptor)>,
    pub modules_removed: Vec<ModulePath>,
    pub modules_changed: Vec<(ModulePath, ModuleDescriptor)>,
    pub riders_added: Vec<(String, RiderDescriptor)>,
    pub riders_removed: Vec<String>,
    pub riders_changed: Vec<(String, RiderDescriptor)>,
    pub options_changed: Option<CompilerOptions>,
}

impl WorkspaceDelta {
    /// True if no changes exist.
    pub fn is_empty(&self) -> bool {
        self.modules_added.is_empty()
            && self.modules_removed.is_empty()
            && self.modules_changed.is_empty()
            && self.riders_added.is_empty()
            && self.riders_removed.is_empty()
            && self.riders_changed.is_empty()
            && self.options_changed.is_none()
    }

    /// True if any rider changed, added, or removed.
    pub fn riders_dirty(&self) -> bool {
        !self.riders_added.is_empty()
            || !self.riders_removed.is_empty()
            || !self.riders_changed.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

impl WorkspaceDescriptor {
    /// Empty workspace with default options.
    pub fn empty() -> Self {
        Self {
            system_library: None,
            user_libraries: Vec::new(),
            options: CompilerOptions::default(),
        }
    }

    /// Build from a loaded [`PackageWorld`].
    ///
    /// The `system` library populates `system_library`; the `local`
    /// library is added as the first user library.
    pub fn from_package_world(world: &PackageWorld, options: CompilerOptions) -> Self {
        let system_library = if world.pkglib_system.is_empty() {
            None
        } else {
            Some(package_library_from_map("sys", &world.pkglib_system))
        };

        let user_libraries = if world.pkglib_local.is_empty() {
            Vec::new()
        } else {
            vec![package_library_from_map("local", &world.pkglib_local)]
        };

        Self { system_library, user_libraries, options }
    }

    /// Build from parsed worldfile sections.
    pub fn from_worldfile_sections(sections: &[WorldfileSection], options: CompilerOptions) -> Self {
        let mut libraries: BTreeMap<String, BTreeMap<String, PackageDescriptor>> = BTreeMap::new();
        let mut riders: Vec<(String, RiderDescriptor)> = Vec::new();

        for section in sections {
            match section {
                WorldfileSection::Module { library, package, module, source } => {
                    let lib = libraries.entry(library.clone()).or_default();
                    let pkg = lib.entry(package.clone()).or_insert_with(|| PackageDescriptor {
                        name: package.clone(),
                        modules: BTreeMap::new(),
                        rider: None,
                    });
                    pkg.modules.insert(module.clone(), ModuleDescriptor {
                        name: module.clone(),
                        source: Arc::from(source.as_str()),
                        origin: None,
                    });
                }
                WorldfileSection::Rider { name, source } => {
                    riders.push((name.clone(), RiderDescriptor {
                        interface_source: Arc::from(source.as_str()),
                        crate_dir: None,
                    }));
                }
                _ => {}
            }
        }

        // Separate system vs user libraries.
        let system_library = libraries.remove("sys").map(|pkgs| PackageLibrary {
            name: "sys".into(),
            packages: pkgs,
        });

        let user_libraries: Vec<PackageLibrary> = libraries.into_iter()
            .map(|(name, packages)| PackageLibrary { name, packages })
            .collect();

        // Attach worldfile riders to matching packages.
        // Worldfile riders are identified by package name.
        let mut descriptor = Self { system_library, user_libraries, options };
        for (rider_name, rider) in riders {
            descriptor.attach_rider(&rider_name, rider);
        }
        descriptor
    }

    /// Attach a rider to the first package matching the rider name.
    fn attach_rider(&mut self, name: &str, rider: RiderDescriptor) {
        let libraries = self.system_library.iter_mut()
            .chain(self.user_libraries.iter_mut());
        for lib in libraries {
            if let Some(pkg) = lib.packages.get_mut(name) {
                pkg.rider = Some(rider);
                return;
            }
        }
        // Rider without a matching package -- create a stub package in the
        // first user library (or create one).
        if self.user_libraries.is_empty() {
            self.user_libraries.push(PackageLibrary {
                name: "local".into(),
                packages: BTreeMap::new(),
            });
        }
        let lib = &mut self.user_libraries[0];
        lib.packages.entry(name.into()).or_insert_with(|| PackageDescriptor {
            name: name.into(),
            modules: BTreeMap::new(),
            rider: Some(rider),
        });
    }
}

// ---------------------------------------------------------------------------
// Iteration helpers
// ---------------------------------------------------------------------------

impl WorkspaceDescriptor {
    /// Iterate all libraries (system first, then user).
    pub fn libraries(&self) -> impl Iterator<Item = &PackageLibrary> {
        self.system_library.iter().chain(self.user_libraries.iter())
    }

    /// Iterate all (path, module) pairs across all libraries.
    pub fn all_modules(&self) -> impl Iterator<Item = (ModulePath, &ModuleDescriptor)> {
        self.libraries().flat_map(|lib| {
            lib.packages.values().flat_map(move |pkg| {
                pkg.modules.values().map(move |m| {
                    let path = format!("{}/{}/{}", lib.name, pkg.name, m.name);
                    (path, m)
                })
            })
        })
    }

    /// Collect all rider sources as (name, source) pairs.
    pub fn rider_sources(&self) -> Vec<(String, String)> {
        let mut sources = Vec::new();
        for lib in self.libraries() {
            for pkg in lib.packages.values() {
                if let Some(ref rider) = pkg.rider {
                    sources.push((pkg.name.clone(), rider.interface_source.to_string()));
                }
            }
        }
        sources
    }

    /// Collect all rider crate directories as (name, path) pairs.
    pub fn rider_crate_dirs(&self) -> Vec<(String, PathBuf)> {
        let mut dirs = Vec::new();
        for lib in self.libraries() {
            for pkg in lib.packages.values() {
                if let Some(ref rider) = pkg.rider {
                    if let Some(ref dir) = rider.crate_dir {
                        dirs.push((pkg.name.clone(), dir.clone()));
                    }
                }
            }
        }
        dirs
    }
}

// ---------------------------------------------------------------------------
// Diffing
// ---------------------------------------------------------------------------

impl WorkspaceDescriptor {
    /// Diff against a newer descriptor, producing a delta.
    pub fn diff(&self, newer: &WorkspaceDescriptor) -> WorkspaceDelta {
        let old_modules: BTreeMap<ModulePath, &ModuleDescriptor> =
            self.all_modules().collect();
        let new_modules: BTreeMap<ModulePath, &ModuleDescriptor> =
            newer.all_modules().collect();

        let mut delta = WorkspaceDelta::default();

        // Added and changed modules.
        for (path, new_mod) in &new_modules {
            match old_modules.get(path) {
                None => delta.modules_added.push((path.clone(), (*new_mod).clone())),
                Some(old_mod) => {
                    if old_mod.source != new_mod.source {
                        delta.modules_changed.push((path.clone(), (*new_mod).clone()));
                    }
                }
            }
        }

        // Removed modules.
        for path in old_modules.keys() {
            if !new_modules.contains_key(path) {
                delta.modules_removed.push(path.clone());
            }
        }

        // Rider diffing.
        let old_riders: BTreeMap<String, &RiderDescriptor> = self.all_riders().collect();
        let new_riders: BTreeMap<String, &RiderDescriptor> = newer.all_riders().collect();

        for (name, new_rider) in &new_riders {
            match old_riders.get(name) {
                None => delta.riders_added.push((name.clone(), (*new_rider).clone())),
                Some(old_rider) => {
                    if *old_rider != *new_rider {
                        delta.riders_changed.push((name.clone(), (*new_rider).clone()));
                    }
                }
            }
        }
        for name in old_riders.keys() {
            if !new_riders.contains_key(name) {
                delta.riders_removed.push(name.clone());
            }
        }

        // Options.
        if self.options != newer.options {
            delta.options_changed = Some(newer.options.clone());
        }

        delta
    }

    /// Iterate all (package_name, rider) pairs.
    fn all_riders(&self) -> impl Iterator<Item = (String, &RiderDescriptor)> {
        self.libraries().flat_map(|lib| {
            lib.packages.values().filter_map(|pkg| {
                pkg.rider.as_ref().map(|r| (pkg.name.clone(), r))
            })
        })
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn package_library_from_map(
    lib_name: &str,
    packages: &BTreeMap<String, Package>,
) -> PackageLibrary {
    let mut pkg_map = BTreeMap::new();
    for (pkg_name, pkg) in packages {
        let modules = pkg.modules.iter().map(|(mod_name, pkg_mod)| {
            (mod_name.clone(), ModuleDescriptor {
                name: mod_name.clone(),
                source: Arc::from(pkg_mod.text.as_str()),
                origin: Some(pkg_mod.path.clone()),
            })
        }).collect();

        let rider = match (&pkg.rider_source, &pkg.rider_crate_dir) {
            (Some(source), crate_dir) => Some(RiderDescriptor {
                interface_source: Arc::from(source.as_str()),
                crate_dir: crate_dir.clone(),
            }),
            (None, Some(crate_dir)) => Some(RiderDescriptor {
                interface_source: Arc::from(""),
                crate_dir: Some(crate_dir.clone()),
            }),
            (None, None) => None,
        };

        pkg_map.insert(pkg_name.clone(), PackageDescriptor {
            name: pkg_name.clone(),
            modules,
            rider,
        });
    }

    PackageLibrary {
        name: lib_name.into(),
        packages: pkg_map,
    }
}

// ---------------------------------------------------------------------------
// Pipeline integration
// ---------------------------------------------------------------------------

use super::module_pipeline::{ModuleCompilationPipeline, ConstInlining};

impl WorkspaceDescriptor {
    /// Apply this descriptor to a fresh pipeline, populating all modules and riders.
    pub fn apply_to_pipeline(
        &self,
        pipeline: &mut ModuleCompilationPipeline,
        db: &dyn salsa::Database,
    ) {
        for (path, module) in self.all_modules() {
            let parts: Vec<&str> = path.splitn(3, '/').collect();
            if parts.len() == 3 {
                pipeline.add_module(db, parts[0], parts[1], parts[2], &module.source);
            }
        }

        pipeline.set_rider_sources(self.rider_sources());
        pipeline.set_rider_crate_dirs(self.rider_crate_dirs());
    }

    /// Create a fresh pipeline from this descriptor.
    pub fn to_pipeline(&self, db: &dyn salsa::Database) -> ModuleCompilationPipeline {
        let const_inlining = if self.options.const_inlining {
            ConstInlining::Enabled
        } else {
            ConstInlining::Disabled
        };
        let mut pipeline = ModuleCompilationPipeline::new(const_inlining);
        pipeline.set_skip_specialization(self.options.skip_specialization);
        self.apply_to_pipeline(&mut pipeline, db);
        pipeline
    }
}

impl WorkspaceDelta {
    /// Apply this delta to an existing pipeline for incremental recompilation.
    pub fn apply_to_pipeline(
        &self,
        pipeline: &mut ModuleCompilationPipeline,
        db: &mut dyn salsa::Database,
    ) {
        for path in &self.modules_removed {
            let parts: Vec<&str> = path.splitn(3, '/').collect();
            if parts.len() == 3 {
                pipeline.remove_module(parts[0], parts[1], parts[2]);
            }
        }

        for (path, module) in &self.modules_added {
            let parts: Vec<&str> = path.splitn(3, '/').collect();
            if parts.len() == 3 {
                pipeline.add_module(db, parts[0], parts[1], parts[2], &module.source);
            }
        }

        for (path, module) in &self.modules_changed {
            let parts: Vec<&str> = path.splitn(3, '/').collect();
            if parts.len() == 3 {
                pipeline.update_source(db, parts[0], parts[1], parts[2], &module.source);
            }
        }
    }
}
