#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use clap::Parser;
    let command = qr_scanner::cli::Cli::parse().command();
    let primary = qr_scanner::instance::forward_or_listen(command)?;
    qr_scanner::app::run(command, primary)?;
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("This development build supports Linux only")
}
