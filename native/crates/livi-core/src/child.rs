// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::process::Stdio;
use std::time::Duration;

use livi_core_proto::LIFELINE_ENV;
use tokio::process::{Child, ChildStdin, Command};

pub fn with_lifeline(cmd: &mut Command) -> &mut Command {
    cmd.stdin(Stdio::piped()).env(LIFELINE_ENV, "1")
}

pub fn hold_lifeline(child: &mut Child) -> Option<ChildStdin> {
    child.stdin.take()
}

pub async fn terminate(child: &mut Child, grace: Duration, group: bool) {
    if let Some(pid) = child.id().and_then(|p| i32::try_from(p).ok()) {
        let target = if group { -pid } else { pid };
        // SAFETY: plain signal delivery to a process we started, sudo passes it on.
        unsafe { libc::kill(target, libc::SIGTERM) };
        if tokio::time::timeout(grace, child.wait()).await.is_ok() {
            return;
        }
        if group {
            // SAFETY: as above.
            unsafe { libc::kill(target, libc::SIGKILL) };
        }
    }
    let _ = child.kill().await;
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;

    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command;

    use super::*;

    #[tokio::test]
    async fn a_polite_child_ends_on_term() {
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        terminate(&mut child, Duration::from_secs(5), false).await;
        let status = child.wait().await.unwrap();
        assert_eq!(status.signal(), Some(libc::SIGTERM));
    }

    #[tokio::test]
    async fn the_lifeline_ends_only_with_the_child_handle() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("cat >/dev/null; echo gone");
        let mut child = with_lifeline(&mut cmd)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let lifeline = hold_lifeline(&mut child);
        assert!(
            tokio::time::timeout(Duration::from_millis(300), child.wait())
                .await
                .is_err()
        );
        drop(lifeline);
        let out = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out.stdout, b"gone\n");
    }

    #[tokio::test]
    async fn a_stubborn_group_is_killed() {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("trap '' TERM; sleep 30 & echo ready; wait")
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap();
        let mut ready = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut ready)
            .await
            .unwrap();
        terminate(&mut child, Duration::from_millis(300), true).await;
        assert_eq!(child.wait().await.unwrap().signal(), Some(libc::SIGKILL));
    }
}
