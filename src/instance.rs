use crate::cli::Command;
use anyhow::{Context, bail};
use fs2::FileExt;
use std::os::unix::{
    fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    net::{UnixListener, UnixStream},
};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
pub enum InstanceRole {
    Forwarded,
    Primary(Primary),
}
pub struct Primary {
    pub listener: UnixListener,
    socket: PathBuf,
    _lock: File,
}
impl Drop for Primary {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket);
    }
}
pub fn forward_or_listen(command: Command) -> anyhow::Result<InstanceRole> {
    let root = std::env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is required")?;
    validate(Path::new(&root))?;
    let directory = PathBuf::from(root).join("qr-scanner");
    match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    at(&directory, command)
}
fn validate(path: &Path) -> anyhow::Result<()> {
    let m = std::fs::symlink_metadata(path)?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
        bail!("Runtime directory must be a private directory owned by the current user");
    }
    Ok(())
}
pub fn at(directory: &Path, command: Command) -> anyhow::Result<InstanceRole> {
    validate(directory)?;
    let socket = directory.join("control.sock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(directory.join("instance.lock"))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if let Ok(mut stream) = UnixStream::connect(&socket) {
                    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
                    stream.write_all(&serde_json::to_vec(&command)?)?;
                    return Ok(InstanceRole::Forwarded);
                }
                if Instant::now() >= deadline {
                    bail!("Primary instance is starting or unavailable");
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(e.into()),
        }
    }
    // Even with our lock, never unlink an independently live listener.
    match UnixStream::connect(&socket) {
        Ok(mut stream) => {
            stream.set_write_timeout(Some(Duration::from_secs(1)))?;
            stream.write_all(&serde_json::to_vec(&command)?)?;
            return Ok(InstanceRole::Forwarded);
        }
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) => {}
        Err(e) => return Err(e.into()),
    }
    match std::fs::symlink_metadata(&socket) {
        Ok(m) => {
            use std::os::unix::fs::FileTypeExt;
            if !m.file_type().is_socket() {
                bail!("Refusing to remove a non-socket control path");
            }
            std::fs::remove_file(&socket)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let listener = UnixListener::bind(&socket)?;
    Ok(InstanceRole::Primary(Primary {
        listener,
        socket,
        _lock: lock,
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    impl Primary {
        fn receive(&mut self) -> anyhow::Result<Command> {
            let (mut s, _) = self.listener.accept()?;
            s.set_read_timeout(Some(Duration::from_secs(1)))?;
            let mut bytes = Vec::new();
            std::io::Read::by_ref(&mut s)
                .take(256)
                .read_to_end(&mut bytes)?;
            Ok(serde_json::from_slice(&bytes)?)
        }
    }

    use std::os::unix::net::UnixListener;
    fn private_dir() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::set_permissions(
            d.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        d
    }
    #[test]
    fn concurrent_invocations_elect_one_primary() {
        let d = private_dir();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let barrier = barrier.clone();
                let tx = tx.clone();
                let path = d.path();
                scope.spawn(move || {
                    barrier.wait();
                    match at(path, Command::Show).unwrap() {
                        InstanceRole::Primary(mut p) => {
                            tx.send(true).unwrap();
                            for _ in 0..7 {
                                assert_eq!(p.receive().unwrap(), Command::Show);
                            }
                        }
                        InstanceRole::Forwarded => tx.send(false).unwrap(),
                    }
                });
            }
        });
        drop(tx);
        assert_eq!(rx.iter().filter(|v| *v).count(), 1);
    }
    #[test]
    fn live_listener_without_lock_is_never_unlinked() {
        let d = private_dir();
        let listener = UnixListener::bind(d.path().join("control.sock")).unwrap();
        assert!(matches!(
            at(d.path(), Command::Quit).unwrap(),
            InstanceRole::Forwarded
        ));
        assert!(d.path().join("control.sock").exists());
        drop(listener);
    }
    #[test]
    fn refuses_world_accessible_runtime_directory() {
        let d = private_dir();
        std::fs::set_permissions(
            d.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        assert!(at(d.path(), Command::Show).is_err());
    }
    #[test]
    fn refuses_non_socket_control_path() {
        let d = private_dir();
        std::fs::write(d.path().join("control.sock"), "keep").unwrap();
        assert!(at(d.path(), Command::Show).is_err());
        assert_eq!(
            std::fs::read_to_string(d.path().join("control.sock")).unwrap(),
            "keep"
        );
    }
    #[test]
    fn stale_socket_is_recovered() {
        let d = tempfile::tempdir().unwrap();
        std::fs::set_permissions(
            d.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        let l = UnixListener::bind(d.path().join("control.sock")).unwrap();
        drop(l);
        assert!(matches!(
            at(d.path(), Command::Show).unwrap(),
            InstanceRole::Primary(_)
        ));
    }
    #[test]
    fn second_invocation_forwards_without_unlinking_primary() {
        let d = tempfile::tempdir().unwrap();
        std::fs::set_permissions(
            d.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        let InstanceRole::Primary(mut p) = at(d.path(), Command::Show).unwrap() else {
            panic!()
        };
        assert!(matches!(
            at(d.path(), Command::ScanScreen).unwrap(),
            InstanceRole::Forwarded
        ));
        assert_eq!(p.receive().unwrap(), Command::ScanScreen);
        assert!(matches!(
            at(d.path(), Command::Quit).unwrap(),
            InstanceRole::Forwarded
        ));
        assert_eq!(p.receive().unwrap(), Command::Quit);
    }
}
