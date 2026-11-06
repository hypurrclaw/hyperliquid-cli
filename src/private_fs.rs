//! Owner-only file helpers: private directories (0700), private files (0600),
//! atomic replacement and advisory locks.
//!
//! Every place that writes secrets or state next to secrets goes through here so
//! the Unix mode handling exists once.

use std::fs::{DirBuilder, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const DIR_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;
const LOCK_POLL: Duration = Duration::from_millis(25);
const TEMP_NAME_ATTEMPTS: u32 = 16;

/// `OpenOptions` that create files with `mode` on Unix.
#[must_use]
pub fn open_options_with_mode(mode: u32) -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    options
}

fn dir_builder(recursive: bool) -> DirBuilder {
    let mut builder = DirBuilder::new();
    builder.recursive(recursive);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(DIR_MODE);
    }
    builder
}

/// Create `dir` and any missing parents; new directories are owner-only (0700).
pub fn create_dir_all(dir: &Path) -> std::io::Result<()> {
    dir_builder(true).create(dir)
}

/// Create exactly one new owner-only directory; fails if it already exists.
pub fn create_new_dir(dir: &Path) -> std::io::Result<()> {
    dir_builder(false).create(dir)
}

/// Replace `path` with `contents` atomically and owner-only.
///
/// The bytes go to a temp file created with `create_new` and mode 0600 in the
/// target directory, are fsynced, then renamed over `path`. A crash leaves
/// either the old file or the new one, never a truncated file. Missing parent
/// directories are created with mode 0700.
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());

    let mut attempt = 0_u32;
    let (tmp_path, mut tmp) = loop {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.subsec_nanos());
        let tmp_path = parent.join(format!(
            ".{file_name}.{}.{nanos}.{attempt}.tmp",
            std::process::id()
        ));
        match open_options_with_mode(FILE_MODE)
            .write(true)
            .create_new(true)
            .open(&tmp_path)
        {
            Ok(file) => break (tmp_path, file),
            Err(err)
                if err.kind() == std::io::ErrorKind::AlreadyExists
                    && attempt < TEMP_NAME_ATTEMPTS =>
            {
                attempt += 1;
            }
            Err(err) => return Err(err),
        }
    };

    let written = tmp
        .write_all(contents)
        .and_then(|()| tmp.sync_all())
        .and_then(|()| std::fs::rename(&tmp_path, path));
    if let Err(err) = written {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }
    #[cfg(unix)]
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Why [`FileLock::acquire`] failed.
#[derive(Debug)]
pub enum LockError {
    Io(std::io::Error),
    /// Another process still held the lock after the wait.
    Busy,
}

/// Advisory lock on `<path>.lock`, released when dropped.
#[derive(Debug)]
pub struct FileLock {
    _file: File,
}

impl FileLock {
    /// Lock the sibling `<path>.lock` file, waiting up to `wait` for another process.
    pub fn acquire(path: &Path, wait: Duration) -> Result<Self, LockError> {
        let mut lock_name = path.as_os_str().to_os_string();
        lock_name.push(".lock");
        let lock_path = PathBuf::from(lock_name);
        if let Some(parent) = lock_path.parent() {
            create_dir_all(parent).map_err(LockError::Io)?;
        }
        let file = open_options_with_mode(FILE_MODE)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(LockError::Io)?;

        let deadline = Instant::now() + wait;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(LOCK_POLL);
                }
                Err(std::fs::TryLockError::WouldBlock) => return Err(LockError::Busy),
                Err(std::fs::TryLockError::Error(err)) => return Err(LockError::Io(err)),
            }
        }
    }
}

/// The mode of `path` when group or others can access it.
#[cfg(unix)]
#[must_use]
pub fn loose_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path).ok()?.permissions().mode() & 0o777;
    (mode & 0o077 != 0).then_some(mode)
}

#[cfg(not(unix))]
#[must_use]
pub fn loose_mode(_path: &Path) -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_replaces_contents_and_leaves_no_temp_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("nested").join("state.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(leftovers.len(), 1, "unexpected files: {leftovers:?}");
    }

    #[cfg(unix)]
    #[test]
    fn files_and_directories_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("nested").join("state.json");
        write_atomic(&path, b"secret").unwrap();
        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let dir_mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
        assert_eq!(dir_mode, 0o700);
        assert_eq!(loose_mode(&path), None);
    }

    #[test]
    fn lock_is_exclusive_until_dropped() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("config.json");
        let first = FileLock::acquire(&path, Duration::ZERO).unwrap();
        assert!(matches!(
            FileLock::acquire(&path, Duration::ZERO),
            Err(LockError::Busy)
        ));
        drop(first);
        FileLock::acquire(&path, Duration::ZERO).unwrap();
    }
}
