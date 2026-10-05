//! Unix commands own a fresh process group. Never signal the caller's group.

use std::time::Duration;
use tokio::process::Child;

pub(crate) const DRAIN_GRACE: Duration = Duration::from_millis(250);

pub(crate) struct GroupGuard(Option<u32>);

impl GroupGuard {
    pub(crate) fn new(pid: Option<u32>) -> Self {
        Self(pid)
    }

    pub(crate) fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for GroupGuard {
    fn drop(&mut self) {
        // Runtime teardown can cancel asynchronous cleanup. This last-resort
        // local signal never depends on that runtime being alive.
        #[cfg(unix)]
        if let Some(pid) = self.0 {
            let _ = std::process::Command::new("/bin/kill")
                .args(["-KILL", "--", &format!("-{pid}")])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}

pub(crate) async fn signal_group(pid: Option<u32>, signal: &str) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        let mut command = tokio::process::Command::new("/bin/kill");
        command
            .arg(format!("-{signal}"))
            .arg("--")
            .arg(format!("-{pid}"))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let _ = tokio::time::timeout(Duration::from_secs(1), command.status()).await;
    }
    #[cfg(not(unix))]
    let _ = (pid, signal);
}

/// Escalate the entire owned group even if its leader exits on TERM; reap the
/// direct child. Descendants that escape the group are outside this guarantee.
pub(crate) async fn stop(child: &mut Child, pid: Option<u32>, grace: Duration) {
    signal_group(pid, "TERM").await;
    #[cfg(not(unix))]
    let _ = child.start_kill();
    // Waiting for the leader alone must not skip descendant escalation.
    tokio::time::sleep(grace).await;
    signal_group(pid, "KILL").await;
    let _ = child.start_kill();
    let _ = child.wait().await;
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Stdio;
    use tokio::process::Command;

    #[tokio::test]
    async fn group_escalation_survives_a_leader_exiting_on_term() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = Command::new("/bin/sh")
            .args(["-c", "trap 'exit 0' TERM; (trap '' TERM; sleep 1; printf escaped > escaped) & echo ready > ready; wait"])
            .current_dir(dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut group = GroupGuard::new(pid);
        tokio::time::timeout(Duration::from_secs(3), async {
            while !dir.path().join("ready").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        stop(&mut child, pid, DRAIN_GRACE).await;
        group.disarm();
        assert_eq!(child.wait().await.unwrap().code(), Some(0));
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(
            !dir.path().join("escaped").exists(),
            "TERM exit of leader must not skip descendant KILL"
        );
    }
}
