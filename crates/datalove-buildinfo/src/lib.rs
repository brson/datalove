//! Where this datalove's own sources came from.
//!
//! Executing datalove needs three things that are not datalove: the runtime,
//! the riders a package declares, and `sys/`. The compiler builds a native
//! component out of the first two with cargo, which means naming them as cargo
//! dependencies, and how to name them has exactly two answers:
//!
//! - built from a checkout, so they are beside us and named by path;
//! - built from a registry, so they are published and named by version.
//!
//! Nothing at run time distinguishes the two. An installed binary has no way
//! to look at itself and say which it is, and guessing wrong is a cargo error
//! about a path that does not exist. So the build script decides, while the
//! answer is still in front of it, and bakes it in here.

use std::path::Path;

/// Where this binary's sources came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildInfo {
    /// Built from a published release. Dependencies come from a registry.
    Prod {
        /// This datalove's version, which the crates it needs share.
        version: &'static str,
    },
    /// Built from a checkout. Dependencies are beside us.
    Local {
        /// What was checked out, when git could be asked.
        ///
        /// For fetching the same sources again once the checkout is gone.
        /// Nothing reads it yet, and it is as of whenever
        /// `datalove-buildinfo` last compiled rather than of this moment ---
        /// watching git would relink the tree on every commit.
        git_sha: Option<&'static str>,
        /// The root of the tree this was built from.
        ///
        /// Absolute, and not necessarily still there: a checkout can be moved
        /// or deleted, and `cargo install --git` builds from a clone it
        /// removes straight afterwards.
        checkout: &'static str,
    },
}

// `BUILD_INFO`, decided by `build.rs`.
include!(concat!(env!("OUT_DIR"), "/build_info.rs"));

impl BuildInfo {
    /// The tree this was built from, if it was built from one.
    ///
    /// Says nothing about whether it is still there; see
    /// [`checkout_exists`](Self::checkout_exists).
    pub fn checkout(&self) -> Option<&'static Path> {
        match self {
            BuildInfo::Prod { .. } => None,
            BuildInfo::Local { checkout, .. } => Some(Path::new(*checkout)),
        }
    }

    /// Whether the tree this was built from is where it was.
    pub fn checkout_exists(&self) -> bool {
        self.checkout().is_some_and(|dir| dir.is_dir())
    }

    /// One line saying which kind of build this is, for `--version`.
    ///
    /// A binary that cannot say where it expects to find its sources leaves
    /// whoever is holding it guessing when they turn out not to be there.
    pub fn describe(&self) -> String {
        match self {
            BuildInfo::Prod { version } => format!("{version} (release)"),
            BuildInfo::Local { git_sha, checkout } => {
                let revision = git_sha.unwrap_or("revision unknown");
                let gone = if Path::new(checkout).is_dir() { "" } else { ", missing" };
                format!("{revision} ({checkout}{gone})")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A checkout that has been moved or deleted is reported as such.
    ///
    /// This is the case a user actually hits --- `cargo install --path` and
    /// then tidying the checkout away --- and the one where saying nothing
    /// leaves them with a cargo error about a path and no reason for it.
    #[test]
    fn a_missing_checkout_says_so() {
        let info = BuildInfo::Local {
            git_sha: Some("deadbeef"),
            checkout: "/nowhere/this/tree/went",
        };
        assert!(!info.checkout_exists());
        assert!(info.describe().contains("missing"), "{}", info.describe());
    }

    #[test]
    fn a_checkout_that_is_there_is_not_called_missing() {
        let info = BuildInfo::Local { git_sha: None, checkout: "/" };
        assert!(info.checkout_exists());
        assert!(!info.describe().contains("missing"), "{}", info.describe());
        // Nothing to say about the revision is said plainly, not as a blank.
        assert!(info.describe().contains("unknown"), "{}", info.describe());
    }

    /// A release build has no tree, which is not the same as having a missing
    /// one: nothing has gone wrong and there is nothing to put back.
    #[test]
    fn a_release_has_no_checkout() {
        let info = BuildInfo::Prod { version: "0.1.0" };
        assert!(info.checkout().is_none());
        assert!(!info.checkout_exists());
        assert!(info.describe().contains("release"), "{}", info.describe());
    }

    /// The build script decided something about this very build, and the two
    /// halves of what it decided agree.
    #[test]
    fn this_build_knows_what_it_is() {
        match BUILD_INFO {
            BuildInfo::Local { checkout, .. } => {
                assert!(BUILD_INFO.checkout() == Some(Path::new(checkout)));
            }
            BuildInfo::Prod { .. } => assert!(BUILD_INFO.checkout().is_none()),
        }
    }
}
