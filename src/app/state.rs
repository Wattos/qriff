#[derive(Default)]
pub struct State {
    pub host_available: bool,
    pub windows: usize,
    pub background: bool,
    pub window_ids: Vec<iced::window::Id>,
    pub session: u64,
    pub busy: bool,
    pub error: Option<String>,
    pub results: Vec<crate::results::QrResult>,
    pub scanned: bool,
}
impl State {
    pub fn begin_session(&mut self) -> crate::jobs::JobId {
        self.session = self.session.wrapping_add(1);
        self.busy = true;
        self.scanned = false;
        self.error = None;
        self.results.clear();
        crate::jobs::JobId(self.session)
    }
    pub fn accept_decode(&mut self, outcome: &crate::jobs::DecodeOutcome) {
        if outcome.id.0 != self.session {
            return;
        }
        self.busy = false;
        self.scanned = true;
        match &outcome.result {
            Ok(results) => {
                self.error = None;
                for result in results {
                    if !self.results.iter().any(|old| old.payload == result.payload) {
                        self.results.push(result.clone());
                    }
                }
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    pub fn close_window(&mut self) -> bool {
        self.windows = self.windows.saturating_sub(1);
        self.windows == 0 && !self.host_available
    }
    pub fn host_changed(&mut self, available: bool) -> bool {
        self.host_available = available;
        !available && self.windows == 0
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn last_window_with_host_stays_alive() {
        let mut s = State {
            host_available: true,
            windows: 1,
            ..Default::default()
        };
        assert!(!s.close_window());
        assert_eq!(s.windows, 0);
    }
    #[test]
    fn last_window_without_host_exits() {
        let mut s = State {
            windows: 1,
            ..Default::default()
        };
        assert!(s.close_window());
    }
    #[test]
    fn background_without_host_opens_window() {
        let mut s = State {
            background: true,
            ..Default::default()
        };
        assert!(s.host_changed(false));
    }
    #[test]
    fn losing_host_while_hidden_opens_window() {
        let mut s = State {
            host_available: true,
            ..Default::default()
        };
        assert!(s.host_changed(false));
    }
}

#[cfg(test)]
mod decode_tests {
    use super::*;
    use crate::{
        jobs::{DecodeOutcome, JobId},
        results::QrResult,
    };
    #[test]
    fn ignores_completed_old_session() {
        let mut state = State::default();
        let old = state.begin_session();
        let current = state.begin_session();
        state.accept_decode(&DecodeOutcome {
            id: old,
            result: Ok(vec![QrResult::from_payload(b"stale".to_vec())]),
        });
        assert!(state.results.is_empty());
        assert!(state.busy);
        state.accept_decode(&DecodeOutcome {
            id: current,
            result: Ok(vec![QrResult::from_payload(b"current".to_vec())]),
        });
        assert_eq!(state.results.len(), 1);
        assert_eq!(state.results[0].payload, b"current");
        assert!(!state.busy);
    }
    #[test]
    fn session_results_are_unique_complete_and_cleared() {
        let mut state = State::default();
        let id = state.begin_session();
        for i in 0..200u16 {
            state.accept_decode(&DecodeOutcome {
                id,
                result: Ok(vec![
                    QrResult::from_payload(i.to_be_bytes().to_vec()),
                    QrResult::from_payload(vec![0, 0]),
                ]),
            });
        }
        assert_eq!(state.results.len(), 200);
        assert_eq!(state.results[0].payload, [0, 0]);
        let next = state.begin_session();
        assert_ne!(id, next);
        assert!(state.results.is_empty());
        assert!(!state.scanned);
    }
    #[test]
    fn empty_scan_and_failure_are_distinct() {
        let mut state = State::default();
        let id = state.begin_session();
        state.accept_decode(&DecodeOutcome {
            id,
            result: Ok(vec![]),
        });
        assert!(state.scanned);
        assert!(state.error.is_none());
        assert!(!state.busy);
        state.accept_decode(&DecodeOutcome {
            id,
            result: Err(anyhow::anyhow!("broken QR")),
        });
        assert_eq!(state.error.as_deref(), Some("broken QR"));
        state.accept_decode(&DecodeOutcome {
            id: JobId(id.0 + 1),
            result: Ok(vec![]),
        });
        assert!(state.error.is_some());
    }
}
