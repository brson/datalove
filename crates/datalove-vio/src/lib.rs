//! The file I/O interface datalove reads and writes through.
//!
//! Every file the compiler reads or writes goes through an [`Fs`]: package
//! libraries, manifests, scripts, worldfiles, saved sessions, and the outputs
//! a driver writes. A driver makes one, usually a [`HostFs`], and hands it
//! down; a test or a wasm build hands down a [`MemFs`] instead. Files built
//! into a binary, like the system library, are a [`StaticFs`].
//!
//! The native toolchain is not behind this. Building a rider, linking a
//! program and loading a shared library all hand files to another program,
//! which needs them at real paths, so those use the host's filesystem
//! directly. See `botdocs/plan-vio.md`.
//!
//! The method names and signatures follow `megaspace-fs` where the two have
//! the same operation, so that moving onto it later changes imports rather
//! than callers.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A filesystem shared between the parts of a driver that use it.
pub type DynFs = Arc<dyn Fs>;

/// What a path names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    File,
    Dir,
    /// Neither a file nor a directory: a device, a socket or a fifo.
    Other,
}

/// Filesystem operations.
///
/// A relative path is relative to [`current_dir`](Fs::current_dir).
/// Symbolic links are followed.
pub trait Fs: Send + Sync + fmt::Debug + 'static {
    /// The directory relative paths are taken from.
    fn current_dir(&self) -> &Path;

    /// What `path` names, or `None` if it names nothing.
    ///
    /// An error is a failure to find out, such as a denied permission, and
    /// not the absence of the path.
    fn kind(&self, path: &Path) -> io::Result<Option<EntryKind>>;

    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn read_to_string(&self, path: &Path) -> io::Result<String>;

    /// The entries of a directory, each as `path` joined with its name.
    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>>;

    /// Create or replace a file. Its directory must exist.
    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()>;

    fn create_dir_all(&self, path: &Path) -> io::Result<()>;

    /// Whether `path` is a file, as `Path::is_file` answers it.
    ///
    /// A path that cannot be looked at is not a file.
    fn is_file(&self, path: &Path) -> bool {
        matches!(self.kind(path), Ok(Some(EntryKind::File)))
    }

    /// Whether `path` is a directory, as `Path::is_dir` answers it.
    fn is_dir(&self, path: &Path) -> bool {
        matches!(self.kind(path), Ok(Some(EntryKind::Dir)))
    }
}

/// The host's filesystem, through `std::fs`.
///
/// Its working directory is its own, fixed when it is made, rather than the
/// process's. That way two of them can be at two places at once, and nothing
/// that changes the process's working directory later moves one.
#[derive(Debug)]
pub struct HostFs {
    cwd: PathBuf,
}

impl HostFs {
    /// The host's filesystem, at the process's working directory.
    pub fn new() -> io::Result<HostFs> {
        Ok(HostFs { cwd: std::env::current_dir()? })
    }

    /// The host's filesystem, at `cwd`.
    pub fn at(cwd: impl Into<PathBuf>) -> HostFs {
        HostFs { cwd: cwd.into() }
    }

    fn resolve(&self, path: &Path) -> PathBuf {
        self.cwd.join(path)
    }
}

impl Fs for HostFs {
    fn current_dir(&self) -> &Path {
        &self.cwd
    }

    fn kind(&self, path: &Path) -> io::Result<Option<EntryKind>> {
        match std::fs::metadata(self.resolve(path)) {
            Ok(meta) if meta.is_file() => Ok(Some(EntryKind::File)),
            Ok(meta) if meta.is_dir() => Ok(Some(EntryKind::Dir)),
            Ok(_) => Ok(Some(EntryKind::Other)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(self.resolve(path))
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(self.resolve(path))
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(self.resolve(path))? {
            entries.push(path.join(entry?.file_name()));
        }
        Ok(entries)
    }

    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        std::fs::write(self.resolve(path), data)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(self.resolve(path))
    }
}

/// A filesystem in memory.
///
/// Paths are taken apart lexically: `a/../b` is `b` whether or not `a` is
/// there, there being no links to make it otherwise. The root always exists.
#[derive(Debug)]
pub struct MemFs {
    cwd: PathBuf,
    tree: Mutex<Tree>,
}

#[derive(Debug, Default)]
struct Tree {
    dirs: BTreeSet<PathBuf>,
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl Tree {
    fn kind(&self, path: &Path) -> Option<EntryKind> {
        if self.dirs.contains(path) {
            Some(EntryKind::Dir)
        } else if self.files.contains_key(path) {
            Some(EntryKind::File)
        } else {
            None
        }
    }
}

impl MemFs {
    /// An empty filesystem, at the root.
    pub fn new() -> MemFs {
        MemFs::with_current_dir("/")
    }

    /// An empty filesystem at `cwd`, which is created.
    ///
    /// `cwd` is taken from the root.
    pub fn with_current_dir(cwd: impl AsRef<Path>) -> MemFs {
        let cwd = normalize(&Path::new("/").join(cwd));
        let fs = MemFs {
            cwd,
            tree: Mutex::new(Tree::default()),
        };
        {
            let mut tree = fs.tree.lock().unwrap();
            tree.dirs.extend(fs.cwd.ancestors().map(Path::to_path_buf));
        }
        fs
    }

    /// Write a file, creating any directories it is in.
    ///
    /// For laying out a filesystem to test against.
    pub fn add_file(&self, path: impl AsRef<Path>, data: impl AsRef<[u8]>) {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            self.create_dir_all(parent).expect("creating the directories of a file");
        }
        self.write(path, data.as_ref()).expect("writing a file");
    }

    fn resolve(&self, path: &Path) -> PathBuf {
        normalize(&self.cwd.join(path))
    }
}

impl Default for MemFs {
    fn default() -> MemFs {
        MemFs::new()
    }
}

/// An absolute path with its `.` and `..` taken out.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => out.push(component),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(name) => out.push(name),
        }
    }
    out
}

/// A read-only filesystem over files compiled into the binary.
///
/// Each file is named by its path from the root, as `sys/std/list.dfm`; the
/// directories are whatever those paths pass through. The working directory
/// is the root.
#[derive(Debug)]
pub struct StaticFs {
    files: &'static [(&'static str, &'static [u8])],
}

impl StaticFs {
    pub fn new(files: &'static [(&'static str, &'static [u8])]) -> StaticFs {
        StaticFs { files }
    }

    /// `path` from the root, as the files are named.
    fn resolve(path: &Path) -> PathBuf {
        let full = normalize(&Path::new("/").join(path));
        full.strip_prefix("/").expect("a path from the root").to_path_buf()
    }

    fn file(&self, path: &Path) -> Option<&'static [u8]> {
        self.files.iter()
            .find(|(name, _)| Path::new(name) == path)
            .map(|(_, data)| *data)
    }
}

impl Fs for StaticFs {
    fn current_dir(&self) -> &Path {
        Path::new("/")
    }

    fn kind(&self, path: &Path) -> io::Result<Option<EntryKind>> {
        let full = StaticFs::resolve(path);
        if self.file(&full).is_some() {
            return Ok(Some(EntryKind::File));
        }
        let is_dir = self.files.iter().any(|(name, _)| Path::new(name).starts_with(&full));
        Ok(is_dir.then_some(EntryKind::Dir))
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        match self.file(&StaticFs::resolve(path)) {
            Some(data) => Ok(data.to_vec()),
            None if self.is_dir(path) => Err(is_a_directory(path)),
            None => Err(not_found(path)),
        }
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        String::from_utf8(self.read(path)?)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        match self.kind(path)? {
            Some(EntryKind::Dir) => {}
            Some(_) => return Err(not_a_directory(path)),
            None => return Err(not_found(path)),
        }
        let full = StaticFs::resolve(path);
        let children = self.files.iter()
            .filter_map(|(name, _)| Path::new(name).strip_prefix(&full).ok())
            .filter_map(|rest| rest.components().next())
            .map(|child| path.join(child))
            .collect::<BTreeSet<_>>();
        Ok(children.into_iter().collect())
    }

    fn write(&self, path: &Path, _data: &[u8]) -> io::Result<()> {
        Err(read_only(path))
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        Err(read_only(path))
    }
}

fn read_only(path: &Path) -> io::Error {
    io::Error::new(io::ErrorKind::ReadOnlyFilesystem, format!("{}: read-only filesystem", path.display()))
}

fn not_found(path: &Path) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("{}: no such file or directory", path.display()))
}

fn is_a_directory(path: &Path) -> io::Error {
    io::Error::new(io::ErrorKind::IsADirectory, format!("{}: is a directory", path.display()))
}

fn not_a_directory(path: &Path) -> io::Error {
    io::Error::new(io::ErrorKind::NotADirectory, format!("{}: not a directory", path.display()))
}

impl Fs for MemFs {
    fn current_dir(&self) -> &Path {
        &self.cwd
    }

    fn kind(&self, path: &Path) -> io::Result<Option<EntryKind>> {
        Ok(self.tree.lock().unwrap().kind(&self.resolve(path)))
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let full = self.resolve(path);
        let tree = self.tree.lock().unwrap();
        match tree.kind(&full) {
            Some(EntryKind::File) => Ok(tree.files[&full].clone()),
            Some(_) => Err(is_a_directory(path)),
            None => Err(not_found(path)),
        }
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        String::from_utf8(self.read(path)?)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        let full = self.resolve(path);
        let tree = self.tree.lock().unwrap();
        match tree.kind(&full) {
            Some(EntryKind::Dir) => {}
            Some(_) => return Err(not_a_directory(path)),
            None => return Err(not_found(path)),
        }
        let children = tree.dirs.iter()
            .chain(tree.files.keys())
            .filter(|entry| entry.parent() == Some(full.as_path()))
            .map(|entry| path.join(entry.file_name().expect("a named entry")))
            .collect::<BTreeSet<_>>();
        Ok(children.into_iter().collect())
    }

    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        let full = self.resolve(path);
        let mut tree = self.tree.lock().unwrap();
        let Some(parent) = full.parent() else {
            return Err(is_a_directory(path));
        };
        match tree.kind(parent) {
            Some(EntryKind::Dir) => {}
            Some(_) => return Err(not_a_directory(path)),
            None => return Err(not_found(path)),
        }
        if tree.kind(&full) == Some(EntryKind::Dir) {
            return Err(is_a_directory(path));
        }
        tree.files.insert(full, data.to_vec());
        Ok(())
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        let full = self.resolve(path);
        let mut tree = self.tree.lock().unwrap();
        if let Some(file) = full.ancestors().find(|dir| tree.files.contains_key(*dir)) {
            return Err(not_a_directory(file));
        }
        tree.dirs.extend(full.ancestors().map(Path::to_path_buf));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two filesystems, laid out the same, for asking the same questions.
    fn both(f: impl Fn(&dyn Fs)) {
        let dir = tempfile::tempdir().unwrap();
        f(&HostFs::at(dir.path()));
        f(&MemFs::with_current_dir("/work"));
    }

    #[test]
    fn a_written_file_reads_back() {
        both(|fs| {
            fs.create_dir_all(Path::new("a/b")).unwrap();
            fs.write(Path::new("a/b/f.txt"), b"hello").unwrap();
            assert_eq!(fs.read_to_string(Path::new("a/b/f.txt")).unwrap(), "hello");
            assert_eq!(fs.read(Path::new("a/b/f.txt")).unwrap(), b"hello");
        });
    }

    #[test]
    fn a_write_replaces() {
        both(|fs| {
            fs.write(Path::new("f"), b"one").unwrap();
            fs.write(Path::new("f"), b"two").unwrap();
            assert_eq!(fs.read_to_string(Path::new("f")).unwrap(), "two");
        });
    }

    #[test]
    fn kinds() {
        both(|fs| {
            fs.create_dir_all(Path::new("d")).unwrap();
            fs.write(Path::new("d/f"), b"").unwrap();
            assert_eq!(fs.kind(Path::new("d")).unwrap(), Some(EntryKind::Dir));
            assert_eq!(fs.kind(Path::new("d/f")).unwrap(), Some(EntryKind::File));
            assert_eq!(fs.kind(Path::new("d/g")).unwrap(), None);
            assert!(fs.is_dir(Path::new("d")) && !fs.is_file(Path::new("d")));
            assert!(fs.is_file(Path::new("d/f")) && !fs.is_dir(Path::new("d/f")));
            assert!(!fs.is_file(Path::new("d/g")) && !fs.is_dir(Path::new("d/g")));
        });
    }

    #[test]
    fn a_listing_joins_names_to_the_path_given() {
        both(|fs| {
            fs.create_dir_all(Path::new("d/sub")).unwrap();
            fs.write(Path::new("d/b"), b"").unwrap();
            fs.write(Path::new("d/a"), b"").unwrap();
            fs.write(Path::new("d/sub/deeper"), b"").unwrap();
            let mut entries = fs.list_dir(Path::new("d")).unwrap();
            entries.sort();
            assert_eq!(entries, [Path::new("d/a"), Path::new("d/b"), Path::new("d/sub")]);
        });
    }

    #[test]
    fn relative_and_absolute_paths_meet() {
        both(|fs| {
            fs.write(Path::new("f"), b"x").unwrap();
            let absolute = fs.current_dir().join("f");
            assert_eq!(fs.read_to_string(&absolute).unwrap(), "x");
        });
    }

    #[test]
    fn writing_into_a_missing_directory_fails() {
        both(|fs| {
            let e = fs.write(Path::new("nowhere/f"), b"").unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::NotFound);
        });
    }

    #[test]
    fn reading_what_is_not_there_fails() {
        both(|fs| {
            let e = fs.read_to_string(Path::new("missing")).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::NotFound);
            let e = fs.list_dir(Path::new("missing")).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::NotFound);
        });
    }

    #[test]
    fn a_file_is_not_a_directory() {
        both(|fs| {
            fs.write(Path::new("f"), b"").unwrap();
            assert!(fs.list_dir(Path::new("f")).is_err());
            assert!(fs.create_dir_all(Path::new("f/g")).is_err());
            assert!(fs.write(Path::new("f/g"), b"").is_err());
        });
    }

    #[test]
    fn a_directory_is_not_a_file() {
        both(|fs| {
            fs.create_dir_all(Path::new("d")).unwrap();
            assert!(fs.read(Path::new("d")).is_err());
            assert!(fs.write(Path::new("d"), b"").is_err());
        });
    }

    #[test]
    fn invalid_utf8_is_invalid_data() {
        both(|fs| {
            fs.write(Path::new("f"), &[0xff, 0xfe]).unwrap();
            let e = fs.read_to_string(Path::new("f")).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        });
    }

    static FILES: &[(&str, &[u8])] = &[
        ("lib/pkg/a.dfm", b"a"),
        ("lib/pkg/b.dfm", b"b"),
        ("lib/pkg/sub/c.dli", b"c"),
        ("top", b"t"),
    ];

    #[test]
    fn static_files_read_and_list() {
        let fs = StaticFs::new(FILES);
        assert_eq!(fs.read_to_string(Path::new("lib/pkg/a.dfm")).unwrap(), "a");
        assert_eq!(fs.read_to_string(Path::new("/lib/./pkg/sub/../b.dfm")).unwrap(), "b");
        assert_eq!(
            fs.list_dir(Path::new("lib/pkg")).unwrap(),
            [Path::new("lib/pkg/a.dfm"), Path::new("lib/pkg/b.dfm"), Path::new("lib/pkg/sub")],
        );
        assert_eq!(fs.list_dir(Path::new("/")).unwrap(), [Path::new("/lib"), Path::new("/top")]);
    }

    #[test]
    fn static_directories_are_what_the_paths_pass_through() {
        let fs = StaticFs::new(FILES);
        assert_eq!(fs.kind(Path::new("lib")).unwrap(), Some(EntryKind::Dir));
        assert_eq!(fs.kind(Path::new("lib/pkg/sub")).unwrap(), Some(EntryKind::Dir));
        assert_eq!(fs.kind(Path::new("/")).unwrap(), Some(EntryKind::Dir));
        assert_eq!(fs.kind(Path::new("top")).unwrap(), Some(EntryKind::File));
        // A prefix of a name is not a directory.
        assert_eq!(fs.kind(Path::new("lib/pk")).unwrap(), None);
        assert!(fs.list_dir(Path::new("top")).is_err());
        assert_eq!(fs.read(Path::new("lib")).unwrap_err().kind(), io::ErrorKind::IsADirectory);
        assert_eq!(fs.read(Path::new("nope")).unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn static_files_are_read_only() {
        let fs = StaticFs::new(FILES);
        assert_eq!(fs.write(Path::new("top"), b"").unwrap_err().kind(), io::ErrorKind::ReadOnlyFilesystem);
        assert_eq!(fs.create_dir_all(Path::new("d")).unwrap_err().kind(), io::ErrorKind::ReadOnlyFilesystem);
    }

    #[test]
    fn mem_paths_are_taken_apart_lexically() {
        let fs = MemFs::with_current_dir("/work/here");
        fs.add_file("../there/f", "x");
        assert_eq!(fs.read_to_string(Path::new("/work/there/f")).unwrap(), "x");
        assert_eq!(fs.read_to_string(Path::new("/work/./there/../there/f")).unwrap(), "x");
        assert!(fs.is_dir(Path::new("/")));
    }
}
