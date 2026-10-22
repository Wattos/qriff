//! Portal hints are loaded once and updated by D-Bus signals; no idle polling.
use super::{Message, style};
use futures_util::{SinkExt, StreamExt};
pub fn events() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(4, async |mut output| {
        let Ok(settings) = ashpd::desktop::settings::Settings::new().await else {
            return;
        };
        // Subscribe before reading so a change during startup cannot be lost.
        let Ok(mut changes) = settings.receive_setting_changed().await else {
            return;
        };
        loop {
            let dark = matches!(
                settings.color_scheme().await,
                Ok(ashpd::desktop::settings::ColorScheme::PreferDark)
            );
            let accent = settings
                .accent_color()
                .await
                .map(|c| iced::Color::from_rgb(c.red() as f32, c.green() as f32, c.blue() as f32))
                .unwrap_or(style::BLUE);
            if output
                .send(Message::Appearance(dark, accent))
                .await
                .is_err()
            {
                break;
            }
            loop {
                match changes.next().await {
                    Some(setting) if setting.namespace() == "org.freedesktop.appearance" => break,
                    Some(_) => continue,
                    None => return,
                }
            }
        }
    })
}
