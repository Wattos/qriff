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
/// Interactive v2 fallback; request an area only when v3 explicitly advertises it.
pub async fn screenshot() -> anyhow::Result<CaptureOutcome> {
    use ashpd::desktop::screenshot::{AvailableTargets, ScreenshotOptions, ScreenshotProxy};
    let proxy = ScreenshotProxy::new().await?;
    let mut options = ScreenshotOptions::default()
        .set_interactive(true)
        .set_modal(false);
    if proxy.version() >= 3
        && proxy
            .available_targets()
            .await
            .is_ok_and(|targets| targets.contains(AvailableTargets::Area))
    {
        options = options.set_target(AvailableTargets::Area);
    }
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
