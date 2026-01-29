//! Worldfile parsing and loading.
//!
//! A worldfile is a text format for defining multiple modules and script units
//! in a single file. This format is primarily used for testing the compiler
//! and language runtime.
//!
//! # Format
//!
//! A worldfile consists of sections separated by `----------` lines. Each section
//! has a header line specifying its type, followed by another separator, then the
//! section content.
//!
//! ```text
//! ----------
//! <section-header>
//! ----------
//! <content>
//! ```
//!
//! # Section Types
//!
//! ## Module Sections
//!
//! Define a module with source code:
//!
//! | Header | Description |
//! |--------|-------------|
//! | `module lib/pkg/mod` | Initial module definition |
//! | `module-add lib/pkg/mod` | Add module (memoization tests) |
//! | `module-remove lib/pkg/mod` | Remove module (memoization tests) |
//! | `module-change-ws lib/pkg/mod` | Whitespace-only change |
//! | `module-change-ast lib/pkg/mod` | AST change, same types |
//! | `module-change-ty lib/pkg/mod` | Type-level change |
//!
//! ## Script Sections
//!
//! | Header | Description |
//! |--------|-------------|
//! | `scriptunit-fragment` | Script with statements |
//! | `scriptunit-expr` | Single expression |
//! | `script` | Legacy alias for `scriptunit-fragment` |
//!
//! # Example
//!
//! ```text
//! ----------
//! module sys/std/u32
//! ----------
//!
//! fun add(x: u32, y: u32): u32
//!     ret x + y
//! end fun
//!
//! ----------
//! scriptunit-fragment
//! ----------
//!
//! require module sys/std/u32
//! import u32.add
//! debuglog add(1, 2)
//! ```

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;
use rmx::std::io::Read;

use crate::package_load::{PackageWorld, Package, PackageModule};

// ============================================================================
// Module Path
// ============================================================================

/// A fully-qualified module path: `library/package/module`.
///
/// Libraries are either `sys` (standard library) or `local` (user code).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModulePath {
    pub library: String,
    pub package: String,
    pub module: String,
}

impl ModulePath {
    /// Create a new module path.
    pub fn new(library: impl Into<String>, package: impl Into<String>, module: impl Into<String>) -> Self {
        Self {
            library: library.into(),
            package: package.into(),
            module: module.into(),
        }
    }

    /// Parse a module path from `library/package/module` format.
    pub fn parse(path: &str) -> Option<Self> {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() == 3 {
            Some(Self::new(parts[0], parts[1], parts[2]))
        } else {
            None
        }
    }

    /// Format as `library/package/module`.
    pub fn to_path_string(&self) -> String {
        format!("{}/{}/{}", self.library, self.package, self.module)
    }
}

impl std::fmt::Display for ModulePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}/{}", self.library, self.package, self.module)
    }
}

// ============================================================================
// Section Types
// ============================================================================

/// The type of action a module section represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleSectionKind {
    /// Initial module definition.
    Module,
    /// Add a new module (for memoization tests).
    Add,
    /// Remove an existing module (for memoization tests).
    Remove,
    /// Change with whitespace-only changes (for memoization tests).
    ChangeWs,
    /// Change with AST changes but same types (for memoization tests).
    ChangeAst,
    /// Change with type-level changes (for memoization tests).
    ChangeTy,
}

impl ModuleSectionKind {
    /// Returns the header prefix for this section kind.
    pub fn header_prefix(&self) -> &'static str {
        match self {
            ModuleSectionKind::Module => "module",
            ModuleSectionKind::Add => "module-add",
            ModuleSectionKind::Remove => "module-remove",
            ModuleSectionKind::ChangeWs => "module-change-ws",
            ModuleSectionKind::ChangeAst => "module-change-ast",
            ModuleSectionKind::ChangeTy => "module-change-ty",
        }
    }

    /// Whether this section kind includes source content.
    pub fn has_source(&self) -> bool {
        !matches!(self, ModuleSectionKind::Remove)
    }
}

/// A section in a worldfile.
#[derive(Debug, Clone)]
pub enum WorldfileSection {
    /// Initial module definition.
    Module {
        library: String,
        package: String,
        module: String,
        source: String,
    },
    /// Add a new module (for memoization tests).
    ModuleAdd {
        library: String,
        package: String,
        module: String,
        source: String,
    },
    /// Remove an existing module (for memoization tests).
    ModuleRemove {
        library: String,
        package: String,
        module: String,
    },
    /// Change module with whitespace-only changes (for memoization tests).
    ModuleChangeWs {
        library: String,
        package: String,
        module: String,
        source: String,
    },
    /// Change module with AST changes but same types (for memoization tests).
    ModuleChangeAst {
        library: String,
        package: String,
        module: String,
        source: String,
    },
    /// Change module with type-level changes (for memoization tests).
    ModuleChangeTy {
        library: String,
        package: String,
        module: String,
        source: String,
    },
    /// Script unit containing statements (let/var/fun declarations, control flow).
    ScriptFragment {
        source: String,
    },
    /// Script unit containing a single expression.
    ScriptExpr {
        source: String,
    },
    /// Inline directives for function inlining tests.
    InlineDirectives {
        source: String,
    },
}

impl WorldfileSection {
    /// Create a module section from a path and source.
    pub fn module(path: ModulePath, source: String) -> Self {
        Self::Module {
            library: path.library,
            package: path.package,
            module: path.module,
            source,
        }
    }

    /// Get the module path if this is a module-related section.
    pub fn module_path(&self) -> Option<ModulePath> {
        match self {
            WorldfileSection::Module { library, package, module, .. }
            | WorldfileSection::ModuleAdd { library, package, module, .. }
            | WorldfileSection::ModuleRemove { library, package, module }
            | WorldfileSection::ModuleChangeWs { library, package, module, .. }
            | WorldfileSection::ModuleChangeAst { library, package, module, .. }
            | WorldfileSection::ModuleChangeTy { library, package, module, .. } => {
                Some(ModulePath::new(library.clone(), package.clone(), module.clone()))
            }
            WorldfileSection::ScriptFragment { .. } | WorldfileSection::ScriptExpr { .. } => None,
        }
    }

    /// Get the source code if this section has source.
    pub fn source(&self) -> Option<&str> {
        match self {
            WorldfileSection::Module { source, .. }
            | WorldfileSection::ModuleAdd { source, .. }
            | WorldfileSection::ModuleChangeWs { source, .. }
            | WorldfileSection::ModuleChangeAst { source, .. }
            | WorldfileSection::ModuleChangeTy { source, .. }
            | WorldfileSection::ScriptFragment { source }
            | WorldfileSection::ScriptExpr { source } => Some(source),
            WorldfileSection::ModuleRemove { .. } => None,
        }
    }

    /// Get the section kind for module sections, or None for script sections.
    pub fn module_section_kind(&self) -> Option<ModuleSectionKind> {
        match self {
            WorldfileSection::Module { .. } => Some(ModuleSectionKind::Module),
            WorldfileSection::ModuleAdd { .. } => Some(ModuleSectionKind::Add),
            WorldfileSection::ModuleRemove { .. } => Some(ModuleSectionKind::Remove),
            WorldfileSection::ModuleChangeWs { .. } => Some(ModuleSectionKind::ChangeWs),
            WorldfileSection::ModuleChangeAst { .. } => Some(ModuleSectionKind::ChangeAst),
            WorldfileSection::ModuleChangeTy { .. } => Some(ModuleSectionKind::ChangeTy),
            WorldfileSection::ScriptFragment { .. } | WorldfileSection::ScriptExpr { .. } => None,
        }
    }

    /// Returns the section type as a string (for display/logging).
    pub fn section_type_str(&self) -> &'static str {
        match self {
            WorldfileSection::Module { .. } => "module",
            WorldfileSection::ModuleAdd { .. } => "module-add",
            WorldfileSection::ModuleRemove { .. } => "module-remove",
            WorldfileSection::ModuleChangeWs { .. } => "module-change-ws",
            WorldfileSection::ModuleChangeAst { .. } => "module-change-ast",
            WorldfileSection::ModuleChangeTy { .. } => "module-change-ty",
            WorldfileSection::ScriptFragment { .. } => "scriptunit-fragment",
            WorldfileSection::ScriptExpr { .. } => "scriptunit-expr",
        }
    }

    /// Returns true if this is a module-related section.
    pub fn is_module_section(&self) -> bool {
        self.module_path().is_some()
    }

    /// Returns true if this is a script-related section.
    pub fn is_script_section(&self) -> bool {
        matches!(self, WorldfileSection::ScriptFragment { .. } | WorldfileSection::ScriptExpr { .. })
    }
}

// ============================================================================
// Result Types
// ============================================================================

/// Result of loading a worldfile that may contain both modules and a script.
pub struct WorldfileWithScript {
    pub package_world: PackageWorld,
    pub script: Option<String>,
}

/// Result of parsing a worldfile into sections.
pub struct ParsedWorldfile {
    pub sections: Vec<WorldfileSection>,
}

// ============================================================================
// Public Loading Functions
// ============================================================================

/// Load a package world from a worldfile format byte stream.
///
/// This function parses only `module` sections and ignores script sections.
/// For worldfiles that may contain scripts, use [`load_worldfile_with_script`].
pub fn load_world_from_worldfile(reader: impl Read) -> AnyResult<PackageWorld> {
    let result = load_worldfile_with_script(reader)?;
    Ok(result.package_world)
}

/// Load a worldfile that may contain both modules and a script.
///
/// Parses `module` and `script`/`scriptunit-fragment` sections. The script
/// content is returned separately from the package world.
pub fn load_worldfile_with_script(mut reader: impl Read) -> AnyResult<WorldfileWithScript> {
    let mut content = String::new();
    reader.read_to_string(&mut content)?;

    let parsed = parse_worldfile_to_sections(&content)?;

    let mut pkglib_system = BTreeMap::new();
    let mut pkglib_local = BTreeMap::new();
    let mut script = None;

    for section in parsed {
        match section {
            WorldfileSection::Module { library, package, module, source } => {
                insert_module(&mut pkglib_system, &mut pkglib_local, &library, &package, &module, source)?;
            }
            WorldfileSection::ScriptFragment { source } => {
                script = Some(source);
            }
            // Ignore other section types for this loader.
            _ => {}
        }
    }

    Ok(WorldfileWithScript {
        package_world: PackageWorld {
            pkglib_system,
            pkglib_local,
        },
        script,
    })
}

/// Parse a worldfile into sections without building a PackageWorld.
///
/// This is the most general parsing function, returning all section types.
/// Useful for testing infrastructure that needs to process all sections.
pub fn parse_worldfile_sections(mut reader: impl Read) -> AnyResult<ParsedWorldfile> {
    let mut content = String::new();
    reader.read_to_string(&mut content)?;

    let sections = parse_worldfile_to_sections(&content)?;
    Ok(ParsedWorldfile { sections })
}

// ============================================================================
// Internal Helpers
// ============================================================================

/// Insert a module into the appropriate package library.
fn insert_module(
    pkglib_system: &mut BTreeMap<String, Package>,
    pkglib_local: &mut BTreeMap<String, Package>,
    library: &str,
    package: &str,
    module: &str,
    source: String,
) -> AnyResult<()> {
    let pkglib = match library {
        "sys" => pkglib_system,
        "local" => pkglib_local,
        other => bail!("unknown library '{other}' (must be 'sys' or 'local')"),
    };

    let pkg = pkglib.entry(package.S())
        .or_insert_with(|| Package {
            name: package.S(),
            modules: BTreeMap::new(),
        });

    let path_str = format!("{}/{}/{}", library, package, module);
    pkg.modules.insert(module.S(), PackageModule {
        name: module.S(),
        path: path_str.C().into(),
        text: source,
    });

    Ok(())
}

/// Module section header prefixes and their kinds, in order of specificity.
const MODULE_SECTION_PREFIXES: &[(&str, ModuleSectionKind)] = &[
    ("module-change-ws ", ModuleSectionKind::ChangeWs),
    ("module-change-ast ", ModuleSectionKind::ChangeAst),
    ("module-change-ty ", ModuleSectionKind::ChangeTy),
    ("module-remove ", ModuleSectionKind::Remove),
    ("module-add ", ModuleSectionKind::Add),
    ("module ", ModuleSectionKind::Module),
];

/// Parse a header line into a section kind and module path.
fn parse_module_header(header: &str) -> Option<(ModuleSectionKind, ModulePath)> {
    for (prefix, kind) in MODULE_SECTION_PREFIXES {
        if let Some(path_str) = header.strip_prefix(prefix) {
            if let Some(path) = ModulePath::parse(path_str) {
                return Some((*kind, path));
            }
        }
    }
    None
}

/// Create a WorldfileSection from a module section kind, path, and source.
fn make_module_section(kind: ModuleSectionKind, path: ModulePath, source: String) -> WorldfileSection {
    let ModulePath { library, package, module } = path;
    match kind {
        ModuleSectionKind::Module => WorldfileSection::Module { library, package, module, source },
        ModuleSectionKind::Add => WorldfileSection::ModuleAdd { library, package, module, source },
        ModuleSectionKind::Remove => WorldfileSection::ModuleRemove { library, package, module },
        ModuleSectionKind::ChangeWs => WorldfileSection::ModuleChangeWs { library, package, module, source },
        ModuleSectionKind::ChangeAst => WorldfileSection::ModuleChangeAst { library, package, module, source },
        ModuleSectionKind::ChangeTy => WorldfileSection::ModuleChangeTy { library, package, module, source },
    }
}

// ============================================================================
// Core Parser
// ============================================================================

/// Internal state for the worldfile parser.
struct Parser<'a> {
    lines: Vec<&'a str>,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(content: &'a str) -> Self {
        Self {
            lines: content.lines().collect(),
            pos: 0,
        }
    }

    /// Current line number (1-indexed for error messages).
    fn line_num(&self) -> usize {
        self.pos + 1
    }

    /// Skip empty lines, returning an error if non-empty content is found before a separator.
    fn skip_to_separator(&mut self) -> AnyResult<bool> {
        while self.pos < self.lines.len() {
            let line = self.lines[self.pos];
            if is_separator(line) {
                return Ok(true);
            }
            if !line.trim().is_empty() {
                bail!("expected '----------' separator at line {}, found '{}'", self.line_num(), line);
            }
            self.pos += 1;
        }
        Ok(false)
    }

    /// Expect and consume a separator line.
    fn expect_separator(&mut self) -> AnyResult<()> {
        if self.pos >= self.lines.len() {
            bail!("unexpected end of file, expected separator");
        }
        if !is_separator(self.lines[self.pos]) {
            bail!("expected '----------' separator at line {}, found '{}'", self.line_num(), self.lines[self.pos]);
        }
        self.pos += 1;
        Ok(())
    }

    /// Read and return the header line.
    fn read_header(&mut self) -> AnyResult<&'a str> {
        if self.pos >= self.lines.len() {
            bail!("unexpected end of file after separator");
        }
        let header = self.lines[self.pos].trim();
        if header.is_empty() {
            bail!("expected section header at line {}, found empty line", self.line_num());
        }
        self.pos += 1;
        Ok(header)
    }

    /// Read content until the next separator (or end of file).
    fn read_content(&mut self) -> String {
        let mut content_lines = Vec::new();
        while self.pos < self.lines.len() && !is_separator(self.lines[self.pos]) {
            content_lines.push(self.lines[self.pos]);
            self.pos += 1;
        }
        content_lines.join("\n")
    }

    /// Parse a single section.
    fn parse_section(&mut self, header: &str) -> AnyResult<WorldfileSection> {
        // Script sections.
        if header == "scriptunit-fragment" || header == "script" {
            let source = self.read_content();
            return Ok(WorldfileSection::ScriptFragment { source });
        }

        if header == "scriptunit-expr" {
            let source = self.read_content();
            return Ok(WorldfileSection::ScriptExpr { source });
        }

        if header == "inline-directives" {
            let source = self.read_content();
            return Ok(WorldfileSection::InlineDirectives { source });
        }

        // Module sections.
        if let Some((kind, path)) = parse_module_header(header) {
            let source = self.read_content();
            return Ok(make_module_section(kind, path, source));
        }

        // Unknown section type.
        bail!(
            "unknown section type '{}' (expected 'module', 'module-add', 'module-remove', \
             'module-change-ws', 'module-change-ast', 'module-change-ty', 'scriptunit-fragment', \
             'scriptunit-expr', or 'inline-directives')",
            header
        );
    }
}

/// Parse worldfile content into sections.
fn parse_worldfile_to_sections(content: &str) -> AnyResult<Vec<WorldfileSection>> {
    let mut parser = Parser::new(content);
    let mut sections = Vec::new();

    while parser.skip_to_separator()? {
        parser.expect_separator()?;
        let header = parser.read_header()?;
        parser.expect_separator()?;
        let section = parser.parse_section(header)?;
        sections.push(section);
    }

    Ok(sections)
}

fn is_separator(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("----------") && trimmed.chars().all(|c| c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_worldfile() {
        let worldfile = r#"
----------
module sys/std/bool
----------

fun foo()
end fun

----------
module local/util/utils
----------

fun bar()
end fun
"#;

        let world = load_world_from_worldfile(worldfile.as_bytes()).X();

        assert_eq!(world.pkglib_system.len(), 1);
        assert_eq!(world.pkglib_local.len(), 1);

        let sys_std = world.pkglib_system.get("std").X();
        assert_eq!(sys_std.modules.len(), 1);
        assert!(sys_std.modules.contains_key("bool"));

        let local_util = world.pkglib_local.get("util").X();
        assert_eq!(local_util.modules.len(), 1);
        assert!(local_util.modules.contains_key("utils"));
    }

    #[test]
    fn test_multiple_modules_same_package() {
        let worldfile = r#"
----------
module sys/std/bool
----------

fun foo()
end fun

----------
module sys/std/int
----------

fun bar()
end fun
"#;

        let world = load_world_from_worldfile(worldfile.as_bytes()).X();

        assert_eq!(world.pkglib_system.len(), 1);
        let sys_std = world.pkglib_system.get("std").X();
        assert_eq!(sys_std.modules.len(), 2);
        assert!(sys_std.modules.contains_key("bool"));
        assert!(sys_std.modules.contains_key("int"));
    }

    #[test]
    fn test_multiple_packages_and_modules() {
        let worldfile = r#"
----------
module sys/std/bool
----------

fun bool_func()
end fun

----------
module sys/std/int
----------

fun int_func()
end fun

----------
module sys/collections/list
----------

fun list_func()
end fun

----------
module sys/collections/map
----------

fun map_func()
end fun

----------
module sys/collections/set
----------

fun set_func()
end fun
"#;

        let world = load_world_from_worldfile(worldfile.as_bytes()).X();

        // Should have 2 packages in sys library.
        assert_eq!(world.pkglib_system.len(), 2);
        assert_eq!(world.pkglib_local.len(), 0);

        // Check std package has 2 modules.
        let sys_std = world.pkglib_system.get("std").X();
        assert_eq!(sys_std.modules.len(), 2);
        assert!(sys_std.modules.contains_key("bool"));
        assert!(sys_std.modules.contains_key("int"));

        // Check collections package has 3 modules.
        let sys_collections = world.pkglib_system.get("collections").X();
        assert_eq!(sys_collections.modules.len(), 3);
        assert!(sys_collections.modules.contains_key("list"));
        assert!(sys_collections.modules.contains_key("map"));
        assert!(sys_collections.modules.contains_key("set"));
    }

    #[test]
    fn test_worldfile_with_script() {
        let worldfile = r#"
----------
module sys/std/u32
----------

fun add(x: u32, y: u32): u32
  ret x + y
end fun

----------
script
----------

require module sys/std/u32
import u32.add
let output = add(@5, @10)
"#;

        let result = load_worldfile_with_script(worldfile.as_bytes()).X();

        // Check package world.
        assert_eq!(result.package_world.pkglib_system.len(), 1);
        let sys_std = result.package_world.pkglib_system.get("std").X();
        assert_eq!(sys_std.modules.len(), 1);
        assert!(sys_std.modules.contains_key("u32"));

        // Check script.
        assert!(result.script.is_some());
        let script = result.script.unwrap();
        assert!(script.contains("require module sys/std/u32"));
        assert!(script.contains("import u32.add"));
        assert!(script.contains("let output = add(@5, @10)"));
    }

    #[test]
    fn test_worldfile_without_script() {
        let worldfile = r#"
----------
module sys/std/bool
----------

fun foo()
end fun
"#;

        let result = load_worldfile_with_script(worldfile.as_bytes()).X();

        // Check package world.
        assert_eq!(result.package_world.pkglib_system.len(), 1);

        // Check no script.
        assert!(result.script.is_none());
    }
}
