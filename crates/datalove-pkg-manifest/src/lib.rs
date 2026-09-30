//! A datalove package's `manifest.toml`.
//!
//! Modules are still found by convention --- every `.dfm` in the package
//! directory --- and always will be, there being nothing to say about them
//! that looking cannot answer. A rider is different: it is a Rust crate with a
//! name and a version, and a packaged datalove package has had the Rust
//! stripped out of it, so the name and version have to be written down
//! somewhere that survives. That is what this is for.
//!
//! Expect it to grow. Unknown keys are refused rather than ignored, so a
//! manifest written for a later datalove says so instead of half working.

use std::path::Path;

use serde::Deserialize;

/// The file name, in the package's own directory.
pub const FILE_NAME: &str = "manifest.toml";

/// What a package's manifest says about it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The rider this package declares, if it has one.
    pub rider: Option<RiderManifest>,
}

/// The Rust crate implementing a package's native functions.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiderManifest {
    /// The crate's name, as crates.io knows it.
    pub name: String,
    /// The version to build against.
    ///
    /// Used exactly, not as a requirement: a rider and the runtime loading it
    /// have to agree about the layout of everything that crosses between
    /// them, which a range does not promise.
    pub version: String,
}

/// Read a package's manifest, if it has one.
///
/// A package with no rider needs no manifest and may have none. A package
/// with a rider must say what it is named, since a packaged one has nothing
/// else left that would.
pub fn load(dir: &Path, has_rider: bool) -> Result<Option<Manifest>, ManifestError> {
    let path = dir.join(FILE_NAME);

    if !path.is_file() {
        if has_rider {
            return Err(ManifestError::Missing { package: dir.to_path_buf() });
        }
        return Ok(None);
    }

    let text = std::fs::read_to_string(&path)
        .map_err(|source| ManifestError::Unreadable { path: path.clone(), source })?;
    let manifest: Manifest = toml::from_str(&text)
        .map_err(|source| ManifestError::Malformed { path: path.clone(), source })?;

    if has_rider && manifest.rider.is_none() {
        return Err(ManifestError::NoRiderSection { package: dir.to_path_buf() });
    }

    Ok(Some(manifest))
}

/// What can be wrong with a package's manifest.
#[derive(Debug)]
pub enum ManifestError {
    /// A package with a rider and no manifest to name it.
    Missing { package: std::path::PathBuf },
    /// The file is there and could not be read.
    Unreadable { path: std::path::PathBuf, source: std::io::Error },
    /// The file is there and is not the manifest it should be.
    ///
    /// Unknown keys land here, being refused rather than ignored, so a
    /// manifest written for a later datalove says so plainly.
    Malformed { path: std::path::PathBuf, source: toml::de::Error },
    /// A manifest that does not mention the rider beside it.
    NoRiderSection { package: std::path::PathBuf },
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestError::Missing { package } => write!(
                f,
                "package {} declares a rider but has no {}; the rider's crate \
                 name and version have to be written there, a packaged package \
                 having no Cargo.toml left to read them from",
                package.display(), FILE_NAME,
            ),
            ManifestError::Unreadable { path, source } => {
                write!(f, "unable to read {}: {}", path.display(), source)
            }
            ManifestError::Malformed { path, source } => {
                write!(f, "unable to parse {}: {}", path.display(), source)
            }
            ManifestError::NoRiderSection { package } => write!(
                f,
                "package {} has a rider in rider/ but its {} does not declare \
                 one; add a [rider] section naming the crate and version",
                package.display(), FILE_NAME,
            ),
        }
    }
}

impl std::error::Error for ManifestError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A package directory, with a manifest and a rider if asked for.
    fn package(manifest: Option<&str>, rider: bool) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a temp dir");
        if let Some(text) = manifest {
            std::fs::write(dir.path().join(FILE_NAME), text).expect("writing the manifest");
        }
        if rider {
            let rider_dir = dir.path().join("rider");
            std::fs::create_dir(&rider_dir).expect("creating rider/");
            std::fs::write(rider_dir.join("rider.dli"), "").expect("writing the interface");
        }
        dir
    }

    #[test]
    fn a_rider_is_named() {
        let dir = package(Some("[rider]\nname = \"a-rider\"\nversion = \"0.1.0\"\n"), true);
        let rider = load(dir.path(), true).expect("it loads")
            .expect("a manifest").rider.expect("a rider");
        assert_eq!(rider.name, "a-rider");
        assert_eq!(rider.version, "0.1.0");
    }

    /// The point of the file: a packaged package has no Cargo.toml left, so
    /// nothing else would say what to build.
    #[test]
    fn a_rider_without_a_manifest_is_refused() {
        let dir = package(None, true);
        let error = load(dir.path(), true).expect_err("a rider needs a manifest");
        assert!(matches!(error, ManifestError::Missing { .. }), "got {error:?}");
    }

    #[test]
    fn a_rider_a_manifest_does_not_mention_is_refused() {
        let dir = package(Some("# nothing here\n"), true);
        let error = load(dir.path(), true).expect_err("the [rider] section is required");
        assert!(matches!(error, ManifestError::NoRiderSection { .. }), "got {error:?}");
    }

    /// Refused rather than ignored, so a manifest written for a later
    /// datalove says so instead of half working.
    #[test]
    fn an_unknown_key_is_refused() {
        let dir = package(
            Some("[rider]\nname = \"a\"\nversion = \"0.1.0\"\nfrom_the_future = true\n"), true);
        let error = load(dir.path(), true).expect_err("unknown keys are refused");
        assert!(matches!(error, ManifestError::Malformed { .. }), "got {error:?}");
    }

    /// A package with no rider has nothing to declare and need not.
    #[test]
    fn no_rider_needs_no_manifest() {
        let dir = package(None, false);
        assert!(load(dir.path(), false).expect("it loads").is_none());
    }
}
