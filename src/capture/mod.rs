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
) -> ashpd::desktop::screenshot::ScreenshotOptions {
    use ashpd::desktop::screenshot::{AvailableTargets, ScreenshotOptions};
    let options = ScreenshotOptions::default()
        .set_interactive(mode != CaptureMode::Quick)
        .set_modal(false);
    if mode != CaptureMode::Quick && area_available {
        options.set_target(AvailableTargets::Area)
    } else {
        options
    }
}

/// Request a screenshot using the desktop portal.
pub async fn screenshot(mode: CaptureMode) -> anyhow::Result<CaptureOutcome> {
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
    if mode == CaptureMode::Region && !area_available {
        anyhow::bail!(
            "Direct region selection is unavailable on this desktop. Use Scan screen or Desktop capture instead."
        );
    }
    let options = screenshot_options(mode, area_available);
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
    let mut child = tokio::process::Command::new(executable)
        .args([
            "--background",
            "--nonotify",
            "--region",
            "--release-capture",
            "--output",
        ])
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| {
            anyhow::anyhow!("Spectacle is required for native region selection on KDE.")
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
            &screenshot_options(mode, area_available),
        )
        .unwrap();
        bytes.deserialize().unwrap().0
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
            &args[..5],
            &[
                "--background",
                "--nonotify",
                "--region",
                "--release-capture",
                "--output"
            ]
        );
        assert!(
            !std::path::Path::new(args[5]).parent().unwrap().exists(),
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
