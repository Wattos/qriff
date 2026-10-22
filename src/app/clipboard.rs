use crate::{jobs::JobId, results::QrResult};
/// Evaluated only after the controller has filtered stale outcomes and deduplicated payloads.
#[derive(Default)]
pub struct AutoCopyPolicy {
    copied: Option<JobId>,
}
impl AutoCopyPolicy {
    pub fn on_results(
        &mut self,
        enabled: bool,
        session: JobId,
        results: &[QrResult],
        newly_added: usize,
    ) -> Option<String> {
        if !enabled || newly_added != 1 || results.len() != 1 || self.copied == Some(session) {
            return None;
        }
        self.copied = Some(session);
        Some(results[0].display_text.clone())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ClipboardUnavailable;
impl std::fmt::Display for ClipboardUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Windowless clipboard unavailable.")
    }
}
impl std::error::Error for ClipboardUnavailable {}
/// Wayland data-control clipboard ownership does not require a visible window.
/// Payload goes over stdin, never in process arguments or logs.
pub async fn write_windowless(text: String) -> Result<(), ClipboardUnavailable> {
    write_using(std::path::Path::new("wl-copy"), text).await
}
async fn write_using(
    executable: &std::path::Path,
    text: String,
) -> Result<(), ClipboardUnavailable> {
    use tokio::io::AsyncWriteExt;
    let mut child = tokio::process::Command::new(executable)
        .args(["--type", "text/plain;charset=utf-8"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| ClipboardUnavailable)?;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut stdin = child.stdin.take().ok_or(ClipboardUnavailable)?;
        stdin
            .write_all(text.as_bytes())
            .await
            .map_err(|_| ClipboardUnavailable)?;
        drop(stdin);
        if child
            .wait()
            .await
            .map_err(|_| ClipboardUnavailable)?
            .success()
        {
            Ok(())
        } else {
            Err(ClipboardUnavailable)
        }
    })
    .await
    .map_err(|_| ClipboardUnavailable)?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn result(text: &[u8]) -> QrResult {
        QrResult::from_payload(text.to_vec())
    }
    #[test]
    fn introduction_only_once_and_no_retroactive_copy() {
        let mut policy = AutoCopyPolicy::default();
        let one = vec![result(b"one")];
        assert_eq!(policy.on_results(false, JobId(1), &one, 1), None);
        assert_eq!(policy.on_results(true, JobId(1), &one, 0), None);
        assert_eq!(
            policy.on_results(true, JobId(2), &one, 1),
            Some("one".into())
        );
        assert_eq!(policy.on_results(true, JobId(2), &one, 1), None);
        assert_eq!(policy.on_results(true, JobId(2), &one, 0), None);
        let two = vec![result(b"one"), result(b"two")];
        assert_eq!(policy.on_results(true, JobId(2), &two, 1), None);
        assert_eq!(policy.on_results(true, JobId(3), &two, 2), None);
        assert_eq!(
            policy.on_results(true, JobId(4), &one, 1),
            Some("one".into())
        );
    }
    #[test]
    fn exact_binary_and_wifi_text() {
        for payload in [b"\xff\x00".as_slice(), b"WIFI:T:WPA;S:home;P:secret;;"] {
            let result = result(payload);
            assert_eq!(
                AutoCopyPolicy::default().on_results(
                    true,
                    JobId(1),
                    std::slice::from_ref(&result),
                    1
                ),
                Some(result.display_text)
            );
        }
    }
}

#[cfg(test)]
mod windowless_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[tokio::test]
    async fn windowless_copy_uses_stdin_and_reports_failure_without_user_clipboard() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("wl-copy");
        let copied = directory.path().join("payload");
        let args = directory.path().join("args");
        std::fs::write(
            &executable,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\ncat > '{}'\n",
                args.display(),
                copied.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        write_using(&executable, "WIFI:P:secret;\\n☃".into())
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(copied).unwrap(),
            "WIFI:P:secret;\\n☃"
        );
        assert_eq!(
            std::fs::read_to_string(args).unwrap(),
            "--type\ntext/plain;charset=utf-8\n"
        );
        std::fs::write(&executable, "#!/bin/sh\nexit 1\n").unwrap();
        assert!(write_using(&executable, "text".into()).await.is_err());
        assert!(
            write_using(&directory.path().join("missing"), "text".into())
                .await
                .is_err()
        );
    }
}
