#[derive(Default)]
pub struct State {
    pub host_available: bool,
    pub windows: usize,
    pub background: bool,
    pub window_ids: Vec<iced::window::Id>,
    pub session: u64,
    pub busy: bool,
    pub error: Option<String>,
    pub results: Vec<String>,
}
impl State {
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
