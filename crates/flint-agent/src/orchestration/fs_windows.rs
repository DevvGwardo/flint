//! Handle-relative Windows storage. Reparse points never become directories or
//! files; Job/child processes cannot inherit handles, and byte locks own writers.
//! File contents are flushed before atomic publication. Windows has no supported
//! equivalent of Unix directory fsync, so power-loss durability is OS-dependent.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::mem::{offset_of, size_of};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::*;
use windows_sys::Win32::Foundation::{
    HANDLE, LocalFree, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, RtlNtStatusToDosError,
    UNICODE_STRING,
};
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::IO::{IO_STATUS_BLOCK, OVERLAPPED};

pub(crate) struct Directory(File);

fn name(component: &str) -> io::Result<Vec<u16>> {
    let stem = component
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        });
    if component.is_empty()
        || component.contains(['/', '\\', ':', '\0', '<', '>', '"', '|', '?', '*'])
        || component.chars().any(|c| c.is_control())
        || component.ends_with(['.', ' '])
        || matches!(component, "." | "..")
        || reserved
    {
        return Err(io::Error::other("Invalid storage component."));
    }
    let name: Vec<u16> = component.encode_utf16().collect();
    if name.len() > (u16::MAX as usize / 2) {
        return Err(io::Error::other("Storage component is too long."));
    }
    Ok(name)
}

fn nt_result(status: i32) -> io::Result<()> {
    if status < 0 {
        Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        ))
    } else {
        Ok(())
    }
}

/// Protected owner/SYSTEM ACLs, including children of newly created directories.
struct PrivateSecurity(PSECURITY_DESCRIPTOR);

impl PrivateSecurity {
    fn new() -> io::Result<Self> {
        let sddl: Vec<u16> = "D:P(A;OICI;FA;;;OW)(A;OICI;FA;;;SY)\0"
            .encode_utf16()
            .collect();
        let mut descriptor = null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(descriptor))
    }
}

impl Drop for PrivateSecurity {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

impl Directory {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        // Not sharing DELETE pins the actual directory for this handle's life.
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::other(
                "Session directory is not a real directory.",
            ));
        }
        Ok(Self(file))
    }

    fn open_relative(
        &self,
        component: &str,
        access: u32,
        disposition: u32,
        directory: bool,
    ) -> io::Result<File> {
        let mut name = name(component)?;
        let unicode = UNICODE_STRING {
            Length: (name.len() * 2) as u16,
            MaximumLength: (name.len() * 2) as u16,
            Buffer: name.as_mut_ptr(),
        };
        let security = (disposition != FILE_OPEN)
            .then(PrivateSecurity::new)
            .transpose()?;
        let attributes = OBJECT_ATTRIBUTES {
            Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: self.0.as_raw_handle(),
            ObjectName: &unicode as *const _ as *mut _,
            Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
            SecurityDescriptor: security.as_ref().map_or(null(), |s| s.0.cast()),
            SecurityQualityOfService: null_mut(),
        };
        let mut handle: HANDLE = null_mut();
        let mut status = IO_STATUS_BLOCK::default();
        let kind = if directory {
            FILE_DIRECTORY_FILE
        } else {
            FILE_NON_DIRECTORY_FILE
        };
        // Pin the writer-lock name too: removing a locked pathname must not
        // create a new lock inode and admit a second writer.
        let sharing = FILE_SHARE_READ
            | FILE_SHARE_WRITE
            | if directory || component == ".flint-writer.lock" {
                0
            } else {
                FILE_SHARE_DELETE
            };
        nt_result(unsafe {
            NtCreateFile(
                &mut handle,
                access | SYNCHRONIZE,
                &attributes,
                &mut status,
                null(),
                FILE_ATTRIBUTE_NORMAL,
                sharing,
                disposition,
                kind | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
                null(),
                0,
            )
        })?;
        // The handle is synchronous, owned, and not inheritable (OBJ_INHERIT
        // was deliberately omitted). File drops it on every error path.
        let file = unsafe { File::from_raw_handle(handle) };
        let metadata = file.metadata()?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || if directory {
                !metadata.is_dir()
            } else {
                !metadata.is_file()
            }
        {
            return Err(io::Error::other(
                "Storage object is not a real directory or regular file.",
            ));
        }
        Ok(file)
    }

    pub(crate) fn child(&self, component: &str) -> io::Result<Self> {
        self.open_relative(component, FILE_GENERIC_READ, FILE_OPEN_IF, true)
            .map(Self)
    }

    pub(crate) fn open_child(&self, component: &str) -> io::Result<Self> {
        self.open_relative(component, FILE_GENERIC_READ, FILE_OPEN, true)
            .map(Self)
    }

    pub(crate) fn reader(&self, component: &str, limit: usize) -> io::Result<File> {
        let file = self.open_relative(component, FILE_GENERIC_READ, FILE_OPEN, false)?;
        if file.metadata()?.len() > limit as u64 {
            return Err(io::Error::other("Storage object exceeds its read limit."));
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

    fn temporary(&self) -> io::Result<Temporary> {
        let component = format!(".flint-{}.tmp", uuid::Uuid::new_v4());
        let file = self.open_relative(
            &component,
            FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE,
            FILE_CREATE,
            false,
        )?;
        Ok(Temporary {
            file,
            published: false,
        })
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
        match self.reader(component, super::store::MAX_SNAPSHOT_BYTES) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let mut temporary = self.temporary()?;
        temporary.file.write_all(bytes)?;
        temporary.file.sync_all()?;
        before_rename()?;
        self.publish(&temporary.file, component, true)?;
        temporary.published = true;
        after_rename()
    }

    pub(crate) fn immutable(&self, component: &str, bytes: &[u8]) -> io::Result<()> {
        if let Some(existing) = self.read_optional(component, super::store::MAX_COMMAND_BYTES)? {
            return if existing == bytes {
                self.open_relative(
                    component,
                    FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                    FILE_OPEN,
                    false,
                )?
                .sync_all()
            } else {
                Err(io::Error::other("Immutable storage identity conflict."))
            };
        }
        let mut temporary = self.temporary()?;
        temporary.file.write_all(bytes)?;
        temporary.file.sync_all()?;
        // A non-replacing handle-relative rename publishes the immutable file.
        // Unlike a check followed by a replace, an existing identity cannot win
        // a race and then be overwritten.
        self.publish(&temporary.file, component, false)?;
        temporary.published = true;
        Ok(())
    }

    fn publish(&self, file: &File, component: &str, replace: bool) -> io::Result<()> {
        let name = name(component)?;
        let length = offset_of!(FILE_RENAME_INFORMATION, FileName) + name.len() * 2;
        // usize storage provides the native structure's alignment, including
        // its variable-length UTF-16 filename.
        let mut storage = vec![
            0usize;
            length
                .max(size_of::<FILE_RENAME_INFORMATION>())
                .div_ceil(size_of::<usize>())
        ];
        let information = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        unsafe {
            (*information).Anonymous.ReplaceIfExists = replace;
            (*information).RootDirectory = self.0.as_raw_handle();
            (*information).FileNameLength = (name.len() * 2) as u32;
            std::ptr::copy_nonoverlapping(
                name.as_ptr(),
                (*information).FileName.as_mut_ptr(),
                name.len(),
            );
        }
        let mut status = IO_STATUS_BLOCK::default();
        nt_result(unsafe {
            NtSetInformationFile(
                file.as_raw_handle(),
                &mut status,
                information.cast(),
                length as u32,
                FileRenameInformation,
            )
        })
    }
}

struct Temporary {
    file: File,
    published: bool,
}

impl Drop for Temporary {
    fn drop(&mut self) {
        if !self.published {
            // Delete only the file opened with FILE_CREATE, not whatever may
            // now occupy its old name.
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            unsafe {
                SetFileInformationByHandle(
                    self.file.as_raw_handle(),
                    FileDispositionInfo,
                    (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                    size_of::<FILE_DISPOSITION_INFO>() as u32,
                );
            }
        }
    }
}

pub(crate) struct WriterLease {
    pub(crate) directory: Directory,
    _lock: File,
}

impl WriterLease {
    pub(crate) fn acquire(path: &Path) -> io::Result<Self> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata)
                if !metadata.is_dir()
                    || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 =>
            {
                return Err(io::Error::other(
                    "Session directory is not a real directory.",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                use std::os::windows::ffi::OsStrExt;
                if let Some(parent) = path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                {
                    std::fs::create_dir_all(parent)?;
                }
                let security = PrivateSecurity::new()?;
                let attributes = SECURITY_ATTRIBUTES {
                    nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                    lpSecurityDescriptor: security.0,
                    bInheritHandle: 0,
                };
                let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
                if unsafe { CreateDirectoryW(name.as_ptr(), &attributes) } == 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(error);
                    }
                }
            }
            Err(error) => return Err(error),
        }
        let directory = Directory::open(path)?;
        let lock = directory.open_relative(
            ".flint-writer.lock",
            FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            FILE_OPEN_IF,
            false,
        )?;
        let mut overlapped = OVERLAPPED::default();
        if unsafe {
            LockFileEx(
                lock.as_raw_handle(),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                1,
                0,
                &mut overlapped,
            )
        } == 0
        {
            let error = io::Error::last_os_error();
            if error.raw_os_error()
                == Some(windows_sys::Win32::Foundation::ERROR_LOCK_VIOLATION as i32)
            {
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

// Closing the non-inheritable lock handle releases the byte lock, including
// after a crash. No PID file or stale-lock deletion is involved.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_cannot_address_devices_streams_or_parent_directories() {
        for component in [
            "",
            ".",
            "..",
            r"a\b",
            "a/b",
            "C:escape",
            "history.json:stream",
            "NUL",
            "con.txt",
            "COM1",
            "COM¹",
            "CONIN$",
            "LPT9.txt",
            "trailing.",
            "trailing ",
            "a\0b",
            "a*b",
            "a?b",
            "a|b",
            "a\"b",
            "a<b",
            "a>b",
        ] {
            assert!(name(component).is_err(), "{component:?}");
        }
        for component in [
            "history.json",
            ".flint-writer.lock",
            "engine-objects",
            ".flint-unique.tmp",
        ] {
            assert!(name(component).is_ok());
        }
    }

    #[test]
    fn owned_directory_handles_refuse_directory_replacement() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("session");
        std::fs::create_dir(&path).unwrap();
        let directory = Directory::open(&path).unwrap();
        assert!(std::fs::rename(&path, parent.path().join("moved")).is_err());
        directory.replace("history.json", b"first").unwrap();
        directory.replace("history.json", b"second").unwrap();
        assert_eq!(directory.read("history.json", 100).unwrap(), b"second");
    }

    #[test]
    fn writer_lock_name_cannot_be_replaced_while_a_lease_is_active() {
        let dir = tempfile::tempdir().unwrap();
        let lease = WriterLease::acquire(dir.path()).unwrap();
        assert!(std::fs::remove_file(dir.path().join(".flint-writer.lock")).is_err());
        assert!(WriterLease::acquire(dir.path()).is_err());
        drop(lease);
        assert!(WriterLease::acquire(dir.path()).is_ok());
    }

    #[test]
    fn failed_publication_only_removes_its_own_temporary() {
        let parent = tempfile::tempdir().unwrap();
        let directory = Directory::open(parent.path()).unwrap();
        directory.replace("history.json", b"first").unwrap();
        std::fs::write(parent.path().join("history.json.tmp"), b"unrelated").unwrap();
        assert!(
            directory
                .replace_with(
                    "history.json",
                    b"second",
                    || Err(io::Error::other("injected")),
                    || Ok(())
                )
                .is_err()
        );
        assert_eq!(directory.read("history.json", 100).unwrap(), b"first");
        assert_eq!(
            std::fs::read(parent.path().join("history.json.tmp")).unwrap(),
            b"unrelated"
        );
        assert!(std::fs::read_dir(parent.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".flint-")
        }));
    }

    #[test]
    fn newly_published_files_have_protected_owner_and_system_acls() {
        use windows_sys::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SE_FILE_OBJECT,
        };
        use windows_sys::Win32::Security::DACL_SECURITY_INFORMATION;
        let dir = tempfile::tempdir().unwrap();
        let directory = Directory::open(dir.path()).unwrap();
        directory.replace("history.json", b"private").unwrap();
        let file = directory.reader("history.json", 100).unwrap();
        let mut descriptor = null_mut();
        assert_eq!(
            unsafe {
                GetSecurityInfo(
                    file.as_raw_handle(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    &mut descriptor,
                )
            },
            0
        );
        let descriptor = PrivateSecurity(descriptor);
        let mut text = null_mut();
        let mut length = 0;
        assert_ne!(
            unsafe {
                ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    descriptor.0,
                    1,
                    DACL_SECURITY_INFORMATION,
                    &mut text,
                    &mut length,
                )
            },
            0
        );
        let acl = String::from_utf16_lossy(unsafe {
            std::slice::from_raw_parts(text, length.saturating_sub(1) as usize)
        });
        unsafe {
            LocalFree(text.cast());
        }
        assert!(
            acl.contains("D:P") && acl.contains(";;;OW)") && acl.contains(";;;SY)"),
            "{acl}"
        );
        for public in [";;;WD)", ";;;BU)", ";;;AU)", ";;;AN)"] {
            assert!(!acl.contains(public), "{acl}");
        }
    }
}
