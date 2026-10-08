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
use std::path::{Path, PathBuf};
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

    /// Directory the compiler may write to, equivalent to cargo's target dir.
    ///
    /// The synthesized native component crate is built here. Two workspaces
    /// compiled concurrently must not share one, or their components overwrite
    /// each other. `None` for workspaces with no on-disk working area, such as
    /// those built from worldfiles; building a native rider then fails.
    ///
    /// This is where output goes, not an input to compilation, so it takes no
    /// part in [`diff`](Self::diff).
    pub work_dir: Option<PathBuf>,
}

/// The system library a driver carries, ready to compile against.
///
/// Sources and native rider addresses travel together: a rider interface
/// declares functions that must be linked into the running binary for the
/// library to work. A driver that has no stdlib of its own, like a test
/// compiling from a directory, does not need this type.
pub struct SystemLibrary {
    /// The packages that make up the library.
    pub library: PackageLibrary,

    /// Address of every native rider function linked into this binary,
    /// by the linker symbol the compiler emits calls to.
    pub natives: Vec<(String, *const ())>,
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

    /// Data files in this package, keyed by name, which `require data` names.
    pub data: BTreeMap<String, ModuleDescriptor>,

    /// Native rider for this package, if any.
    pub rider: Option<RiderDescriptor>,
}

impl PackageDescriptor {
    /// A package of nothing yet, under a name.
    pub fn empty(name: &str) -> Self {
        Self {
            name: name.into(),
            modules: BTreeMap::new(),
            data: BTreeMap::new(),
            rider: None,
        }
    }
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

    /// What the package's manifest calls the crate, and which version.
    ///
    /// `(name, version)`. None for a rider with no manifest behind it: an
    /// inline one, or one from a worldfile, which carries interfaces without
    /// the packages they came from.
    pub crate_spec: Option<(String, String)>,

    /// Path to the rider Cargo crate directory.
    /// None for synthetic or inline riders, and for a packaged datalove
    /// package, whose rider comes from a registry by `crate_spec` instead.
    pub crate_dir: Option<PathBuf>,
}

/// A rider to compile into a native component.
///
/// What `rider_build` needs to name one as a cargo dependency: which crate,
/// which version, and where its source is when it is beside the package
/// rather than in a registry.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RiderCrate {
    /// The rider's name as `require rider` writes it, for messages.
    pub rider_name: String,

    /// The crate's name, as the package's manifest gives it.
    pub crate_name: String,

    /// The version the manifest asks for.
    ///
    /// Taken exactly rather than as a requirement: a rider and the runtime
    /// loading it have to agree on the layout of everything crossing between
    /// them, which a range does not promise.
    pub version: String,

    /// Where the crate's source is, when it is beside the package.
    ///
    /// None for a packaged datalove package, whose rider comes from a
    /// registry by name and version instead.
    pub dir: Option<PathBuf>,
}

/// Compiler options that affect all compilation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompilerOptions {
    pub const_inlining: bool,
    pub skip_specialization: bool,
    /// Keep a printed copy of every lowered function on the compiled modules.
    ///
    /// Off by default, because printing them costs about as much as the whole
    /// of phase 5a and only the fixtures and `worldfile_analysis` ever read
    /// them. See `CompiledModules::module_ir_dumps`.
    pub keep_ir_dumps: bool,
    /// Keep evaluated consts from one compile to the next, by module.
    ///
    /// On by default. Off is for measuring what it saves, and for checking a
    /// recompile against one that evaluates everything.
    pub cache_consts: bool,
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self {
            const_inlining: true,
            skip_specialization: false,
            keep_ir_dumps: false,
            cache_consts: true,
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
    pub data_added: Vec<(ModulePath, ModuleDescriptor)>,
    pub data_removed: Vec<ModulePath>,
    pub data_changed: Vec<(ModulePath, ModuleDescriptor)>,
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
            && self.data_added.is_empty()
            && self.data_removed.is_empty()
            && self.data_changed.is_empty()
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

    /// True when this delta cannot be applied to a pipeline that already exists.
    ///
    /// Compiler options are settled when a pipeline is built, because nothing
    /// already compiled under the old ones would be recompiled under the new.
    /// Riders are set as a whole list rather than one at a time, so a delta
    /// naming only the ones that moved cannot say what the list becomes.
    ///
    /// A driver seeing this builds a new pipeline from the newer descriptor.
    pub fn requires_new_pipeline(&self) -> bool {
        self.new_pipeline_reason().is_some()
    }

    /// What in this delta needs a new pipeline, for the message that says so.
    fn new_pipeline_reason(&self) -> Option<&'static str> {
        match (self.options_changed.is_some(), self.riders_dirty()) {
            (true, true) => Some("compiler options and riders"),
            (true, false) => Some("compiler options"),
            (false, true) => Some("riders"),
            (false, false) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

impl WorkspaceDescriptor {
    /// Empty workspace with default options and no working area.
    pub fn empty() -> Self {
        Self {
            system_library: None,
            user_libraries: Vec::new(),
            options: CompilerOptions::default(),
            work_dir: None,
        }
    }

    /// Set the directory the compiler may write to.
    pub fn with_work_dir(mut self, work_dir: impl Into<PathBuf>) -> Self {
        self.work_dir = Some(work_dir.into());
        self
    }

    /// Build a workspace around the system library the driver carries.
    pub fn from_system_library(sys: &SystemLibrary) -> Self {
        Self {
            system_library: Some(sys.library.clone()),
            user_libraries: Vec::new(),
            options: CompilerOptions::default(),
            work_dir: None,
        }
    }

    /// Load a system library from a directory of packages.
    ///
    /// This reads the library the compiler was built from, so it is for use
    /// inside the source tree; a distributed binary carries its own library
    /// and uses [`from_system_library`](Self::from_system_library).
    pub async fn load_sys_dir(sys_dir: PathBuf) -> rmx::anyhow::Result<Self> {
        use datalove_datafun_pkg::package_load;

        let config = package_load::PackageWorldConfig {
            dir_pkglib_system: sys_dir,
            dir_pkglib_local: None,
        };

        let world = package_load::load_world(config).await?;
        Ok(Self::from_package_world(&world, CompilerOptions::default()))
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

        Self { system_library, user_libraries, options, work_dir: None }
    }

    /// Build from parsed worldfile sections.
    pub fn from_worldfile_sections(sections: &[WorldfileSection], options: CompilerOptions) -> Self {
        let mut libraries: BTreeMap<String, BTreeMap<String, PackageDescriptor>> = BTreeMap::new();
        let mut riders: Vec<(String, RiderDescriptor)> = Vec::new();

        for section in sections {
            match section {
                WorldfileSection::Module { library, package, module, source } => {
                    let lib = libraries.entry(library.clone()).or_default();
                    let pkg = lib.entry(package.clone()).or_insert_with(|| PackageDescriptor::empty(package));
                    pkg.modules.insert(module.clone(), ModuleDescriptor {
                        name: module.clone(),
                        source: Arc::from(source.as_str()),
                        origin: None,
                    });
                }
                WorldfileSection::Data { library, package, name, source } => {
                    let lib = libraries.entry(library.clone()).or_default();
                    let pkg = lib.entry(package.clone()).or_insert_with(|| PackageDescriptor::empty(package));
                    pkg.data.insert(name.clone(), ModuleDescriptor {
                        name: name.clone(),
                        source: Arc::from(source.as_str()),
                        origin: None,
                    });
                }
                WorldfileSection::Rider { name, source } => {
                    riders.push((name.clone(), RiderDescriptor {
                        interface_source: Arc::from(source.as_str()),
                        crate_spec: None,
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
        let mut descriptor = Self { system_library, user_libraries, options, work_dir: None };
        for (rider_name, rider) in riders {
            descriptor.attach_rider(&rider_name, rider);
        }
        descriptor
    }

    /// Combine two descriptors, producing a new one.
    ///
    /// The system library comes from `self` if present, otherwise from `other`.
    /// User libraries from both are concatenated (`self` first, then `other`).
    /// Options come from `other` (the newer descriptor wins).
    pub fn merge(&self, other: &WorkspaceDescriptor) -> WorkspaceDescriptor {
        let system_library = self.system_library.clone()
            .or_else(|| other.system_library.clone());

        let mut user_libraries = self.user_libraries.clone();
        for other_lib in &other.user_libraries {
            // Merge into existing library with same name, or append.
            if let Some(existing) = user_libraries.iter_mut().find(|l| l.name == other_lib.name) {
                for (pkg_name, pkg) in &other_lib.packages {
                    existing.packages.insert(pkg_name.clone(), pkg.clone());
                }
            } else {
                user_libraries.push(other_lib.clone());
            }
        }

        WorkspaceDescriptor {
            system_library,
            user_libraries,
            options: other.options.clone(),
            work_dir: other.work_dir.clone().or_else(|| self.work_dir.clone()),
        }
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
            rider: Some(rider),
            ..PackageDescriptor::empty(name)
        });
    }
}

// ---------------------------------------------------------------------------
// Iteration helpers
// ---------------------------------------------------------------------------

/// The directory a command takes the `local` library from, if it has one.
///
/// A command given a script looks beside the script, and then, for a script
/// in a directory named `scripts`, beside that directory. One given no script
/// looks in `cwd`. A command given a script does not look in `cwd`, so what a
/// script means does not depend on where it is run from.
///
/// Nothing further up is looked at. With no manifest to say where a workspace
/// begins, a walk to the root would find directories that are not workspaces
/// -- `/usr` has a `local` -- so this looks only where a workspace laid out
/// as documented puts its library.
///
/// This is the command line's rule. The pipeline itself takes whatever library
/// it is given.
pub fn find_local_library(script: Option<&Path>, cwd: &Path) -> Option<PathBuf> {
    let Some(script) = script else {
        let here = cwd.join("local");
        return here.is_dir().then_some(here);
    };
    let script = cwd.join(script);
    let dir = script.parent()?;
    let beside = dir.join("local");
    if beside.is_dir() {
        return Some(beside);
    }
    if dir.file_name()? != "scripts" {
        return None;
    }
    let above = dir.parent()?.join("local");
    above.is_dir().then_some(above)
}

/// Load the `local` library from `dir`.
pub fn load_local_library(dir: &Path) -> AnyResult<PackageLibrary> {
    let packages = datalove_datafun_pkg::package_load::load_library_dir(&dir.to_path_buf())
        .with_context(|| fmt!("failed to load the local library at {}", dir.display()))?;
    Ok(package_library_from_map("local", &packages))
}

impl WorkspaceDescriptor {
    /// Add a user library, as [`load_local_library`] gives.
    ///
    /// A rider is named after its package, in `require rider` and in the
    /// symbols of its natives alike, and riders are not told apart by
    /// library. So a package with a rider may not share its name with a
    /// package already here that has one: `local/std` with a rider would be
    /// a second `std` rider beside the system library's.
    pub fn with_user_library(self, library: PackageLibrary) -> AnyResult<Self> {
        for (name, package) in &library.packages {
            if package.rider.is_none() {
                continue;
            }
            let clash = self.libraries()
                .find(|lib| lib.packages.get(name).is_some_and(|p| p.rider.is_some()));
            if let Some(clash) = clash {
                bail!(
                    "package `{}/{name}` has a rider, and so does `{}/{name}`: \
                     riders are named after their packages, so two cannot share a name",
                    library.name, clash.name,
                );
            }
        }
        let other = WorkspaceDescriptor {
            system_library: None,
            user_libraries: vec![library],
            options: self.options.clone(),
            work_dir: None,
        };
        Ok(self.merge(&other))
    }

    /// The file each module is read from, by module path, for diagnostics.
    ///
    /// A module with no file on disk, like one of the system library compiled
    /// into this binary, is named by where it sits in its library, as
    /// `sys/std/string.dfm`.
    pub fn module_files(&self) -> BTreeMap<ModulePath, PathBuf> {
        self.all_modules()
            .map(|(path, module)| {
                let file = module.origin.clone()
                    .unwrap_or_else(|| PathBuf::from(fmt!("{path}.dfm")));
                (path, file)
            })
            .collect()
    }

    /// The file each data file is read from, by path, for diagnostics.
    ///
    /// Named the way [`module_files`](Self::module_files) names one with no
    /// file on disk.
    pub fn data_files(&self) -> BTreeMap<ModulePath, PathBuf> {
        self.all_data()
            .map(|(path, file)| {
                let origin = file.origin.clone()
                    .unwrap_or_else(|| PathBuf::from(fmt!("{path}.dlt")));
                (path, origin)
            })
            .collect()
    }

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

    /// Iterate all (path, data file) pairs across all libraries.
    pub fn all_data(&self) -> impl Iterator<Item = (ModulePath, &ModuleDescriptor)> {
        self.libraries().flat_map(|lib| {
            lib.packages.values().flat_map(move |pkg| {
                pkg.data.values().map(move |d| {
                    (format!("{}/{}/{}", lib.name, pkg.name, d.name), d)
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
    pub fn rider_crates(&self) -> Vec<RiderCrate> {
        rider_crates_of(self.libraries())
    }

    /// The rider crates of the user libraries, which no binary carries.
    pub fn user_rider_crates(&self) -> Vec<RiderCrate> {
        rider_crates_of(self.user_libraries.iter())
    }
}

/// The rider crates of the packages in `libraries`.
fn rider_crates_of<'a>(libraries: impl Iterator<Item = &'a PackageLibrary>) -> Vec<RiderCrate> {
    let mut riders = Vec::new();
    for lib in libraries {
        for pkg in lib.packages.values() {
            let Some(rider) = &pkg.rider else { continue };
            // A rider nothing declared is a rider nothing can call. The
            // manifest is required wherever an interface is, so every
            // rider a module reaches has a name and a version here.
            let Some((crate_name, version)) = &rider.crate_spec else { continue };
            riders.push(RiderCrate {
                rider_name: pkg.name.clone(),
                crate_name: crate_name.clone(),
                version: version.clone(),
                dir: rider.crate_dir.clone(),
            });
        }
    }
    riders
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

        // Data, the same way.
        let old_data: BTreeMap<ModulePath, &ModuleDescriptor> = self.all_data().collect();
        let new_data: BTreeMap<ModulePath, &ModuleDescriptor> = newer.all_data().collect();
        for (path, new_file) in &new_data {
            match old_data.get(path) {
                None => delta.data_added.push((path.clone(), (*new_file).clone())),
                Some(old_file) if old_file.source != new_file.source => {
                    delta.data_changed.push((path.clone(), (*new_file).clone()));
                }
                Some(_) => {}
            }
        }
        for path in old_data.keys() {
            if !new_data.contains_key(path) {
                delta.data_removed.push(path.clone());
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

        let crate_spec = pkg.rider_crate.as_ref()
            .map(|rider| (rider.name.clone(), rider.version.clone()));

        let rider = match (&pkg.rider_source, &pkg.rider_crate_dir) {
            (Some(source), crate_dir) => Some(RiderDescriptor {
                interface_source: Arc::from(source.as_str()),
                crate_spec,
                crate_dir: crate_dir.clone(),
            }),
            (None, Some(crate_dir)) => Some(RiderDescriptor {
                interface_source: Arc::from(""),
                crate_spec,
                crate_dir: Some(crate_dir.clone()),
            }),
            (None, None) => None,
        };

        let data = pkg.data.iter().map(|(name, file)| {
            (name.clone(), ModuleDescriptor {
                name: name.clone(),
                source: Arc::from(file.text.as_str()),
                origin: Some(file.path.clone()),
            })
        }).collect();

        pkg_map.insert(pkg_name.clone(), PackageDescriptor {
            name: pkg_name.clone(),
            modules,
            data,
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

use super::module_pipeline::ModuleCompilationPipeline;

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
        for (path, file) in self.all_data() {
            let parts: Vec<&str> = path.splitn(3, '/').collect();
            pipeline.add_data(db, parts[0], parts[1], parts[2], &file.source);
        }

        pipeline.set_rider_sources(self.rider_sources());
    }

    /// Create a fresh pipeline from this descriptor.
    pub fn to_pipeline(&self, db: &dyn salsa::Database) -> ModuleCompilationPipeline {
        let mut pipeline = ModuleCompilationPipeline::new(self.options.clone());
        self.apply_to_pipeline(&mut pipeline, db);
        pipeline
    }
}

impl WorkspaceDelta {
    /// Apply this delta to an existing pipeline for incremental recompilation.
    ///
    /// Modules are the only part of a workspace a live pipeline can take one
    /// change at a time. See [`requires_new_pipeline`](Self::requires_new_pipeline)
    /// for the rest, which this refuses rather than drops.
    pub fn apply_to_pipeline(
        &self,
        pipeline: &mut ModuleCompilationPipeline,
        db: &mut dyn salsa::Database,
    ) {
        assert!(
            !self.requires_new_pipeline(),
            "this delta changes {}, which a built pipeline cannot take; \
             build a new one from the newer descriptor instead",
            self.new_pipeline_reason().expect("requires_new_pipeline said so"),
        );

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

        for path in &self.data_removed {
            let parts: Vec<&str> = path.splitn(3, '/').collect();
            pipeline.remove_data(parts[0], parts[1], parts[2]);
        }
        for (path, file) in &self.data_added {
            let parts: Vec<&str> = path.splitn(3, '/').collect();
            pipeline.add_data(db, parts[0], parts[1], parts[2], &file.source);
        }
        for (path, file) in &self.data_changed {
            let parts: Vec<&str> = path.splitn(3, '/').collect();
            pipeline.update_data(db, parts[0], parts[1], parts[2], &file.source);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A workspace of one module, optionally with a rider on its package.
    fn descriptor(
        source: &str,
        rider: Option<&str>,
        options: CompilerOptions,
    ) -> WorkspaceDescriptor {
        let package = PackageDescriptor {
            name: "pkg".to_string(),
            modules: BTreeMap::from([(
                "main".to_string(),
                ModuleDescriptor {
                    name: "main".to_string(),
                    source: Arc::from(source),
                    origin: None,
                },
            )]),
            data: BTreeMap::new(),
            rider: rider.map(|interface| RiderDescriptor {
                interface_source: Arc::from(interface),
                crate_spec: None,
                crate_dir: None,
            }),
        };
        WorkspaceDescriptor {
            system_library: None,
            user_libraries: vec![PackageLibrary {
                name: "local".to_string(),
                packages: BTreeMap::from([("pkg".to_string(), package)]),
            }],
            options,
            work_dir: None,
        }
    }

    #[test]
    fn an_edited_module_applies_to_the_pipeline_it_was_built_for() {
        let before = descriptor("let x = 1", None, CompilerOptions::default());
        let after = descriptor("let x = 2", None, CompilerOptions::default());
        let delta = before.diff(&after);

        assert_eq!(delta.modules_changed.len(), 1);
        assert!(!delta.requires_new_pipeline());
    }

    #[test]
    fn changed_options_need_a_new_pipeline() {
        let before = descriptor("let x = 1", None, CompilerOptions::default());
        let after = descriptor(
            "let x = 1",
            None,
            CompilerOptions { keep_ir_dumps: true, ..CompilerOptions::default() },
        );
        let delta = before.diff(&after);

        assert!(!delta.is_empty(), "the change is reported");
        assert!(delta.requires_new_pipeline());
    }

    #[test]
    fn changed_riders_need_a_new_pipeline() {
        let before = descriptor("let x = 1", Some("native fun f()"), CompilerOptions::default());
        let after = descriptor("let x = 1", Some("native fun g()"), CompilerOptions::default());
        let delta = before.diff(&after);

        assert!(delta.riders_dirty());
        assert!(delta.requires_new_pipeline());
    }

    /// The case that used to pass silently, compiling under the old options.
    #[test]
    #[should_panic(expected = "compiler options")]
    fn applying_changed_options_to_a_built_pipeline_is_refused() {
        let before = descriptor("let x = 1", None, CompilerOptions::default());
        let after = descriptor(
            "let x = 1",
            None,
            CompilerOptions { skip_specialization: true, ..CompilerOptions::default() },
        );
        let delta = before.diff(&after);

        let mut db = crate::Database::default();
        let mut pipeline = before.to_pipeline(&db);
        delta.apply_to_pipeline(&mut pipeline, &mut db);
    }
}

#[cfg(test)]
mod local_library_tests {
    use super::*;
    use std::fs;

    /// A directory to lay a workspace out in, with `dirs` made inside it.
    fn layout(dirs: &[&str]) -> rmx::tempfile::TempDir {
        let root = rmx::tempfile::tempdir().unwrap();
        for dir in dirs {
            fs::create_dir_all(root.path().join(dir)).unwrap();
        }
        root
    }

    #[test]
    fn local_beside_the_script() {
        let ws = layout(&["local"]);
        let found = find_local_library(Some(&ws.path().join("s.dfs")), Path::new("/"));
        assert_eq!(found, Some(ws.path().join("local")));
    }

    #[test]
    fn local_beside_a_scripts_directory() {
        let ws = layout(&["local", "scripts"]);
        let script = ws.path().join("scripts").join("s.dfs");
        assert_eq!(find_local_library(Some(&script), Path::new("/")), Some(ws.path().join("local")));
    }

    #[test]
    fn local_beside_the_script_comes_first() {
        let ws = layout(&["local", "scripts/local"]);
        let script = ws.path().join("scripts").join("s.dfs");
        let found = find_local_library(Some(&script), Path::new("/"));
        assert_eq!(found, Some(ws.path().join("scripts").join("local")));
    }

    #[test]
    fn only_a_directory_named_scripts_is_looked_above() {
        let ws = layout(&["local", "tools"]);
        let script = ws.path().join("tools").join("s.dfs");
        assert_eq!(find_local_library(Some(&script), Path::new("/")), None);
    }

    #[test]
    fn nothing_further_up() {
        let ws = layout(&["local", "a/scripts"]);
        let script = ws.path().join("a").join("scripts").join("s.dfs");
        assert_eq!(find_local_library(Some(&script), Path::new("/")), None);
    }

    #[test]
    fn a_script_is_found_relative_to_cwd() {
        let ws = layout(&["local"]);
        let found = find_local_library(Some(Path::new("s.dfs")), ws.path());
        assert_eq!(found, Some(ws.path().join("local")));
    }

    #[test]
    fn a_script_does_not_look_in_cwd() {
        let ws = layout(&["local", "elsewhere"]);
        let script = ws.path().join("elsewhere").join("s.dfs");
        assert_eq!(find_local_library(Some(&script), ws.path()), None);
    }

    #[test]
    fn without_a_script_cwd() {
        let ws = layout(&["local"]);
        assert_eq!(find_local_library(None, ws.path()), Some(ws.path().join("local")));
        let empty = layout(&[]);
        assert_eq!(find_local_library(None, empty.path()), None);
    }

    #[test]
    fn a_file_named_local_is_not_a_library() {
        let ws = layout(&[]);
        fs::write(ws.path().join("local"), "").unwrap();
        assert_eq!(find_local_library(Some(&ws.path().join("s.dfs")), Path::new("/")), None);
    }

    #[test]
    fn local_library_loads_its_packages() {
        let ws = layout(&["local/app"]);
        let module = ws.path().join("local/app/greet.dfm");
        fs::write(&module, "fun answer(): int\n  ret 42\nend fun\n").unwrap();

        let library = load_local_library(&ws.path().join("local")).unwrap();
        assert_eq!(library.name, "local");
        let greet = &library.packages["app"].modules["greet"];
        assert_eq!(greet.origin.as_deref(), Some(module.as_path()));
    }

    #[test]
    fn local_library_takes_a_rider() {
        let ws = layout(&["local/app/rider"]);
        fs::write(ws.path().join("local/app/m.dfm"), "").unwrap();
        fs::write(ws.path().join("local/app/rider/rider.dli"), "native fun f(): int\n").unwrap();
        fs::write(
            ws.path().join("local/app/manifest.toml"),
            "[rider]\nname = \"app-rider\"\nversion = \"0.1.0\"\n",
        ).unwrap();

        let library = load_local_library(&ws.path().join("local")).unwrap();
        assert!(library.packages["app"].rider.is_some());
        let workspace = WorkspaceDescriptor::empty().with_user_library(library).unwrap();
        assert_eq!(workspace.user_rider_crates().len(), 1);
    }

    #[test]
    fn a_rider_may_not_share_a_name_with_another() {
        let with_rider = |library: &str| PackageLibrary {
            name: library.S(),
            packages: BTreeMap::from([("std".S(), PackageDescriptor {
                name: "std".S(),
                modules: BTreeMap::new(),
                data: BTreeMap::new(),
                rider: Some(RiderDescriptor {
                    interface_source: Arc::from(""),
                    crate_spec: Some(("r".S(), "0.1.0".S())),
                    crate_dir: None,
                }),
            })]),
        };
        let workspace = WorkspaceDescriptor::empty().with_user_library(with_rider("sys")).unwrap();
        let error = workspace.with_user_library(with_rider("local")).unwrap_err();
        assert!(fmt!("{error}").contains("two cannot share a name"), "{error}");
    }
}
