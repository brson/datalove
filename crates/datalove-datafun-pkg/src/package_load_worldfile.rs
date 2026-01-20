use rmx::prelude::*;
use rmx::std::collections::BTreeMap;
use rmx::std::io::Read;

use crate::package_load::{PackageWorld, Package, PackageModule};

/// Result of loading a worldfile that may contain both modules and a script.
pub struct WorldfileWithScript {
    pub package_world: PackageWorld,
    pub script: Option<String>,
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
}

/// Result of parsing a worldfile into sections.
pub struct ParsedWorldfile {
    pub sections: Vec<WorldfileSection>,
}

/// Load a package world from a worldfile format byte stream.
///
/// The worldfile format consists of sections separated by "----------" lines.
/// Each section has a header line with "module library/package/module",
/// followed by another "----------" line, then the source code.
///
/// Example:
/// ```notrust
/// ----------
/// module sys/std/bool
/// ----------
///
/// fun foo()
/// end fun
///
/// ----------
/// module local/util/utils
/// ----------
///
/// fun bar()
/// end fun
/// ```
pub fn load_world_from_worldfile(
    reader: impl Read,
) -> AnyResult<PackageWorld> {
    let result = load_worldfile_with_script(reader)?;
    Ok(result.package_world)
}

/// Load a worldfile that may contain both modules and a script.
///
/// The worldfile format supports an optional "script" section:
/// ```notrust
/// ----------
/// module sys/std/u32
/// ----------
///
/// fun add(x: @u32, y: @u32): @u32
///   ret x + y
/// end fun
///
/// ----------
/// script
/// ----------
///
/// require module sys/std/u32
/// import u32.add
/// let output = add(@5, @10)
/// ```
pub fn load_worldfile_with_script(
    mut reader: impl Read,
) -> AnyResult<WorldfileWithScript> {
    let mut content = String::new();
    reader.read_to_string(&mut content)?;

    let (sections, script) = parse_worldfile(&content)?;

    let mut pkglib_system = BTreeMap::new();
    let mut pkglib_local = BTreeMap::new();

    for section in sections {
        let library = match section.library.as_str() {
            "sys" => &mut pkglib_system,
            "local" => &mut pkglib_local,
            other => bail!("unknown library '{other}' (must be 'sys' or 'local')"),
        };

        let package = library.entry(section.package.C())
            .or_insert_with(|| Package {
                name: section.package.C(),
                modules: BTreeMap::new(),
            });

        let module_path_str = format!("{}/{}/{}", section.library, section.package, section.module);

        let module = PackageModule {
            name: section.module.C(),
            path: module_path_str.C().into(),
            text: section.source,
        };

        package.modules.insert(section.module, module);
    }

    Ok(WorldfileWithScript {
        package_world: PackageWorld {
            pkglib_system,
            pkglib_local,
        },
        script,
    })
}

struct Section {
    library: String,
    package: String,
    module: String,
    source: String,
}

/// Parse a worldfile into sections without building a PackageWorld.
///
/// This is useful for testing infrastructure that needs to process
/// all section types (module, scriptunit, expr, script) sequentially.
pub fn parse_worldfile_sections(
    mut reader: impl Read,
) -> AnyResult<ParsedWorldfile> {
    let mut content = String::new();
    reader.read_to_string(&mut content)?;

    let sections = parse_worldfile_to_sections(&content)?;
    Ok(ParsedWorldfile { sections })
}

fn parse_worldfile_to_sections(content: &str) -> AnyResult<Vec<WorldfileSection>> {
    let mut sections = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        // Skip empty lines and find first separator.
        while i < lines.len() && !is_separator(lines[i]) {
            if !lines[i].trim().is_empty() {
                bail!("expected '----------' separator at line {}, found '{}'", i + 1, lines[i]);
            }
            i += 1;
        }

        if i >= lines.len() {
            break;
        }

        // Skip the first separator.
        i += 1;

        if i >= lines.len() {
            bail!("unexpected end of file after separator");
        }

        // Read the header line.
        let header_line = lines[i].trim();
        if header_line.is_empty() {
            bail!("expected section header at line {}, found empty line", i + 1);
        }

        i += 1;

        if i >= lines.len() {
            bail!("unexpected end of file after header");
        }

        // Expect second separator.
        if !is_separator(lines[i]) {
            bail!("expected '----------' separator at line {}, found '{}'", i + 1, lines[i]);
        }

        i += 1;

        // Read source until next separator or end.
        let mut source_lines = Vec::new();
        while i < lines.len() && !is_separator(lines[i]) {
            source_lines.push(lines[i]);
            i += 1;
        }

        let source = source_lines.join("\n");

        // Determine section type and create appropriate variant.
        if header_line == "scriptunit-fragment" {
            sections.push(WorldfileSection::ScriptFragment { source });
        } else if header_line == "scriptunit-expr" {
            sections.push(WorldfileSection::ScriptExpr { source });
        } else if let Some(path) = header_line.strip_prefix("module-remove ") {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() != 3 {
                bail!("module-remove path must be 'module-remove library/package/module', got '{header_line}'");
            }
            sections.push(WorldfileSection::ModuleRemove {
                library: S(parts[0]),
                package: S(parts[1]),
                module: S(parts[2]),
            });
        } else if let Some(path) = header_line.strip_prefix("module-add ") {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() != 3 {
                bail!("module-add path must be 'module-add library/package/module', got '{header_line}'");
            }
            sections.push(WorldfileSection::ModuleAdd {
                library: S(parts[0]),
                package: S(parts[1]),
                module: S(parts[2]),
                source,
            });
        } else if let Some(path) = header_line.strip_prefix("module-change-ws ") {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() != 3 {
                bail!("module-change-ws path must be 'module-change-ws library/package/module', got '{header_line}'");
            }
            sections.push(WorldfileSection::ModuleChangeWs {
                library: S(parts[0]),
                package: S(parts[1]),
                module: S(parts[2]),
                source,
            });
        } else if let Some(path) = header_line.strip_prefix("module-change-ast ") {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() != 3 {
                bail!("module-change-ast path must be 'module-change-ast library/package/module', got '{header_line}'");
            }
            sections.push(WorldfileSection::ModuleChangeAst {
                library: S(parts[0]),
                package: S(parts[1]),
                module: S(parts[2]),
                source,
            });
        } else if let Some(path) = header_line.strip_prefix("module-change-ty ") {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() != 3 {
                bail!("module-change-ty path must be 'module-change-ty library/package/module', got '{header_line}'");
            }
            sections.push(WorldfileSection::ModuleChangeTy {
                library: S(parts[0]),
                package: S(parts[1]),
                module: S(parts[2]),
                source,
            });
        } else if let Some(path) = header_line.strip_prefix("module ") {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() != 3 {
                bail!("module path must be 'module library/package/module', got '{header_line}'");
            }
            sections.push(WorldfileSection::Module {
                library: S(parts[0]),
                package: S(parts[1]),
                module: S(parts[2]),
                source,
            });
        } else {
            bail!("unknown section type '{header_line}' (expected 'module', 'module-add', 'module-remove', 'module-change-ws', 'module-change-ast', 'module-change-ty', 'scriptunit-fragment', or 'scriptunit-expr')");
        }
    }

    Ok(sections)
}

fn parse_worldfile(content: &str) -> AnyResult<(Vec<Section>, Option<String>)> {
    let mut sections = Vec::new();
    let mut script = None;
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        // Skip empty lines and find first separator.
        while i < lines.len() && !is_separator(lines[i]) {
            if !lines[i].trim().is_empty() {
                bail!("expected '----------' separator at line {}, found '{}'", i + 1, lines[i]);
            }
            i += 1;
        }

        if i >= lines.len() {
            break;
        }

        // Skip the first separator.
        i += 1;

        if i >= lines.len() {
            bail!("unexpected end of file after separator");
        }

        // Read the header line.
        let header_line = lines[i].trim();
        if header_line.is_empty() {
            bail!("expected 'module library/package/module' or 'script' at line {}, found empty line", i + 1);
        }

        // Check if this is a script section.
        // Accept both "script" and "scriptunit-fragment" for compatibility.
        if header_line == "script" || header_line == "scriptunit-fragment" {
            i += 1;

            if i >= lines.len() {
                bail!("unexpected end of file after 'script' header");
            }

            // Expect second separator.
            if !is_separator(lines[i]) {
                bail!("expected '----------' separator at line {}, found '{}'", i + 1, lines[i]);
            }

            i += 1;

            // Read script source until next separator or end.
            let mut script_lines = Vec::new();
            while i < lines.len() && !is_separator(lines[i]) {
                script_lines.push(lines[i]);
                i += 1;
            }

            script = Some(script_lines.join("\n"));
            continue;
        }

        // Otherwise, it must be a module section.
        let Some(path) = header_line.strip_prefix("module ") else {
            bail!("expected 'module' prefix or 'script' at line {}, found '{header_line}'", i + 1);
        };

        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() != 3 {
            bail!("path must be 'module library/package/module', got '{header_line}'");
        }

        let library = S(parts[0]);
        let package = S(parts[1]);
        let module = S(parts[2]);

        i += 1;

        if i >= lines.len() {
            bail!("unexpected end of file after path");
        }

        // Expect second separator.
        if !is_separator(lines[i]) {
            bail!("expected '----------' separator at line {}, found '{}'", i + 1, lines[i]);
        }

        i += 1;

        // Read source until next separator or end.
        let mut source_lines = Vec::new();
        while i < lines.len() && !is_separator(lines[i]) {
            source_lines.push(lines[i]);
            i += 1;
        }

        let source = source_lines.join("\n");

        sections.push(Section {
            library,
            package,
            module,
            source,
        });
    }

    Ok((sections, script))
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

fun add(x: @u32, y: @u32): @u32
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
