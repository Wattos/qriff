#[cfg(target_os = "linux")]
pub mod app;
pub mod cli;
#[cfg(target_os = "linux")]
pub mod instance;
#[cfg(target_os = "linux")]
pub mod tray;
