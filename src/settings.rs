use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Preferences {
    pub auto_copy_single: bool,
}
#[derive(Debug, Clone)]
pub struct LoadedPreferences {
    pub preferences: Preferences,
    pub path: Option<PathBuf>,
    pub warning: Option<String>,
}
pub fn load() -> LoadedPreferences {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|p| !p.is_empty())
                .map(|p| PathBuf::from(p).join(".config"))
        });
    load_path(base.map(|p| p.join("qr-scanner/settings.json")))
}
fn load_path(path: Option<PathBuf>) -> LoadedPreferences {
    let mut loaded = LoadedPreferences {
        preferences: Preferences::default(),
        path,
        warning: None,
    };
    match loaded.path.as_ref() {
        None => loaded.warning = Some("Settings unavailable: no configuration directory.".into()),
        Some(path) => match fs::read(path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(preferences) => loaded.preferences = preferences,
                Err(_) => loaded.warning = Some(
                    "Settings could not be read. Using defaults; change a setting to replace them."
                        .into(),
                ),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => loaded.warning = Some("Settings could not be read. Using defaults.".into()),
        },
    }
    loaded
}
fn ensure_directory(path: &Path) -> anyhow::Result<()> {
    if path.as_os_str().is_empty() {
        bail!("Settings directory unavailable");
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        ensure_directory(parent)?;
    }
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_dir() => Ok(()),
        Ok(_) => bail!("Settings directory is not a regular directory"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::DirBuilder::new()
            .mode(0o700)
            .create(path)
            .context("Create settings directory"),
        Err(error) => Err(error).context("Inspect settings directory"),
    }
}
pub fn save(path: &Path, preferences: &Preferences) -> anyhow::Result<()> {
    let parent = path.parent().context("Settings directory unavailable")?;
    ensure_directory(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    match fs::symlink_metadata(path) {
        Ok(meta) if !meta.file_type().is_file() => {
            bail!("Settings destination is not a regular file")
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temp = parent.join(format!(
        ".settings-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut created = false;
    let result = (|| -> anyhow::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temp)?;
        created = true;
        file.write_all(&serde_json::to_vec(preferences)?)?;
        file.sync_all()?;
        // Recheck after writing so unexpected replacements are rejected as well.
        if let Ok(meta) = fs::symlink_metadata(path)
            && !meta.file_type().is_file()
        {
            bail!("Settings destination is not a regular file");
        }
        fs::rename(&temp, path)?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(temp);
    }
    result.context("Save settings")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_unknown_fields_and_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.json");
        let missing = load_path(Some(path.clone()));
        assert!(!missing.preferences.auto_copy_single);
        assert!(missing.warning.is_none());
        std::fs::write(&path, r#"{"future":true}"#).unwrap();
        assert!(!load_path(Some(path.clone())).preferences.auto_copy_single);
        std::fs::write(&path, b"broken").unwrap();
        assert!(load_path(Some(path.clone())).warning.is_some());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
        assert!(load_path(None).warning.is_some());
        assert!(load_path(Some(dir.path().to_path_buf())).warning.is_some());
    }
    #[test]
    fn private_atomic_save_and_reject_unsafe_destinations() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config/preferences.json");
        save(
            &path,
            &Preferences {
                auto_copy_single: true,
            },
        )
        .unwrap();
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(load_path(Some(path.clone())).preferences.auto_copy_single);
        save(&path, &Preferences::default()).unwrap();
        assert!(!load_path(Some(path.clone())).preferences.auto_copy_single);
        std::fs::remove_file(&path).unwrap();
        symlink(dir.path().join("victim"), &path).unwrap();
        assert!(save(&path, &Preferences::default()).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(save(&path, &Preferences::default()).is_err());
        assert!(
            save(
                &dir.path().join("missing-parent-file/a"),
                &Preferences::default()
            )
            .is_ok()
        );
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        assert!(save(&blocker.join("a"), &Preferences::default()).is_err());
        let linked_parent = dir.path().join("linked");
        symlink(dir.path().join("config"), &linked_parent).unwrap();
        assert!(
            save(
                &linked_parent.join("settings.json"),
                &Preferences::default()
            )
            .is_err()
        );
    }
}
