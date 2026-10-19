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
}

fn screenshot_options(
    mode: CaptureMode,
    area_available: bool,
) -> ashpd::desktop::screenshot::ScreenshotOptions {
    use ashpd::desktop::screenshot::{AvailableTargets, ScreenshotOptions};
    let options = ScreenshotOptions::default()
        .set_interactive(mode == CaptureMode::Interactive)
        .set_modal(false);
    if mode == CaptureMode::Interactive && area_available {
        options.set_target(AvailableTargets::Area)
    } else {
        options
    }
}

/// Request a screenshot using the desktop portal.
pub async fn screenshot(mode: CaptureMode) -> anyhow::Result<CaptureOutcome> {
    use ashpd::desktop::screenshot::{AvailableTargets, ScreenshotProxy};
    let proxy = ScreenshotProxy::new().await?;
    let area_available = mode == CaptureMode::Interactive
        && proxy.version() >= 3
        && proxy
            .available_targets()
            .await
            .is_ok_and(|targets| targets.contains(AvailableTargets::Area));
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
