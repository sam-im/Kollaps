//! Implements facilities for creating various temporary files and log files.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::Result;
use tracing::warn;

/// Represents modifications made to the filesystem.
/// Dropping this struct performs the required clean-up.
/// Use `FilesBuilder` to create this struct.
pub struct Files {
    files: Vec<File>,
}

impl Files {
    fn try_new(mut files: Vec<File>) -> Result<Self> {
        for file in files.iter_mut() {
            file.create()?;
        }
        Ok(Self { files })
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        while let Some(file) = self.files.pop() {
            let _ = file.remove();
        }
    }
}

/// Builder to configure and create a `Files` instance.
pub struct FilesBuilder {
    files: Vec<File>,
}

impl FilesBuilder {
    /// Returns a struct that can configure and initialize a `Files`.
    /// One can chain the various accessory methods (add_dir, add_temp_file, ...) to create
    /// various filesystem modifications with different clean up strategies, see `File::remove`.
    ///
    /// Use `try_build` to perform these operations on the filesystem.
    /// Dropping the `Files` instance performs clean-up.
    pub fn new() -> Self {
        Self { files: vec![] }
    }
    /// Try and build a `Files` instance.
    pub fn try_build(self) -> Result<Files> {
        Files::try_new(self.files)
    }
    /// Add a to-be-created directory that will not be removed at cleanup.
    pub fn add_dir(mut self, path: &Path, perms: Option<u32>) -> Self {
        let dir = File::Directory(path.to_path_buf(), perms);
        self.files.push(dir);
        self
    }
    /// Add a to-be-created directory that will be recursively deleted at cleanup.
    pub fn add_temp_dir(mut self, path: &Path, perms: Option<u32>) -> Self {
        let dir = File::TempDirectory(path.to_path_buf(), perms);
        self.files.push(dir);
        self
    }
    /// Add a to-be-created text file that will be removed at cleanup.
    pub fn add_temp_file(mut self, path: &Path, content: String) -> Self {
        let file = File::TempRegular(path.to_path_buf(), content);
        self.files.push(file);
        self
    }
    /// Replaces the content of an existing file with `content`, and restores the original
    /// content at cleanup.
    pub fn add_protected_file(mut self, path: &Path, content: String) -> Self {
        let file = File::ProtectedRegular(path.to_path_buf(), content, String::new());
        self.files.push(file);
        self
    }
}

enum File {
    /// Text file, removed when dropped.
    TempRegular(PathBuf, String),
    /// Text file, rewritten with the initial content when dropped.
    ProtectedRegular(PathBuf, String, String),
    /// Directory with optional permissions, removed when dropped.
    TempDirectory(PathBuf, Option<u32>),
    /// Directory with optional permissions, left behind.
    Directory(PathBuf, Option<u32>),
}

impl File {
    fn create(&mut self) -> Result<()> {
        match self {
            File::TempRegular(path, content) => {
                fs::write(path, content)?;
            }
            File::TempDirectory(path, perms) => {
                if let Err(e) = fs::create_dir(&path) {
                    warn!(
                        "Failed to create directory {}: {}",
                        path.to_string_lossy(),
                        e.kind()
                    );
                }
                if let Some(perms) = perms {
                    fs::set_permissions(path, fs::Permissions::from_mode(*perms))?;
                }
            }
            File::ProtectedRegular(path, content, prev_content) => {
                prev_content.clear();
                prev_content.push_str(fs::read_to_string(&path)?.as_ref());
                fs::write(path, content)?;
            }
            File::Directory(path, perms) => {
                if let Err(e) = fs::create_dir(&path) {
                    warn!(
                        "Failed to create directory {}: {}",
                        path.to_string_lossy(),
                        e.kind()
                    );
                }
                if let Some(perms) = perms {
                    fs::set_permissions(path, fs::Permissions::from_mode(*perms))?;
                }
            }
        }
        Ok(())
    }
    fn remove(&self) -> Result<()> {
        match self {
            File::TempRegular(path, _) => {
                fs::remove_file(path)?;
            }
            File::TempDirectory(path, _) => {
                // HACK: failsafe against bad input in `path`
                if path.starts_with("/tmp/kollaps") {
                    for entry in fs::read_dir(path)? {
                        fs::remove_file(entry?.path())?;
                    }
                    fs::remove_dir(path)?;
                }
            }
            File::ProtectedRegular(path, _, prev_content) => {
                fs::write(path, prev_content)?;
            }
            File::Directory(_, _) => (),
        }
        Ok(())
    }
}
