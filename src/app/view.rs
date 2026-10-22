use super::{
    App, Message, focus,
    focus::Target,
    icons::{Icon, icon},
    style,
};
use crate::{
    cli::Command,
    results::{ContentKind, QrContent, WifiSecurity},
};
use iced::{
    Element, Length,
    widget::{button, checkbox, column, container, row, scrollable, text},
};

pub(super) fn wide(width: f32) -> bool {
    width >= 760.0
}
fn focus_style(
    app: &App,
    target: Target,
    primary: bool,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style + use<> {
    let focused = app.focus == Some(target);
    move |theme, status| style::focused(theme, status, primary, focused)
}
fn secondary<'a>(app: &App, label: &'a str, target: Target) -> iced::widget::Button<'a, Message> {
    button(text(label).size(13))
        .padding([8, 12])
        .style(focus_style(app, target, false))
}
fn auto_copy(app: &App, target: Target) -> Element<'_, Message> {
    let focused = app.focus == Some(target);
    container(
        checkbox(app.preferences.auto_copy_single)
            .label("Automatically copy single results")
            .size(15)
            .text_size(12)
            .on_toggle(Message::AutoCopyChanged),
    )
    .padding(3)
    .style(move |theme| style::focus_ring(theme, focused))
    .into()
}
/// Display text deliberately excludes encoded Wi-Fi credentials, including malformed Wi-Fi.
pub(super) fn payload(result: &crate::results::QrResult, reveal: bool) -> String {
    match &result.content {
        QrContent::Wifi(network) => network.ssid.clone(),
        QrContent::Phone { number, .. } => number.clone(),
        QrContent::Invalid {
            kind: ContentKind::Wifi,
            ..
        } if !reveal => "Encoded Wi-Fi details hidden".into(),
        _ => result.display_text.clone(),
    }
}
fn card(app: &App, index: usize) -> Element<'_, Message> {
    let result = &app.state.results[index];
    let revealed = app.revealed.contains(&index);
    let (label, caption) = match &result.content {
        QrContent::Url(_) => ("Website", "Web link"),
        QrContent::Phone { .. } => ("Phone number", "Telephone link"),
        QrContent::Wifi(_) => ("Wi-Fi network", "Network connection"),
        QrContent::Text => ("Text", "QR content"),
        QrContent::Binary => ("Binary", "Raw bytes · hexadecimal"),
        QrContent::Invalid { .. } => ("Invalid content", "Copy is still available"),
    };
    let glyph = match result.content {
        QrContent::Url(_) => Icon::Link,
        QrContent::Phone { .. } => Icon::Phone,
        QrContent::Wifi(_) => Icon::Wifi,
        _ => Icon::Text,
    };
    let mut body = column![
        row![
            container(icon(glyph)).padding(9).style(style::icon_tile),
            column![
                text(label).size(14),
                text(caption).size(11).style(style::muted)
            ]
            .spacing(3)
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center),
        text(payload(result, revealed))
            .size(if matches!(result.content, QrContent::Phone { .. }) {
                21
            } else {
                16
            })
            .style(if matches!(result.content, QrContent::Url(_)) {
                style::accent
            } else {
                iced::widget::text::default
            })
    ]
    .spacing(10);
    if let QrContent::Wifi(network) = &result.content {
        let security = match &network.security {
            WifiSecurity::Open => "Open".to_owned(),
            WifiSecurity::WpaPersonal => "WPA / WPA2 Personal".into(),
            WifiSecurity::Sae => "WPA3 / SAE".into(),
            WifiSecurity::Unsupported(s) => format!("Unsupported: {s}"),
        };
        body = body.push(
            text(format!(
                "Security     {security}{}",
                if network.hidden {
                    " · Hidden network"
                } else {
                    ""
                }
            ))
            .size(12),
        );
        if let Some(password) = &network.password {
            body = body.push(
                row![
                    text(format!(
                        "Password   {}",
                        if revealed {
                            password.as_str()
                        } else {
                            "••••••••••••"
                        }
                    ))
                    .size(12)
                    .width(Length::Fill),
                    focus::anchor(
                        Target::Reveal(index),
                        secondary(
                            app,
                            if revealed { "Hide" } else { "Show" },
                            Target::Reveal(index)
                        )
                        .on_press(Message::RevealPassword(index))
                    )
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center),
            );
        }
        if matches!(network.security, WifiSecurity::Unsupported(_)) {
            body = body.push(text("This security type cannot be connected here. Copy the code to use it elsewhere.").size(12));
        }
    }
    if let QrContent::Invalid { kind, reason } = &result.content {
        body = body.push(text(reason).size(12));
        if matches!(kind, ContentKind::Wifi) {
            body = body.push(focus::anchor(
                Target::Reveal(index),
                secondary(
                    app,
                    if revealed {
                        "Hide encoded details"
                    } else {
                        "Show encoded details"
                    },
                    Target::Reveal(index),
                )
                .on_press(Message::RevealPassword(index)),
            ));
        }
    }
    let mut actions = row![].spacing(8);
    if let Some(action) = result.action() {
        let pending = app.result_actions.get(&index).is_some_and(|s| {
            s.key.session.0 == app.state.session
                && matches!(s.status, crate::actions::ActionStatus::Pending)
        });
        let label = match action {
            crate::results::ResultAction::OpenUrl(_) => "Open URL",
            crate::results::ResultAction::Call(_) => "Call number",
            crate::results::ResultAction::ConnectWifi(_) => "Connect to Wi-Fi",
        };
        let mut b = button(text(if pending { "Working…" } else { label }).size(13))
            .padding([8, 12])
            .style(focus_style(app, Target::Activate(index), true));
        if !pending {
            b = b.on_press(Message::ActivateResult(index));
        }
        actions = actions.push(focus::anchor(Target::Activate(index), b));
    }
    body = body.push(
        actions.push(focus::anchor(
            Target::Copy(index),
            button(
                row![icon(Icon::Copy), text("Copy").size(13)]
                    .spacing(7)
                    .align_y(iced::Alignment::Center),
            )
            .padding([8, 12])
            .style(focus_style(app, Target::Copy(index), false))
            .on_press(Message::CopyResult(index)),
        )),
    );
    if let Some(state) = app
        .result_actions
        .get(&index)
        .filter(|s| s.key.session.0 == app.state.session)
    {
        match &state.status {
            crate::actions::ActionStatus::Failed(e) => {
                body = body.push(text(e.to_string()).size(12))
            }
            crate::actions::ActionStatus::Succeeded => {
                body = body.push(
                    text(if matches!(result.content, QrContent::Wifi(_)) {
                        "Connected. Connection removed when disconnected."
                    } else {
                        "Opened in the desktop app."
                    })
                    .size(12),
                )
            }
            _ => {}
        }
    }
    container(body)
        .padding(18)
        .width(Length::Fill)
        .style(style::card)
        .into()
}
fn source(app: &App) -> Element<'_, Message> {
    let mut pane = column![
        text(if app.camera_mode {
            "Camera"
        } else if app.screenshot.is_some() {
            "Captured screen"
        } else {
            "Capture source"
        })
        .size(14)
    ]
    .spacing(14);
    if app.camera_mode {
        pane = pane.push(
            iced::widget::pick_list(
                app.camera_devices.as_slice(),
                app.selected_camera.as_ref(),
                Message::SelectCamera,
            )
            .placeholder("Select camera")
            .width(Length::Fill),
        );
        if let Some(handle) = &app.camera_preview {
            pane = pane.push(
                iced::widget::image(handle.clone())
                    .height(220)
                    .width(Length::Fill)
                    .content_fit(iced::ContentFit::Contain),
            );
        }
        pane = pane.push(focus::anchor(
            Target::Stop,
            secondary(app, "Stop camera", Target::Stop).on_press(Message::StopCamera),
        ));
    } else if let Some(image) = &app.screenshot {
        pane = pane.push(
            container(
                iced::widget::canvas(crate::capture::preview::Preview {
                    handle: image.handle.clone(),
                    size: (image.image.width(), image.image.height()),
                    session: crate::jobs::JobId(app.state.session),
                    selecting: app.selecting,
                    epoch: app.crop_epoch,
                })
                .width(Length::Fill)
                .height(if wide(app.width) { 220 } else { 140 }),
            )
            .style(style::card),
        );
        pane = pane.push(
            text(format!(
                "{} × {} pixels",
                image.image.width(),
                image.image.height()
            ))
            .size(11)
            .style(style::muted),
        );
        pane = pane.push(
            row![
                focus::anchor(
                    Target::Region,
                    secondary(app, "Select region", Target::Region).on_press(Message::SelectRegion)
                ),
                focus::anchor(
                    Target::Rescan,
                    secondary(app, "Rescan", Target::Rescan).on_press(Message::ScanFull)
                )
            ]
            .spacing(6),
        );
        if wide(app.width) || app.selecting {
            pane = pane.push(text(if app.selecting {"Drag a rectangle in the image. Escape cancels selection."} else {"Need a specific code?\nSelect a region in the captured image to narrow the scan."}).size(12));
        }
    } else {
        pane = pane.push(
            container(
                column![
                    text("Ready to scan").size(20),
                    text("Capture your screen or use a camera to find QR codes.").size(13)
                ]
                .spacing(12),
            )
            .padding(24)
            .height(210)
            .center_y(210)
            .width(Length::Fill)
            .style(style::card),
        );
        pane = pane.push(text("Scan screen captures immediately. Desktop capture in More lets you choose a screen or window.").size(12));
    }
    if wide(app.width) {
        pane = pane.push(
            text("Processed locally on your device")
                .size(11)
                .style(style::muted),
        );
    }
    container(pane)
        .padding(if wide(app.width) { 22 } else { 16 })
        .width(Length::Fill)
        .style(style::chrome)
        .into()
}
pub(super) fn view(app: &App, _window_id: iced::window::Id) -> Element<'_, Message> {
    let capture_tools = row![
        button(
            row![icon(Icon::Scan), text("Scan screen").size(13)]
                .spacing(8)
                .align_y(iced::Alignment::Center)
        )
        .padding([9, 14])
        .style(focus_style(app, Target::Scan, true))
        .on_press(Message::UserCommand(Command::ScanScreen)),
        button(
            row![icon(Icon::Camera), text("Camera").size(13)]
                .spacing(8)
                .align_y(iced::Alignment::Center)
        )
        .padding([8, 12])
        .style(focus_style(app, Target::Camera, false))
        .on_press(Message::UserCommand(Command::ScanCamera))
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);
    let options = row![
        secondary(app, "More ▾", Target::More).on_press(Message::ToggleMore),
        iced::widget::Space::new().width(Length::Fill),
        button(
            row![icon(Icon::Settings), text("Settings").size(13)]
                .spacing(8)
                .align_y(iced::Alignment::Center)
        )
        .padding([8, 12])
        .style(focus_style(app, Target::Settings, false))
        .on_press(Message::ToggleSettings)
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);
    let toolbar: Element<'_, Message> = if app.width < 540.0 {
        column![capture_tools, options].spacing(8).into()
    } else {
        row![capture_tools, options]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into()
    };
    let mut main = column![
        container(toolbar).padding([12, 16]).style(style::chrome),
        iced::widget::rule::horizontal(1)
    ];
    if app.more_open {
        main = main.push(
            container(
                row![
                    secondary(app, "Desktop capture…", Target::Desktop)
                        .on_press(Message::DesktopCapture),
                    secondary(app, "Quit", Target::Quit)
                        .on_press(Message::UserCommand(Command::Quit))
                ]
                .spacing(8),
            )
            .padding([10, 16])
            .style(style::chrome),
        );
    }
    if app.settings_open {
        main = main.push(container(column![text("Scan preferences").size(15), auto_copy(app, Target::SettingsAutoCopy), text("Copy when a scan finds one QR code. With several codes, choose a result to copy.\nOpening links, calling and connecting to Wi-Fi always require a click.").size(12)].spacing(12)).padding(18).width(Length::Fill).style(style::card));
    }
    let count = app.state.results.len();
    let mut results = column![
        row![
            text("Scan results").size(21),
            iced::widget::Space::new().width(Length::Fill),
            text(format!(
                "{count} QR {}",
                if count == 1 { "code" } else { "codes" }
            ))
            .size(12)
        ]
        .align_y(iced::Alignment::Center)
    ]
    .spacing(16);
    if count == 0 {
        results = results.push(container(column![text(if app.state.busy {"Scanning…"} else if app.state.error.is_some() {"Scan failed"} else if app.state.capture_cancelled {"Capture cancelled"} else if app.state.scanned {"No QR codes found"} else {"Your results will appear here"}).size(20), text(if app.state.scanned {"Try another image or select a region around a code."} else {"Choose Scan screen or Camera to get started. QR content stays on your device."}).size(13)].spacing(12)).padding(24).width(Length::Fill).style(style::card));
    }
    for index in 0..count {
        results = results.push(card(app, index));
    }
    if count > 1 {
        results = results.push(text("Several codes found. Choose a result to copy.").size(12));
    }
    let results: Element<'_, Message> = if wide(app.width) {
        scrollable(results)
            .id(focus::RESULTS_SCROLL)
            .on_scroll(|_| Message::ScrollInput)
            .spacing(12)
            .height(Length::Fill)
            .into()
    } else {
        results.into()
    };
    let results = container(results).padding(24).width(Length::Fill);
    if wide(app.width) {
        main = main.push(
            row![
                container(
                    scrollable(source(app))
                        .id(focus::SOURCE_SCROLL)
                        .on_scroll(|_| Message::ScrollInput)
                        .spacing(6)
                )
                .width(Length::FillPortion(36))
                .height(Length::Fill)
                .style(style::chrome),
                container(results)
                    .width(Length::FillPortion(64))
                    .height(Length::Fill)
            ]
            .height(Length::Fill),
        );
    } else {
        main = main.push(
            scrollable(column![source(app), results.height(Length::Shrink)].spacing(0))
                .id(focus::OUTER_SCROLL)
                .on_scroll(|_| Message::ScrollInput)
                .height(Length::Fill),
        );
    }
    let mut feedback = column![text(app.status()).size(12)]
        .spacing(4)
        .width(Length::Fill);
    if app
        .copy_feedback
        .is_some_and(|(session, _)| session.0 == app.state.session)
    {
        feedback = feedback.push(text("Copied to clipboard").size(12));
    }
    if let Some(warning) = &app.settings_warning {
        feedback = feedback.push(text(warning).size(12));
    }
    let status: Element<'_, Message> = if wide(app.width) {
        row![
            feedback,
            iced::widget::Space::new().width(Length::Fill),
            auto_copy(app, Target::AutoCopy)
        ]
        .align_y(iced::Alignment::Center)
        .spacing(12)
        .into()
    } else {
        column![feedback, auto_copy(app, Target::AutoCopy)]
            .spacing(8)
            .into()
    };
    main.push(iced::widget::rule::horizontal(1))
        .push(
            container(status)
                .padding([10, 16])
                .width(Length::Fill)
                .style(style::chrome),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
