pub mod crop;
pub mod preview;
#[derive(Debug)]
pub struct Prepared {
    pub image: image::DynamicImage,
    pub handle: iced::widget::image::Handle,
}
impl Prepared {
    pub fn new(image: image::DynamicImage) -> Self {
        let rgba = image.to_rgba8();
        Self {
            handle: iced::widget::image::Handle::from_rgba(
                rgba.width(),
                rgba.height(),
                rgba.into_raw(),
            ),
            image,
        }
    }
}
#[derive(Debug)]
pub enum CaptureOutcome {
    Captured(image::DynamicImage),
    Cancelled,
}
/// Fixed, secret-safe categories are the only capture errors exposed to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureError {
    Failed,
    RegionUnavailable,
    SpectacleUnavailable,
    PrepareFailed,
}
impl CaptureError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Failed => "Could not capture the desktop.",
            Self::RegionUnavailable => {
                "Direct region selection is unavailable on this desktop. Use Scan screen or Desktop capture instead."
            }
            Self::SpectacleUnavailable => {
                "Spectacle is required for native region selection on KDE. Install Spectacle, or use Scan screen or Desktop capture instead."
            }
            Self::PrepareFailed => "Could not prepare the desktop capture.",
        }
    }
}
impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}
impl std::error::Error for CaptureError {}
impl From<anyhow::Error> for CaptureError {
    fn from(error: anyhow::Error) -> Self {
        error
            .downcast_ref::<Self>()
            .copied()
            .unwrap_or(Self::Failed)
    }
}
fn load_response(
    result: anyhow::Result<Option<String>>,
    loader: impl FnOnce(&std::path::Path) -> anyhow::Result<image::DynamicImage>,
) -> anyhow::Result<CaptureOutcome> {
    let Some(uri) = result? else {
        return Ok(CaptureOutcome::Cancelled);
    };
    let path = url::Url::parse(&uri)?
        .to_file_path()
        .map_err(|_| anyhow::anyhow!("Portal returned a non-local file URI"))?;
    Ok(CaptureOutcome::Captured(loader(&path)?))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaptureMode {
    #[default]
    Quick,
    Interactive,
    Region,
}

fn screenshot_options(
    mode: CaptureMode,
    area_available: bool,
) -> Result<ashpd::desktop::screenshot::ScreenshotOptions, CaptureError> {
    if mode == CaptureMode::Region && !area_available {
        return Err(CaptureError::RegionUnavailable);
    }
    use ashpd::desktop::screenshot::{AvailableTargets, ScreenshotOptions};
    let options = ScreenshotOptions::default()
        .set_interactive(mode != CaptureMode::Quick)
        .set_modal(false);
    if mode != CaptureMode::Quick && area_available {
        Ok(options.set_target(AvailableTargets::Area))
    } else {
        Ok(options)
    }
}

/// Request a screenshot using the desktop portal.
pub async fn screenshot(mode: CaptureMode) -> Result<CaptureOutcome, CaptureError> {
    screenshot_inner(mode).await.map_err(CaptureError::from)
}
async fn screenshot_inner(mode: CaptureMode) -> anyhow::Result<CaptureOutcome> {
    use ashpd::desktop::screenshot::{AvailableTargets, ScreenshotProxy};
    if mode == CaptureMode::Region
        && std::env::var("XDG_CURRENT_DESKTOP")
            .is_ok_and(|v| v.split(':').any(|part| part.eq_ignore_ascii_case("KDE")))
    {
        return spectacle_region().await;
    }
    let proxy = ScreenshotProxy::new().await?;
    let area_available = mode != CaptureMode::Quick
        && proxy.version() >= 3
        && proxy
            .available_targets()
            .await
            .is_ok_and(|targets| targets.contains(AvailableTargets::Area));
    let options = screenshot_options(mode, area_available)?;
    let response = match proxy.screenshot(None, options).await {
        Ok(request) => request.response(),
        Err(error) => Err(error),
    };
    let result = match response {
        Ok(image) => Ok(Some(image.uri().as_str().to_owned())),
        Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)) => Ok(None),
        Err(error) => Err(error.into()),
    };
    tokio::task::spawn_blocking(move || load_response(result, load_local)).await?
}
/// Spectacle owns the native Wayland overlay; its image lives only in a private
/// temporary directory until decoding, never in the user's screenshot collection.
async fn spectacle_region() -> anyhow::Result<CaptureOutcome> {
    spectacle_region_using(std::path::Path::new("spectacle")).await
}
async fn spectacle_region_using(executable: &std::path::Path) -> anyhow::Result<CaptureOutcome> {
    let directory = tempfile::Builder::new().prefix("qr-region-").tempdir()?;
    let path = directory.path().join("region.png");
    // Isolate selection history from the user's Spectacle instance and settings.
    // Spectacle has no CLI option to disable restoring the previous rectangle.
    let config = directory.path().join("config");
    std::fs::create_dir(&config)?;
    std::fs::write(
        config.join("spectaclerc"),
        "[General]\nrememberSelectionRect=Never\n[GuiConfig]\nselectionRect=0,0,0,0\n",
    )?;
    let mut child = tokio::process::Command::new(executable)
        .env("XDG_CONFIG_HOME", &config)
        .args([
            "--background",
            "--nonotify",
            "--region",
            "--release-capture",
            "--new-instance",
            "--output",
        ])
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                CaptureError::SpectacleUnavailable
            } else {
                CaptureError::Failed
            }
        })?;
    let status = tokio::time::timeout(std::time::Duration::from_secs(180), child.wait())
        .await
        .map_err(|_| anyhow::anyhow!("Region selection timed out."))??;
    if !path.exists() {
        if status.success() || status.code() == Some(1) {
            return Ok(CaptureOutcome::Cancelled);
        }
        anyhow::bail!("Region capture failed.");
    }
    if !status.success() {
        anyhow::bail!("Region capture failed.");
    }
    tokio::task::spawn_blocking(move || {
        let image = load_local(&path)?;
        drop(directory);
        Ok(CaptureOutcome::Captured(image))
    })
    .await?
}
fn load_local(path: &std::path::Path) -> anyhow::Result<image::DynamicImage> {
    let file = std::fs::File::open(path)?;
    let mut reader =
        image::ImageReader::new(std::io::BufReader::new(file)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader.decode()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adapter_results() {
        assert!(matches!(
            load_response(Ok(None), |_| panic!()),
            Ok(CaptureOutcome::Cancelled)
        ));
        assert!(load_response(Err(anyhow::anyhow!("permission denied")), |_| panic!()).is_err());
        assert!(load_response(Ok(Some("https://example.org/a".into())), |_| panic!()).is_err());
        assert!(matches!(
            load_response(Ok(Some("file:///tmp/a%20b.png".into())), |p| {
                assert_eq!(p, std::path::Path::new("/tmp/a b.png"));
                Ok(image::DynamicImage::new_rgb8(1, 1))
            }),
            Ok(CaptureOutcome::Captured(_))
        ));
    }
    #[test]
    fn missing_and_corrupt() {
        assert!(
            load_response(
                Ok(Some("file:///definitely-missing-qr.png".into())),
                load_local
            )
            .is_err()
        );
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"corrupt").unwrap();
        assert!(
            load_response(
                Ok(Some(
                    url::Url::from_file_path(file.path()).unwrap().to_string()
                )),
                load_local
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod capture_mode_tests {
    use super::*;
    use zbus::zvariant::{self, LE, OwnedValue, serialized::Context};
    fn wire_options(
        mode: CaptureMode,
        area_available: bool,
    ) -> std::collections::HashMap<String, OwnedValue> {
        let bytes = zvariant::to_bytes(
            Context::new_dbus(LE, 0),
            &screenshot_options(mode, area_available).unwrap(),
        )
        .unwrap();
        bytes.deserialize().unwrap().0
    }
    #[test]
    fn unsupported_region_is_typed_and_never_builds_an_alternative_dialog() {
        assert!(matches!(
            screenshot_options(CaptureMode::Region, false),
            Err(CaptureError::RegionUnavailable)
        ));
        let options = wire_options(CaptureMode::Region, true);
        assert_eq!(
            u32::try_from(options["target"].try_clone().unwrap()).unwrap(),
            4
        );
    }
    #[test]
    fn provider_diagnostics_cannot_impersonate_safe_categories() {
        for detail in [
            "private provider details",
            CaptureError::RegionUnavailable.message(),
        ] {
            assert_eq!(
                CaptureError::from(anyhow::anyhow!(detail)),
                CaptureError::Failed
            );
        }
        assert_eq!(
            CaptureError::from(
                anyhow::Error::new(CaptureError::RegionUnavailable).context("private context")
            ),
            CaptureError::RegionUnavailable
        );
    }
    #[test]
    fn quick_capture_never_requests_a_dialog_or_native_area_selector() {
        for area_available in [false, true] {
            let options = wire_options(CaptureMode::Quick, area_available);
            assert!(!bool::try_from(options["interactive"].try_clone().unwrap()).unwrap());
            assert!(!options.contains_key("target"));
        }
    }
    #[test]
    fn explicit_desktop_capture_uses_interactive_selection_only_when_supported() {
        let older = wire_options(CaptureMode::Interactive, false);
        assert!(bool::try_from(older["interactive"].try_clone().unwrap()).unwrap());
        assert!(!older.contains_key("target"));
        let newer = wire_options(CaptureMode::Interactive, true);
        assert_eq!(
            u32::try_from(newer["target"].try_clone().unwrap()).unwrap(),
            4
        );
    }
}

#[cfg(test)]
mod region_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[tokio::test]
    async fn missing_region_provider_explains_available_alternatives() {
        let fixture = tempfile::tempdir().unwrap();
        let error = spectacle_region_using(&fixture.path().join("missing-spectacle"))
            .await
            .unwrap_err();
        let error = CaptureError::from(error);
        assert_eq!(error, CaptureError::SpectacleUnavailable);
        assert!(error.to_string().contains("Spectacle"));
        assert!(error.to_string().contains("Scan screen"));
        assert!(error.to_string().contains("Desktop capture"));
    }
    #[tokio::test]
    async fn region_capture_starts_empty_in_an_independent_temporary_config() {
        let fixture = tempfile::tempdir().unwrap();
        let script = fixture.path().join("spectacle");
        let arguments = fixture.path().join("arguments");
        let config_root = fixture.path().join("config-root");
        let config_copy = fixture.path().join("config-copy");
        std::fs::write(
            &script,
            format!(
                r#"#!/bin/sh
printf '%s\n' "$@" > '{}'
printf '%s' "$XDG_CONFIG_HOME" > '{}'
case "$XDG_CONFIG_HOME" in
    */qr-region-*/config) ;;
    *) exit 23 ;;
esac
cat "$XDG_CONFIG_HOME/spectaclerc" > '{}'
# Emulate Spectacle saving a selection; the next scan must not inherit it.
printf '[GuiConfig]\nselectionRect=20,30,100,200\n' > "$XDG_CONFIG_HOME/spectaclerc"
exit 0
"#,
                arguments.display(),
                config_root.display(),
                config_copy.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut previous_config = None;
        for _ in 0..2 {
            let outcome = spectacle_region_using(&script).await;
            let args = std::fs::read_to_string(&arguments).unwrap();
            assert!(
                args.lines().any(|arg| arg == "--new-instance"),
                "the private configuration must not be bypassed by D-Bus forwarding"
            );
            assert!(matches!(outcome.unwrap(), CaptureOutcome::Cancelled));
            let config = std::path::PathBuf::from(std::fs::read_to_string(&config_root).unwrap());
            assert_eq!(config.file_name().unwrap(), "config");
            assert!(
                !config.exists(),
                "temporary settings must be removed on cancellation"
            );
            assert_ne!(
                previous_config.as_ref(),
                Some(&config),
                "each scan needs fresh settings"
            );
            previous_config = Some(config);
            let contents = std::fs::read_to_string(&config_copy).unwrap();
            assert!(contents.contains("[General]\nrememberSelectionRect=Never\n"));
            assert!(contents.contains("[GuiConfig]\nselectionRect=0,0,0,0\n"));
        }
    }
    #[tokio::test]
    async fn region_capture_uses_release_selector_private_file_and_removes_it() {
        let fixture = tempfile::tempdir().unwrap();
        let image = fixture.path().join("fixture.png");
        image::DynamicImage::new_rgb8(2, 3).save(&image).unwrap();
        let script = fixture.path().join("spectacle");
        let receipt = fixture.path().join("arguments");
        std::fs::write(&script, format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nwhile [ \"$1\" != --output ]; do shift; done\nshift\ncp '{}' \"$1\"\n", receipt.display(), image.display())).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let outcome = spectacle_region_using(&script).await.unwrap();
        let CaptureOutcome::Captured(image) = outcome else {
            panic!("not captured");
        };
        assert_eq!((image.width(), image.height()), (2, 3));
        let arguments = std::fs::read_to_string(receipt).unwrap();
        let args: Vec<_> = arguments.lines().collect();
        assert_eq!(
            &args[..6],
            &[
                "--background",
                "--nonotify",
                "--region",
                "--release-capture",
                "--new-instance",
                "--output"
            ]
        );
        assert!(
            !std::path::Path::new(args[6]).parent().unwrap().exists(),
            "captured file must be removed after loading"
        );
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        assert!(matches!(
            spectacle_region_using(&script).await.unwrap(),
            CaptureOutcome::Cancelled
        ));
        std::fs::write(&script, "#!/bin/sh\nexit 2\n").unwrap();
        assert!(spectacle_region_using(&script).await.is_err());
    }
}
