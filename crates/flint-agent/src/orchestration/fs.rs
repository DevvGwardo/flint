//! Descriptor-relative, no-follow private storage and an OS-backed writer lease.
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::path::Path;

pub(crate) struct Directory(File);

fn name(value: &str) -> io::Result<CString> {
    if value.is_empty() || value.contains(['/', '\0']) || matches!(value, "." | "..") {
        return Err(io::Error::other("Invalid storage component."));
    }
    CString::new(value).map_err(io::Error::other)
}

impl Directory {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        Ok(Self(file))
    }

    pub(crate) fn child(&self, component: &str) -> io::Result<Self> {
        let component = name(component)?;
        // All names are a single validated component, anchored to the open directory.
        let result = unsafe { libc::mkdirat(self.0.as_raw_fd(), component.as_ptr(), 0o700) };
        if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::AlreadyExists {
            return Err(io::Error::last_os_error());
        }
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                component.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let directory = Self(unsafe { File::from_raw_fd(fd) });
        self.0.sync_all()?;
        Ok(directory)
    }

    /// Opens an existing child without creating it or syncing directories.
    pub(crate) fn open_child(&self, component: &str) -> io::Result<Self> {
        let component = name(component)?;
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                component.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(unsafe { File::from_raw_fd(fd) }))
    }

    fn file(&self, component: &str, flags: i32) -> io::Result<File> {
        let component = name(component)?;
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                component.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("Storage object is not a regular file."));
        }
        Ok(file)
    }

    pub(crate) fn read(&self, component: &str, limit: usize) -> io::Result<Vec<u8>> {
        let file = self.reader(component, limit)?;
        let capacity = (file.metadata()?.len() as usize).min(limit);
        let mut bytes = Vec::with_capacity(capacity);
        file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(io::Error::other("Storage object exceeds its read limit."));
        }
        Ok(bytes)
    }

    pub(crate) fn reader(&self, component: &str, limit: usize) -> io::Result<File> {
        let file = self.file(component, libc::O_RDONLY)?;
        if file.metadata()?.len() > limit as u64 {
            return Err(io::Error::other("Storage object exceeds its read limit."));
        }
        Ok(file)
    }

    pub(crate) fn read_optional(
        &self,
        component: &str,
        limit: usize,
    ) -> io::Result<Option<Vec<u8>>> {
        match self.read(component, limit) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn replace(&self, component: &str, bytes: &[u8]) -> io::Result<()> {
        self.replace_with(component, bytes, || Ok(()), || Ok(()))
    }

    pub(crate) fn replace_with(
        &self,
        component: &str,
        bytes: &[u8],
        before_rename: impl FnOnce() -> io::Result<()>,
        after_rename: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        // Refuse a symlink/FIFO/directory destination even though rename itself
        // would replace it. Never follow it, and never truncate the old inode.
        match self.reader(component, super::store::MAX_SNAPSHOT_BYTES) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let temporary = format!(".flint-{}.tmp", uuid::Uuid::new_v4());
        let mut file = self.file(&temporary, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)?;
        let result = (|| {
            file.write_all(bytes)?;
            file.sync_all()?;
            before_rename()?;
            let from = name(&temporary)?;
            let to = name(component)?;
            let result = unsafe {
                libc::renameat(
                    self.0.as_raw_fd(),
                    from.as_ptr(),
                    self.0.as_raw_fd(),
                    to.as_ptr(),
                )
            };
            if result < 0 {
                return Err(io::Error::last_os_error());
            }
            after_rename()?;
            self.0.sync_all()
        })();
        // Only our own O_EXCL-created temporary may be removed.
        let temporary = name(&temporary)?;
        unsafe { libc::unlinkat(self.0.as_raw_fd(), temporary.as_ptr(), 0) };
        result
    }

    pub(crate) fn immutable(&self, component: &str, bytes: &[u8]) -> io::Result<()> {
        if let Some(existing) = self.read_optional(component, super::store::MAX_COMMAND_BYTES)? {
            return if existing == bytes {
                // A prior publication may have failed its directory fsync.
                // Reusing that orphan must complete both durability barriers.
                self.reader(component, super::store::MAX_COMMAND_BYTES)?
                    .sync_all()?;
                self.0.sync_all()
            } else {
                Err(io::Error::other("Immutable storage identity conflict."))
            };
        }
        let temporary = format!(".flint-{}.tmp", uuid::Uuid::new_v4());
        let mut file = self.file(&temporary, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)?;
        let result = (|| {
            file.write_all(bytes)?;
            file.sync_all()?;
            let from = name(&temporary)?;
            let to = name(component)?;
            let result = unsafe {
                libc::linkat(
                    self.0.as_raw_fd(),
                    from.as_ptr(),
                    self.0.as_raw_fd(),
                    to.as_ptr(),
                    0,
                )
            };
            if result < 0 {
                return Err(io::Error::last_os_error());
            }
            self.0.sync_all()
        })();
        let temporary = name(&temporary)?;
        unsafe { libc::unlinkat(self.0.as_raw_fd(), temporary.as_ptr(), 0) };
        result
    }
}

pub(crate) struct WriterLease {
    pub(crate) directory: Directory,
    _lock: File,
}

impl WriterLease {
    pub(crate) fn acquire(path: &Path) -> io::Result<Self> {
        // The leaf is never a symlink. Parent directories are caller-owned;
        // all subsequent operations stay anchored to this descriptor.
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return Err(io::Error::other(
                    "Session directory is not a real directory.",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new()
                    .mode(0o700)
                    .recursive(true)
                    .create(path)?;
            }
            Err(error) => return Err(error),
        }
        let directory = Directory::open(path)?;
        let lock = directory.file(".flint-writer.lock", libc::O_RDWR | libc::O_CREAT)?;
        loop {
            if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                break;
            }
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if error.kind() == io::ErrorKind::WouldBlock {
                return Err(io::Error::other("Another writer owns this session."));
            }
            return Err(error);
        }
        Ok(Self {
            directory,
            _lock: lock,
        })
    }
}

impl Drop for WriterLease {
    fn drop(&mut self) {
        // Explicitly unlock before close, including when a concurrently
        // spawning child temporarily inherited this descriptor before exec.
        loop {
            if unsafe { libc::flock(self._lock.as_raw_fd(), libc::LOCK_UN) } == 0
                || io::Error::last_os_error().kind() != io::ErrorKind::Interrupted
            {
                break;
            }
        }
    }
}
