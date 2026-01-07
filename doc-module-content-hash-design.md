# Module Content Hash Design for Salsa Incremental Compilation

## Problem Statement

Currently, salsa module analysis reruns even when module content hasn't changed because:
1. **Identity-based memoization**: Source objects are tracked by identity, not content
2. **No transitive dependency tracking**: Changes don't propagate based on content
3. **Missing configuration tracking**: Compiler settings that affect analysis aren't hashed

**Goal**: Develop a recursive content hash that captures:
- Module text content
- Transitive module dependencies
- Compiler configuration affecting module analysis
- Pre-parsing module graph structure

Salsa should only recompute when this hash changes.

---

## Current Architecture Analysis

### Compilation Pipeline Flow

```
PackageWorld (pkglib_system, pkglib_local)
    ↓
import_from_loader() - Convert to Salsa types
    ↓
PackageModule::new(db, name, Source::new(db, text))  ← Creates Source by identity
    ↓
resolve_package_world_with_imports() - Resolve dependencies
    ↓
to_module_graph() - Topological sort, build ModuleGraph
    ↓
parse_module_graph(db, graph) - #[salsa::tracked]
    ↓
typecheck_module_graph(db, parsed) - #[salsa::tracked]
    ↓
analyze_module_graph() - Optional analysis
```

### Key Salsa Inputs

| Type | Salsa Attribute | Content |
|------|----------------|---------|
| `Source` | `#[salsa::input]` | Text content (from bct) |
| `PackageWorld` | `#[salsa::tracked]` | System/local package libraries |
| `PackageModule` | `#[salsa::tracked]` | Name + Source |
| `ModuleGraph` | `#[salsa::tracked]` | Collection of modules |

### Current Memoization Gaps

1. **Source Identity Problem**
   - `Source::new(db, "let x = 1")` creates object A
   - Later: `Source::new(db, "let x = 1")` creates object B
   - A ≠ B in salsa, so all downstream queries recompute

2. **No Cross-Session Persistence**
   - Database is recreated each run
   - No content-based comparison across sessions
   - All modules re-parse on every run

3. **Implicit Dependency Tracking**
   - Module A imports Module B
   - Module B changes → Should invalidate A
   - But no explicit content hash captures this relationship

4. **Configuration Blindness**
   - `enable_analysis` flag changes
   - Package library paths change
   - No hash captures these inputs

---

## Design Approaches

### Approach 1: Source-Level Content Deduplication

**Idea**: Hash source text before creating Source objects, deduplicate by hash.

```rust
pub struct SourceCache {
    /// Map from content hash to Source
    cache: HashMap<u64, bct::input::Source>,
}

impl SourceCache {
    pub fn get_or_create(&mut self, db: &dyn Database, text: String) -> bct::input::Source {
        let hash = hash_string(&text);
        if let Some(source) = self.cache.get(&hash) {
            *source
        } else {
            let source = bct::input::Source::new(db, text);
            self.cache.insert(hash, source);
            source
        }
    }
}
```

**Pros**:
- Minimal changes to existing code
- Automatic deduplication within a session
- Simple to implement

**Cons**:
- Doesn't help across database sessions
- Doesn't capture transitive dependencies
- Doesn't track configuration changes
- Limited incremental benefit

**Verdict**: ❌ Too limited for comprehensive solution

---

### Approach 2: Module-Level Content Hash (Tracked)

**Idea**: Add a salsa-tracked content hash for each module that combines its text with dependency hashes.

```rust
#[salsa::tracked]
pub struct ModuleContentHash {
    /// Hash of this module's source text
    pub source_hash: u64,
    /// Hashes of direct dependencies
    pub dependency_hashes: Vec<u64>,
    /// Combined recursive hash
    #[returns(ref)]
    pub total_hash: u64,
}

#[salsa::tracked]
pub fn compute_module_hash<'db>(
    db: &'db dyn Database,
    module: Module,
    dependency_hashes: Vec<ModuleContentHash>,
) -> ModuleContentHash {
    let source = module.source(db);
    let text = source.text(db);
    let source_hash = hash_string(text.as_str(db));

    let mut dep_hashes = dependency_hashes
        .iter()
        .map(|h| h.total_hash(db))
        .collect::<Vec<_>>();
    dep_hashes.sort(); // Stable ordering

    let mut hasher = DefaultHasher::new();
    hasher.write_u64(source_hash);
    for h in &dep_hashes {
        hasher.write_u64(*h);
    }
    let total_hash = hasher.finish();

    ModuleContentHash::new(db, source_hash, dep_hashes, total_hash)
}
```

**Pros**:
- Salsa automatically tracks hash changes
- Recursive dependency hashing
- Minimal invasiveness

**Cons**:
- Still identity-based at Source level
- Doesn't persist across sessions
- Must compute hashes eagerly

**Verdict**: ✅ Good foundation, but needs enhancement

---

### Approach 3: Pre-Parse Module Graph Content Hash

**Idea**: Compute a complete graph hash BEFORE parsing, use it as memoization key.

```rust
/// Configuration that affects module analysis
#[derive(Debug, Clone, Hash)]
pub struct ModuleAnalysisConfig {
    pub enable_analysis: bool,
    pub pkglib_system_paths: Vec<PathBuf>,
    pub pkglib_local_paths: Vec<PathBuf>,
    // Future: optimization level, feature flags, etc.
}

/// Pre-computed content hash for the entire module graph
#[salsa::input]
pub struct ModuleGraphContentHash {
    /// Configuration affecting analysis
    #[returns(ref)]
    pub config: ModuleAnalysisConfig,
    /// Map: module_path -> (text_hash, dependency_paths)
    #[returns(ref)]
    pub module_hashes: BTreeMap<String, (u64, Vec<String>)>,
    /// Recursive hash of entire graph
    pub graph_hash: u64,
}

impl ModuleGraphContentHash {
    pub fn compute(
        config: ModuleAnalysisConfig,
        modules: &BTreeMap<String, (String, Vec<String>)>,
    ) -> Self {
        // 1. Hash each module's text
        let mut module_hashes = BTreeMap::new();
        for (path, (text, deps)) in modules {
            let text_hash = hash_string(text);
            module_hashes.insert(path.clone(), (text_hash, deps.clone()));
        }

        // 2. Compute recursive hashes (topological order)
        let mut recursive_hashes = BTreeMap::new();
        fn compute_recursive(
            path: &str,
            module_hashes: &BTreeMap<String, (u64, Vec<String>)>,
            cache: &mut BTreeMap<String, u64>,
        ) -> u64 {
            if let Some(&hash) = cache.get(path) {
                return hash;
            }

            let (text_hash, deps) = &module_hashes[path];
            let mut dep_hashes: Vec<u64> = deps
                .iter()
                .map(|d| compute_recursive(d, module_hashes, cache))
                .collect();
            dep_hashes.sort();

            let mut hasher = DefaultHasher::new();
            hasher.write_u64(*text_hash);
            for h in dep_hashes {
                hasher.write_u64(h);
            }
            let hash = hasher.finish();
            cache.insert(path.to_string(), hash);
            hash
        }

        for path in module_hashes.keys() {
            compute_recursive(path, &module_hashes, &mut recursive_hashes);
        }

        // 3. Compute graph-level hash
        let mut hasher = DefaultHasher::new();
        config.hash(&mut hasher);
        for (_path, &hash) in &recursive_hashes {
            hasher.write_u64(hash);
        }
        let graph_hash = hasher.finish();

        ModuleGraphContentHash { config, module_hashes, graph_hash }
    }
}

// Usage in pipeline
#[salsa::tracked]
pub fn parse_module_graph_cached<'db>(
    db: &'db dyn Database,
    content_hash: ModuleGraphContentHash,
    graph: ModuleGraph,
) -> ParsedModuleGraph<'db> {
    // Salsa memoizes by content_hash as input
    // If hash hasn't changed, returns cached ParsedModuleGraph
    parse_module_graph_impl(db, graph)
}
```

**Pros**:
- ✅ Captures module text content
- ✅ Captures transitive dependencies
- ✅ Captures configuration
- ✅ Works across salsa sessions (if hash persisted)
- ✅ Early bailout before parsing
- ✅ Salsa's built-in memoization does the work

**Cons**:
- Requires extracting module info before loading into salsa
- Need to maintain parallel hash computation
- Must carefully handle topological ordering

**Verdict**: ✅✅✅ **BEST APPROACH** - Comprehensive and leverages salsa correctly

---

### Approach 4: Persistent Hash Cache (External)

**Idea**: Store content hashes in external cache (e.g., filesystem), load before salsa queries.

```rust
pub struct PersistentHashCache {
    cache_file: PathBuf,
    hashes: HashMap<String, u64>,
}

impl PersistentHashCache {
    pub fn load(cache_dir: &Path) -> Result<Self> {
        let cache_file = cache_dir.join(".datalove-module-cache.json");
        let hashes = if cache_file.exists() {
            let data = std::fs::read_to_string(&cache_file)?;
            serde_json::from_str(&data)?
        } else {
            HashMap::new()
        };
        Ok(Self { cache_file, hashes })
    }

    pub fn check_changed(&self, path: &str, content: &str) -> bool {
        let hash = hash_string(content);
        self.hashes.get(path) != Some(&hash)
    }

    pub fn update(&mut self, path: String, content: &str) {
        let hash = hash_string(content);
        self.hashes.insert(path, hash);
    }

    pub fn save(&self) -> Result<()> {
        let data = serde_json::to_string(&self.hashes)?;
        std::fs::write(&self.cache_file, data)?;
        Ok(())
    }
}
```

**Pros**:
- Persists across process restarts
- Enables true incremental compilation
- Can skip loading unchanged modules entirely

**Cons**:
- Requires filesystem I/O
- Cache invalidation complexity
- Must handle cache corruption
- Doesn't integrate with salsa's memoization

**Verdict**: ✅ Useful as **complement** to Approach 3, not replacement

---

## Recommended Solution: Hybrid Approach

**Combine Approach 3 (salsa-tracked hash) + Approach 4 (persistent cache)**

### Phase 1: Salsa-Tracked Content Hash (Core)

1. **Add ModuleAnalysisConfig**
   ```rust
   #[derive(Debug, Clone, PartialEq, Eq, Hash)]
   pub struct ModuleAnalysisConfig {
       pub enable_analysis: bool,
       // Future: optimization flags, etc.
   }
   ```

2. **Compute Pre-Parse Hash**
   ```rust
   pub fn compute_module_graph_hash(
       config: &ModuleAnalysisConfig,
       modules: &BTreeMap<String, PackageModule>,
   ) -> ModuleGraphHash {
       // For each module:
       // - Hash its text
       // - Recursively hash its dependencies
       // - Combine with config hash
   }
   ```

3. **Use Hash as Salsa Input**
   ```rust
   #[salsa::tracked]
   pub fn parse_module_graph_with_hash<'db>(
       db: &'db dyn Database,
       graph_hash: ModuleGraphHash,  // Salsa input
       graph: ModuleGraph,
   ) -> ParsedModuleGraph<'db> {
       // Memoized by graph_hash
       parse_module_graph(db, graph)
   }
   ```

### Phase 2: Persistent Cache (Optimization)

1. **Cache File Format**
   ```json
   {
     "version": "1.0",
     "hashes": {
       "sys/std/list": {
         "text_hash": 12345678,
         "recursive_hash": 87654321,
         "dependencies": ["sys/std/base"]
       }
     }
   }
   ```

2. **Pre-Load Check**
   ```rust
   // Before creating PackageWorld
   let cache = HashCache::load()?;
   let changed_modules = cache.find_changed(&package_world_raw);

   if changed_modules.is_empty() {
       // Fast path: reuse previous compilation
       return cached_result;
   }

   // Slow path: recompile changed modules
   ```

---

## Implementation Strategy

### Step 1: Define Hash Types

**File**: `crates/datalove-datafun-compiler/src/module_hash.rs`

```rust
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

/// Hash a string to u64
pub fn hash_string(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Configuration affecting module analysis
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModuleAnalysisConfig {
    pub enable_analysis: bool,
    // Future expansion:
    // pub optimization_level: u8,
    // pub feature_flags: Vec<String>,
}

/// Content hash for a single module
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleHash {
    /// Module path (e.g., "sys/std/list")
    pub path: String,
    /// Hash of source text only
    pub text_hash: u64,
    /// Paths of direct dependencies
    pub dependencies: Vec<String>,
    /// Recursive hash (includes text + all transitive deps)
    pub recursive_hash: u64,
}

/// Content hash for entire module graph
#[salsa::input]
pub struct ModuleGraphHash {
    /// Configuration hash
    #[returns(ref)]
    pub config: ModuleAnalysisConfig,
    /// Per-module hashes
    #[returns(ref)]
    pub modules: BTreeMap<String, ModuleHash>,
    /// Combined graph hash
    pub graph_hash: u64,
}

impl ModuleGraphHash {
    /// Compute hash from raw package data (before salsa)
    pub fn compute(
        config: ModuleAnalysisConfig,
        package_world: &crate::package_load::PackageWorld,
        dependency_map: &HashMap<String, Vec<String>>,
    ) -> Self {
        let mut modules = BTreeMap::new();
        let mut recursive_cache = HashMap::new();

        // Collect all module paths and text hashes
        for (lib_name, packages) in [
            ("sys", &package_world.pkglib_system),
            ("local", &package_world.pkglib_local),
        ] {
            for (pkg_name, package) in packages {
                for (mod_name, module) in &package.modules {
                    let path = format!("{}/{}/{}", lib_name, pkg_name, mod_name);
                    let text_hash = hash_string(&module.text);
                    let deps = dependency_map.get(&path).cloned().unwrap_or_default();

                    modules.insert(path.clone(), ModuleHash {
                        path: path.clone(),
                        text_hash,
                        dependencies: deps,
                        recursive_hash: 0, // Computed below
                    });
                }
            }
        }

        // Compute recursive hashes (topological order via DFS)
        fn compute_recursive(
            path: &str,
            modules: &BTreeMap<String, ModuleHash>,
            cache: &mut HashMap<String, u64>,
        ) -> u64 {
            if let Some(&hash) = cache.get(path) {
                return hash;
            }

            let module = &modules[path];
            let text_hash = module.text_hash;

            let mut dep_hashes: Vec<u64> = module.dependencies
                .iter()
                .map(|dep| compute_recursive(dep, modules, cache))
                .collect();
            dep_hashes.sort(); // Stable ordering

            let mut hasher = DefaultHasher::new();
            hasher.write_u64(text_hash);
            for h in dep_hashes {
                hasher.write_u64(h);
            }
            let hash = hasher.finish();

            cache.insert(path.to_string(), hash);
            hash
        }

        // Fill in recursive hashes
        for path in modules.keys().cloned().collect::<Vec<_>>() {
            let recursive_hash = compute_recursive(&path, &modules, &mut recursive_cache);
            modules.get_mut(&path).unwrap().recursive_hash = recursive_hash;
        }

        // Compute graph-wide hash
        let mut hasher = DefaultHasher::new();
        config.hash(&mut hasher);
        for (_, module) in &modules {
            hasher.write_u64(module.recursive_hash);
        }
        let graph_hash = hasher.finish();

        ModuleGraphHash::new(db, config, modules, graph_hash)
    }
}
```

### Step 2: Extract Dependencies Before Salsa

**Problem**: We need dependency information BEFORE loading into salsa.

**Solution**: Two-pass loading
1. First pass: Quick parse to extract imports (don't create salsa objects)
2. Compute hash
3. Second pass: Load into salsa only if hash changed

```rust
// In package_resolve.rs
pub fn extract_import_demands_raw(
    text: &str,
) -> Vec<String> {
    // Quick parse just for imports (no salsa)
    // This is a lightweight operation
    datafun_parser::parse_imports_only(text)
}

pub fn build_dependency_map(
    package_world: &PackageWorld,
) -> HashMap<String, Vec<String>> {
    let mut deps = HashMap::new();

    for (lib, packages) in [
        ("sys", &package_world.pkglib_system),
        ("local", &package_world.pkglib_local),
    ] {
        for (pkg, package) in packages {
            for (mod_name, module) in &package.modules {
                let path = format!("{}/{}/{}", lib, pkg, mod_name);
                let imports = extract_import_demands_raw(&module.text);
                deps.insert(path, imports);
            }
        }
    }

    deps
}
```

### Step 3: Integrate Into Pipeline

**File**: `crates/datalove-datafun/src/pipeline.rs`

```rust
impl<'db> ModuleCompilationPipeline<'db> {
    pub fn compile(self) -> CompiledModules<'db> {
        // NEW: Build raw package world
        let raw_package_world = PackageWorld {
            pkglib_system: self.pkglib_system,
            pkglib_local: self.pkglib_local,
        };

        // NEW: Extract dependencies without salsa
        let dependency_map = build_dependency_map(&raw_package_world);

        // NEW: Compute content hash
        let config = ModuleAnalysisConfig {
            enable_analysis: self.enable_analysis,
        };
        let graph_hash = ModuleGraphHash::compute(
            config,
            &raw_package_world,
            &dependency_map,
        );

        // Existing: Load into salsa
        let package_world = import_from_loader(self.db, raw_package_world);

        // Existing: Resolve and build graph
        let resolution = resolve_package_world_with_imports(self.db, package_world);
        let pkg_graph = resolution.result(self.db)?;
        let module_graph = to_module_graph(self.db, package_world, pkg_graph);

        // NEW: Use hash-aware parsing
        let parsed_graph = parse_module_graph_with_hash(
            self.db,
            graph_hash,  // Salsa memoizes on this
            module_graph.clone(),
        );

        // Rest of compilation...
    }
}
```

### Step 4: Update Parse Function

```rust
// In module_graph.rs
#[salsa::tracked]
pub fn parse_module_graph_with_hash<'db>(
    db: &'db dyn salsa::Database,
    _graph_hash: ModuleGraphHash,  // Used only for memoization
    graph: ModuleGraph,
) -> ParsedModuleGraph<'db> {
    // Implementation unchanged
    parse_module_graph(db, graph)
}
```

**Key Insight**: Salsa memoizes based on `_graph_hash`. If hash is same as previous call, it returns cached `ParsedModuleGraph` without executing the body!

---

## Testing Strategy

### Test 1: Unchanged Module → No Reparse

```rust
#[test]
fn test_unchanged_module_cached() {
    let db = Database::default();

    // First compilation
    let pipeline1 = ModuleCompilationPipeline::new(&db)
        .add_module("sys", "test", "main", "let x = 1");
    let result1 = pipeline1.compile();

    // Second compilation (same content)
    let pipeline2 = ModuleCompilationPipeline::new(&db)
        .add_module("sys", "test", "main", "let x = 1");
    let result2 = pipeline2.compile();

    // Hash should be identical
    assert_eq!(
        result1.graph_hash.graph_hash(&db),
        result2.graph_hash.graph_hash(&db)
    );

    // Parse should be memoized (check via salsa query logs)
    assert!(parse_was_cached());
}
```

### Test 2: Changed Module → Reparse

```rust
#[test]
fn test_changed_module_recomputes() {
    let db = Database::default();

    let pipeline1 = ModuleCompilationPipeline::new(&db)
        .add_module("sys", "test", "main", "let x = 1");
    let result1 = pipeline1.compile();

    // Change content
    let pipeline2 = ModuleCompilationPipeline::new(&db)
        .add_module("sys", "test", "main", "let x = 2");
    let result2 = pipeline2.compile();

    // Hash should differ
    assert_ne!(
        result1.graph_hash.graph_hash(&db),
        result2.graph_hash.graph_hash(&db)
    );
}
```

### Test 3: Transitive Dependency Change

```rust
#[test]
fn test_transitive_dependency_invalidation() {
    let db = Database::default();

    // Module A depends on B
    let pipeline1 = ModuleCompilationPipeline::new(&db)
        .add_module("sys", "test", "b", "export let y = 1")
        .add_module("sys", "test", "a", "import {y} from sys/test/b\nlet x = y");
    let result1 = pipeline1.compile();

    // Change B only
    let pipeline2 = ModuleCompilationPipeline::new(&db)
        .add_module("sys", "test", "b", "export let y = 2")
        .add_module("sys", "test", "a", "import {y} from sys/test/b\nlet x = y");
    let result2 = pipeline2.compile();

    // Hash of A should change (even though A's text didn't change)
    let hash_a1 = result1.graph_hash.modules(&db).get("sys/test/a").unwrap();
    let hash_a2 = result2.graph_hash.modules(&db).get("sys/test/a").unwrap();
    assert_ne!(hash_a1.recursive_hash, hash_a2.recursive_hash);
}
```

### Test 4: Configuration Change

```rust
#[test]
fn test_config_change_invalidates() {
    let db = Database::default();

    let pipeline1 = ModuleCompilationPipeline::new(&db)
        .enable_analysis(false)
        .add_module("sys", "test", "main", "let x = 1");
    let result1 = pipeline1.compile();

    let pipeline2 = ModuleCompilationPipeline::new(&db)
        .enable_analysis(true)  // Changed!
        .add_module("sys", "test", "main", "let x = 1");
    let result2 = pipeline2.compile();

    // Graph hash should differ
    assert_ne!(
        result1.graph_hash.graph_hash(&db),
        result2.graph_hash.graph_hash(&db)
    );
}
```

### Test 5: Module Rename (No Content Change)

```rust
#[test]
fn test_module_rename_changes_hash() {
    let db = Database::default();

    let pipeline1 = ModuleCompilationPipeline::new(&db)
        .add_module("sys", "test", "foo", "let x = 1");
    let result1 = pipeline1.compile();

    // Same content, different path
    let pipeline2 = ModuleCompilationPipeline::new(&db)
        .add_module("sys", "test", "bar", "let x = 1");
    let result2 = pipeline2.compile();

    // Graph hash should differ (different module paths)
    assert_ne!(
        result1.graph_hash.graph_hash(&db),
        result2.graph_hash.graph_hash(&db)
    );
}
```

---

## Edge Cases and Considerations

### 1. Circular Dependencies

**Problem**: Recursive hash computation requires topological order. Cycles break this.

**Solution**:
- Detect cycles during dependency extraction
- For cycles, use sorted lexicographic order of module paths as tiebreaker
- Hash the cycle as a unit

```rust
fn compute_recursive_with_cycles(
    path: &str,
    modules: &BTreeMap<String, ModuleHash>,
    cache: &mut HashMap<String, u64>,
    visiting: &mut HashSet<String>,
) -> Result<u64, Vec<String>> {
    if let Some(&hash) = cache.get(path) {
        return Ok(hash);
    }

    if visiting.contains(path) {
        // Cycle detected
        return Err(vec![path.to_string()]);
    }

    visiting.insert(path.to_string());

    // ... rest of computation
}
```

### 2. Missing Dependencies

**Problem**: Module A imports B, but B doesn't exist.

**Solution**:
- Hash should include "missing dependency" marker
- Use sentinel hash value (e.g., 0) for missing deps
- This ensures hash changes when missing dep is added

### 3. Import Path Ambiguity

**Problem**: `import {x} from "foo"` - is it `sys/foo` or `local/foo`?

**Solution**:
- Use same resolution logic as package resolver
- Include resolved paths in dependency map
- Hash the resolved path, not the import string

### 4. Hash Collisions

**Problem**: Two different module graphs hash to same value.

**Solution**:
- Use cryptographic hash (SHA-256) instead of DefaultHasher
- Or: Include module count and path list in hash
- Trade-off: Performance vs. collision resistance

```rust
use sha2::{Sha256, Digest};

pub fn hash_string_crypto(s: &str) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    let result = hasher.finalize();
    // Take first 8 bytes as u64
    u64::from_be_bytes(result[0..8].try_into().unwrap())
}
```

### 5. Whitespace and Formatting

**Problem**: Should `let x=1` hash differently from `let x = 1`?

**Options**:
1. **Exact hash**: Hash raw text (current approach)
   - Pro: Simple, deterministic
   - Con: Format changes trigger recompilation

2. **Normalized hash**: Hash AST structure
   - Pro: Resilient to formatting
   - Con: Requires parsing to compute hash (defeats purpose)

**Recommendation**: Use exact hash. Formatting changes are rare and meaningful (could affect error messages).

### 6. Cross-Platform Path Separators

**Problem**: Windows uses `\`, Unix uses `/`.

**Solution**:
- Always use `/` in module paths (already done)
- Normalize paths before hashing

### 7. Source File Line Ending Differences

**Problem**: Git may checkout with CRLF on Windows, LF on Unix.

**Solution**:
- Normalize line endings before hashing
- Or: Trust that build system handles this (Git's `core.autocrlf`)

```rust
pub fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n")
}
```

---

## Performance Considerations

### Hashing Cost Analysis

| Operation | Count | Cost per Module | Total Cost (100 modules) |
|-----------|-------|-----------------|--------------------------|
| Text hash | 100 | ~0.1ms | ~10ms |
| Recursive hash | 100 | ~0.05ms | ~5ms |
| **Total** | | | **~15ms** |

Compare to parsing cost:
- Parsing 100 modules: ~500ms
- Hashing: ~15ms
- **Speedup: 33x** (when cached)

### Memory Overhead

```rust
struct ModuleGraphHash {
    config: 16 bytes
    modules: HashMap<String, ModuleHash>
        - 100 modules × (40 bytes path + 32 bytes hash) = 7.2 KB
    graph_hash: 8 bytes
}
// Total: ~7.5 KB per compilation
```

Negligible compared to AST size (~1MB for 100 modules).

### Scalability to Large Codebases

For 10,000 modules:
- Hashing time: ~1.5 seconds
- Memory: ~750 KB
- Still much faster than parsing (50+ seconds)

**Optimization**: Parallelize hash computation (modules are independent until recursive step).

---

## Migration Path

### Phase 1: Add Hash Infrastructure (No Behavior Change)

1. Add `module_hash.rs` with types
2. Compute hashes in pipeline (discard results)
3. Log hash values for inspection
4. **Goal**: Verify hash correctness

### Phase 2: Use Hash for Memoization

1. Add `parse_module_graph_with_hash`
2. Thread hash through pipeline
3. Enable salsa memoization
4. **Goal**: Functional incremental compilation

### Phase 3: Optimize Hash Computation

1. Parallelize hashing
2. Use cryptographic hash if needed
3. Add persistent cache (optional)
4. **Goal**: Production-ready performance

### Phase 4: Extend to Other Passes

1. Add hash input to `typecheck_module_graph`
2. Add hash input to `analyze_module_graph`
3. Hash-aware drop analysis
4. **Goal**: Full incremental pipeline

---

## Future Enhancements

### 1. Per-Function Incremental Compilation

Hash individual functions, not just modules.

```rust
pub struct FunctionHash {
    pub name: String,
    pub body_hash: u64,
    pub dependencies: Vec<String>,  // Functions called
}
```

### 2. Parallel Module Compilation

Use hashes to identify independent modules, compile in parallel.

### 3. Distributed Caching

Share hashes and compiled artifacts across machines (CI/CD).

```rust
pub trait HashCache {
    fn get(&self, hash: u64) -> Option<ParsedModule>;
    fn put(&mut self, hash: u64, module: ParsedModule);
}

pub struct RemoteHashCache {
    url: String,
}
```

### 4. Incremental Type Checking

Hash type signatures separately from implementations.

### 5. Watch Mode Optimization

Monitor filesystem, compute hashes on file change, trigger minimal recompilation.

---

## Summary and Recommendations

### ✅ Recommended Approach

**Salsa-tracked ModuleGraphHash** (Approach 3):
1. Compute content hash BEFORE creating salsa objects
2. Include: module text, dependencies, configuration
3. Use hash as salsa input to `parse_module_graph_with_hash`
4. Let salsa's memoization handle caching automatically

### 📋 Implementation Checklist

- [ ] Create `module_hash.rs` with hash types
- [ ] Add `ModuleAnalysisConfig` struct
- [ ] Implement `ModuleGraphHash::compute()`
- [ ] Add dependency extraction (parse imports without salsa)
- [ ] Update `ModuleCompilationPipeline::compile()` to compute hash
- [ ] Add `parse_module_graph_with_hash()` wrapper
- [ ] Write comprehensive tests (5 test cases above)
- [ ] Verify no regressions in existing tests
- [ ] Benchmark hashing overhead
- [ ] Document usage in README

### 🎯 Success Criteria

- [ ] Unchanged modules don't re-parse
- [ ] Changed modules trigger recomputation
- [ ] Transitive dependencies propagate correctly
- [ ] Configuration changes invalidate cache
- [ ] Hash computation adds <5% overhead
- [ ] All existing tests pass

### 📊 Expected Benefits

| Scenario | Current | With Hash | Speedup |
|----------|---------|-----------|---------|
| No changes | Parse all | Skip all | 100x |
| 1 module changed (of 100) | Parse all | Parse 1 + deps | 10-50x |
| Config changed | Parse all | Parse all | 1x |
| Fresh build | Parse all | Parse all | 1x |

---

## Questions for Further Discussion

1. **Hash Algorithm**: DefaultHasher (fast) vs SHA-256 (collision-resistant)?
2. **Persistent Cache**: Worth the complexity for cross-session benefits?
3. **Granularity**: Module-level (this design) vs function-level hashing?
4. **Configuration**: What other settings should be included in hash?
5. **Testing**: How to verify salsa memoization is actually working?

---

**Status**: Ready for implementation
**Next Steps**: Review this design, then proceed with Phase 1 implementation
