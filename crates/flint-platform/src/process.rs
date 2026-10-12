//! Owned child trees: Unix process groups and Windows kill-on-close Job Objects.

use std::io;
use tokio::process::{Child, Command};

pub struct ProcessTree {
    #[cfg(unix)]
    pid: Option<u32>,
    #[cfg(windows)]
    job: windows::Job,
}

/// Windows children start suspended so no descendant can escape before job
/// assignment. Failure to attach or resume fails closed and kills the child.
pub fn spawn(command: &mut Command) -> io::Result<(Child, ProcessTree)> {
    command.kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(windows)]
    command.creation_flags(
        windows_sys::Win32::System::Threading::CREATE_SUSPENDED
            | windows_sys::Win32::System::Threading::CREATE_NO_WINDOW,
    );
    #[allow(unused_mut)]
    let mut child = command.spawn()?;
    #[cfg(unix)]
    let tree = ProcessTree { pid: child.id() };
    #[cfg(windows)]
    let tree = match windows::Job::attach(&child) {
        Ok(job) => ProcessTree { job },
        Err(error) => {
            let _ = child.start_kill();
            return Err(error);
        }
    };
    #[cfg(not(any(unix, windows)))]
    let tree = ProcessTree {};
    Ok((child, tree))
}

impl ProcessTree {
    pub fn terminate(&self) {
        #[cfg(unix)]
        self.signal(libc::SIGTERM);
        #[cfg(windows)]
        self.job.kill();
    }

    pub fn kill(&self) {
        #[cfg(unix)]
        self.signal(libc::SIGKILL);
        #[cfg(windows)]
        self.job.kill();
    }

    #[cfg(unix)]
    fn signal(&self, signal: i32) {
        if let Some(pid) = self
            .pid
            .and_then(|pid| i32::try_from(pid).ok())
            .filter(|pid| *pid > 0)
        {
            // spawn() put only our own child in this new process group.
            unsafe {
                libc::kill(-pid, signal);
            }
        }
    }

    /// Call only after all descendants have been stopped. Closing a Windows
    /// job still kills members, including during runtime teardown.
    pub fn disarm(&mut self) {
        #[cfg(unix)]
        {
            self.pid = None;
        }
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::mem::size_of;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::*;

    // NtResumeProcess resumes the original suspended thread without a racy
    // thread enumeration or asking tokio to expose its private thread handle.
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtResumeProcess(process: HANDLE) -> i32;
    }

    pub(super) struct Job(OwnedHandle);

    impl Job {
        pub(super) fn attach(child: &Child) -> io::Result<Self> {
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Self(unsafe { OwnedHandle::from_raw_handle(handle) });
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            let process = child
                .raw_handle()
                .ok_or_else(|| io::Error::other("Child process has no handle."))?;
            if unsafe { AssignProcessToJobObject(handle, process) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let status = unsafe { NtResumeProcess(process) };
            if status < 0 {
                return Err(io::Error::from_raw_os_error(unsafe {
                    windows_sys::Win32::Foundation::RtlNtStatusToDosError(status)
                } as i32));
            }
            Ok(job)
        }

        pub(super) fn kill(&self) {
            unsafe {
                TerminateJobObject(self.0.as_raw_handle(), 1);
            }
        }
    }

    // OwnedHandle, rather than a bare HANDLE, keeps Job Send + Sync and closes
    // the non-inheritable handle even when an async task is abandoned.
    const _: unsafe extern "system" fn(HANDLE) -> i32 = CloseHandle;
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::process::Stdio;
    use std::time::Duration;

    #[tokio::test]
    async fn dropping_owned_tree_stops_background_descendants() {
        let dir = tempfile::tempdir().unwrap();
        let script = "$p = Start-Process powershell.exe -ArgumentList '-NoProfile -Command \"Start-Sleep -Seconds 2; Set-Content escaped escaped\"' -PassThru; Set-Content ready $p.Id; Start-Sleep -Seconds 30";
        let mut command = Command::new("powershell.exe");
        command
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .current_dir(dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let (mut child, tree) = spawn(&mut command).unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            while !dir.path().join("ready").exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        drop(tree);
        assert!(!child.wait().await.unwrap().success());
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(!dir.path().join("escaped").exists());
    }
}
