//! Stop the owned process tree even when its leader exits before descendants.

use std::time::Duration;
use tokio::process::Child;

pub(crate) const DRAIN_GRACE: Duration = Duration::from_millis(250);

/// Escalate the entire owned group even if its leader exits on TERM; reap the
/// direct child. Descendants that escape the group are outside this guarantee.
pub(crate) async fn stop(child: &mut Child, tree: &flint_platform::ProcessTree, grace: Duration) {
    tree.terminate();
    // Waiting for the leader alone must not skip descendant escalation.
    tokio::time::sleep(grace).await;
    tree.kill();
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
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "trap 'exit 0' TERM; (trap '' TERM; sleep 1; printf escaped > escaped) & echo ready > ready; wait"])
            .current_dir(dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let (mut child, mut group) = flint_platform::spawn(&mut command).unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !dir.path().join("ready").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        stop(&mut child, &group, DRAIN_GRACE).await;
        group.disarm();
        assert_eq!(child.wait().await.unwrap().code(), Some(0));
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(
            !dir.path().join("escaped").exists(),
            "TERM exit of leader must not skip descendant KILL"
        );
    }
}
