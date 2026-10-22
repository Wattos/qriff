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
