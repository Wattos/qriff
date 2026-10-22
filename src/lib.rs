#[cfg(target_os = "linux")]
pub mod app;
pub mod cli;
pub mod decode;
#[cfg(target_os = "linux")]
pub mod instance;
pub mod jobs;
pub mod results;
#[cfg(target_os = "linux")]
pub mod tray;

pub mod capture;

#[cfg(target_os = "linux")]
pub mod camera;

pub mod settings;

pub mod actions;

#[cfg(target_os = "linux")]
pub mod desktop;

pub mod overlay;

#[cfg(target_os = "linux")]
pub mod display;
