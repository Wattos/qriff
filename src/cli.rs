use clap::Parser;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    Show,
    ScanScreen,
    ScanRegion,
    CopyResult(u64, usize),
    ActivateResult(u64, usize),
    ScanCamera,
    Background,
    Quit,
}
#[derive(Parser)]
#[command(version, about = "Local QR scanner for Linux")]
pub struct Cli {
    #[arg(long, conflicts_with_all=["scan_screen", "scan_camera", "quit"])]
    pub background: bool,
    #[arg(long, conflicts_with_all=["scan_camera", "quit"])]
    pub scan_screen: bool,
    #[arg(long, conflicts_with = "quit")]
    pub scan_camera: bool,
    #[arg(long)]
    pub quit: bool,
}
impl Cli {
    pub fn command(&self) -> Command {
        if self.quit {
            Command::Quit
        } else if self.scan_screen {
            Command::ScanScreen
        } else if self.scan_camera {
            Command::ScanCamera
        } else if self.background {
            Command::Background
        } else {
            Command::Show
        }
    }
}
