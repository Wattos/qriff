use super::tests::capture_tests::app;
use super::*;

#[test]
fn screen_decode_never_copies_before_explicit_selection() {
    let mut app = app();
    app.preferences.auto_copy_single = true;
    app.windowless = true;
    let id = app.state.begin_session();
    let task = app.update(Message::DecodeFinished(Arc::new(
        crate::jobs::DecodeOutcome {
            id,
            result: Ok(vec![crate::results::QrResult::from_payload(
                b"secret".to_vec(),
            )]),
        },
    )));
    assert_eq!(
        task.units(),
        0,
        "screen detection must not write the clipboard"
    );
}

#[test]
fn capture_failures_never_retain_provider_details() {
    let mut app = app();
    app.state.host_available = true;
    drop(app.update(Message::InstanceCommand(Command::ScanScreen)));
    let id = crate::jobs::JobId(app.state.session);
    drop(app.update(Message::CaptureFinished(
        id,
        Err("private provider details".into()),
    )));
    assert!(!app.state.error.as_ref().unwrap().contains("private"));
}

#[test]
fn repeated_scan_during_spatial_work_cannot_replace_the_pending_capture() {
    let mut app = app();
    app.state.host_available = true;
    drop(app.update(Message::InstanceCommand(Command::ScanScreen)));
    let id = crate::jobs::JobId(app.state.session);
    drop(app.update(Message::CaptureFinished(
        id,
        Ok(Some(Arc::new(crate::capture::Prepared::new(
            image::DynamicImage::new_rgb8(20, 20),
        )))),
    )));
    let decoding = app.state.session;
    drop(app.update(Message::InstanceCommand(Command::ScanScreen)));
    assert_eq!(
        app.state.session, decoding,
        "one tray request stays active through decode"
    );
}

fn layout(revision: u64) -> crate::display::LayoutState {
    crate::display::LayoutState::Ready(crate::display::Layout {
        revision,
        outputs: vec![crate::display::OutputGeometry {
            name: "synthetic".into(),
            logical_position: [0, 0],
            logical_size: [1829, 1143],
            pixel_size: [2560, 1600],
            transform: 0,
        }],
    })
}
fn scan() -> Arc<crate::decode::SpatialScan> {
    Arc::new(crate::decode::SpatialScan {
        results: vec![crate::results::QrResult::from_payload(
            b"https://example.org".to_vec(),
        )],
        detections: vec![crate::decode::Detection {
            result_index: 0,
            corners: [[20., 20.], [120., 20.], [120., 120.], [20., 120.]],
        }],
    })
}
fn captured(app: &mut App, mode: Command) -> crate::jobs::JobId {
    app.state.host_available = true;
    drop(app.update(Message::InstanceCommand(mode)));
    let id = crate::jobs::JobId(app.state.session);
    drop(app.update(Message::CaptureFinished(
        id,
        Ok(Some(Arc::new(crate::capture::Prepared::new(
            image::DynamicImage::new_rgb8(2560, 1600),
        )))),
    )));
    id
}
fn opened() -> (App, crate::jobs::JobId, window::Id) {
    let mut app = app();
    drop(app.update(Message::LayoutChanged(layout(7))));
    let session = captured(&mut app, Command::ScanScreen);
    drop(app.update(Message::SpatialFinished(session, 7, Ok(scan()))));
    let window = app.overlay.as_ref().unwrap().id;
    drop(app.update(Message::WindowResized(
        window,
        iced::Size::new(1829., 1143.),
    )));
    (app, session, window)
}

#[test]
fn spatial_results_open_only_overlay_and_require_a_validated_surface_before_hits() {
    let mut app = app();
    drop(app.update(Message::LayoutChanged(layout(7))));
    let session = captured(&mut app, Command::ScanScreen);
    drop(app.update(Message::SpatialFinished(session, 7, Ok(scan()))));
    assert!(app.state.window_ids.is_empty());
    assert_eq!(app.state.windows, 0);
    let id = app.overlay.as_ref().expect("screen result overlay").id;
    drop(app.update(Message::OverlayEvent(
        session,
        id,
        crate::overlay::Event::Select(0),
    )));
    assert_eq!(app.overlay.as_ref().unwrap().model.selected_result(), None);
    drop(app.update(Message::WindowResized(id, iced::Size::new(1829., 1143.))));
    drop(app.update(Message::OverlayEvent(
        session,
        id,
        crate::overlay::Event::Select(0),
    )));
    assert_eq!(
        app.overlay.as_ref().unwrap().model.selected_result(),
        Some(0)
    );
    assert_eq!(
        app.overlay.as_ref().unwrap().model.focus(),
        Some(crate::overlay::Focus::Text)
    );
}

#[test]
fn unsupported_or_changed_layout_keeps_last_scan_without_overlay() {
    for kind in 0..5 {
        let mut app = app();
        if kind != 0 {
            drop(app.update(Message::LayoutChanged(layout(7))));
        }
        let id = captured(
            &mut app,
            if kind == 4 {
                Command::ScanRegion
            } else {
                Command::ScanScreen
            },
        );
        if kind == 0 {
            drop(app.update(Message::LayoutChanged(layout(7))));
        }
        if kind == 1 {
            drop(app.update(Message::LayoutChanged(layout(8))));
        }
        if kind == 2 {
            drop(app.update(Message::LayoutChanged(
                crate::display::LayoutState::Unavailable,
            )));
        }
        if kind == 3 {
            let crate::display::LayoutState::Ready(mut multi) = layout(8) else {
                unreachable!()
            };
            multi.outputs.push(multi.outputs[0].clone());
            drop(
                app.update(Message::LayoutChanged(crate::display::LayoutState::Ready(
                    multi,
                ))),
            );
        }
        drop(app.update(Message::SpatialFinished(
            id,
            if kind == 0 { 0 } else { 7 },
            Ok(scan()),
        )));
        assert!(app.overlay.is_none(), "case {kind}");
        assert!(app.state.window_ids.is_empty());
        assert_eq!(app.desktop.borrow().results.len(), 1);
        assert!(
            app.desktop
                .borrow()
                .notification
                .as_ref()
                .unwrap()
                .body
                .contains("Last scan")
        );
    }
}

#[test]
fn bad_resize_or_output_change_dismisses_without_resetting_results() {
    for change in [false, true] {
        let (mut app, session, id) = opened();
        let width = app.width;
        if change {
            drop(app.update(Message::LayoutChanged(layout(8))));
        } else {
            drop(app.update(Message::WindowResized(id, iced::Size::new(800., 600.))));
        }
        assert!(app.overlay.is_none());
        assert_eq!(app.width, width);
        drop(app.update(Message::WindowClosed(id)));
        assert_eq!(app.result_session(), session);
        assert_eq!(app.desktop.borrow().results.len(), 1);
        assert!(app.state.window_ids.is_empty());
    }
}

#[test]
fn action_registers_pending_then_dispatches_only_after_matching_close() {
    let (mut app, session, id) = opened();
    drop(app.update(Message::OverlayEvent(
        session,
        id,
        crate::overlay::Event::Select(0),
    )));
    let close = app.update(Message::OverlayEvent(
        session,
        id,
        crate::overlay::Event::Activate,
    ));
    assert!(close.units() > 0);
    drop(close);
    assert!(app.overlay.is_none());
    assert!(app.deferred_action.is_some());
    let token = app.result_actions[&0].token;
    assert_eq!(app.update(Message::ActivateResult(0)).units(), 0);
    assert_eq!(
        app.update(Message::WindowClosed(window::Id::unique()))
            .units(),
        0
    );
    assert!(app.deferred_action.is_some());
    let action = app.update(Message::WindowClosed(id));
    assert!(action.units() > 0); // Never poll an external action task.
    drop(action);
    assert!(app.deferred_action.is_none());
    assert_eq!(app.result_session(), session);
    assert_eq!(app.result_actions[&0].token, token);
    drop(app.update(Message::ActionFinished(
        app.result_actions[&0].key,
        token,
        Ok(()),
    )));
    assert!(matches!(
        app.result_actions[&0].status,
        crate::actions::ActionStatus::Succeeded
    ));
    assert_eq!(app.update(Message::WindowClosed(id)).units(), 0);
}

#[test]
fn source_reset_cancels_deferred_action_and_old_overlay_callbacks() {
    let (mut app, session, id) = opened();
    drop(app.update(Message::OverlayEvent(
        session,
        id,
        crate::overlay::Event::Select(0),
    )));
    drop(app.update(Message::ActivateResult(0))); // Last scan action must use same close path.
    assert!(app.deferred_action.is_some());
    drop(app.update(Message::InstanceCommand(Command::ScanCamera)));
    assert!(app.deferred_action.is_none());
    assert_eq!(app.update(Message::WindowClosed(id)).units(), 0);
    assert_eq!(
        app.update(Message::OverlayEvent(
            session,
            id,
            crate::overlay::Event::Copy
        ))
        .units(),
        0
    );
    assert_eq!(
        app.update(Message::InstanceCommand(Command::ActivateResult(
            session.0, 0
        )))
        .units(),
        0
    );
    assert!(app.result_actions.is_empty());
    assert_eq!(app.state.window_ids.len(), 1);
}

#[test]
fn capture_waits_for_both_old_overlay_and_normal_windows_to_close() {
    let (mut app, _, overlay) = opened();
    drop(app.open());
    let normal = app.state.window_ids[0];
    drop(app.update(Message::InstanceCommand(Command::ScanScreen)));
    assert!(app.overlay.is_none());
    assert_eq!(app.update(Message::WindowClosed(normal)).units(), 0);
    assert!(app.update(Message::WindowClosed(overlay)).units() > 0); // capture task unpolled
    assert!(app.temporary_closes.is_empty());
}

#[test]
fn captured_overlay_escape_is_not_replayed_by_the_global_key_handler() {
    let (mut app, session, id) = opened();
    drop(app.update(Message::OverlayEvent(
        session,
        id,
        crate::overlay::Event::Select(0),
    )));
    drop(app.update(Message::OverlayEvent(
        session,
        id,
        crate::overlay::Event::Escape,
    )));
    drop(app.update(Message::WindowKey(
        id,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
        false,
        iced::event::Status::Captured,
    )));
    assert!(app.overlay.is_some());
    assert_eq!(app.overlay.as_ref().unwrap().model.selected_result(), None);
    drop(app.update(Message::OverlayEvent(
        session,
        id,
        crate::overlay::Event::Escape,
    )));
    assert!(app.overlay.is_none());
}

#[test]
fn obsolete_payload_only_callbacks_cannot_finish_spatial_work() {
    let mut app = app();
    let session = captured(&mut app, Command::ScanScreen);
    drop(app.update(Message::DecodeFinished(Arc::new(
        crate::jobs::DecodeOutcome {
            id: session,
            result: Ok(scan().results.clone()),
        },
    ))));
    assert!(app.state.results.is_empty());
    assert!(app.state.busy);
}

#[test]
fn duplicate_capture_completion_cannot_reopen_a_dismissed_scan() {
    let (mut app, session, id) = opened();
    drop(app.update(Message::OverlayEvent(
        session,
        id,
        crate::overlay::Event::Close,
    )));
    drop(app.update(Message::WindowClosed(id)));
    let task = app.update(Message::CaptureFinished(
        session,
        Ok(Some(Arc::new(crate::capture::Prepared::new(
            image::DynamicImage::new_rgb8(2560, 1600),
        )))),
    ));
    assert_eq!(task.units(), 0);
    assert!(app.screenshot.is_none());
    assert!(app.pending_spatial.is_none());
    assert_eq!(app.state.results.len(), 1);
}

#[test]
fn close_then_open_window_restores_only_masked_selectable_text() {
    use iced::widget::text_editor::Action;
    let (mut app, session, id) = opened();
    let wifi =
        crate::results::QrResult::from_payload(b"WIFI:T:WPA;S:home;P:synthetic-secret;;".to_vec());
    app.state.results = vec![wifi];
    drop(app.update(Message::RevealPassword(0)));
    drop(app.update(Message::SelectText(session, 0, Action::SelectAll)));
    assert!(app.result_texts[&0].text().contains("synthetic-secret"));
    drop(app.update(Message::OverlayEvent(
        session,
        id,
        crate::overlay::Event::Close,
    )));
    drop(app.update(Message::WindowClosed(id)));
    drop(app.update(Message::InstanceCommand(Command::Show)));
    assert!(app.revealed.is_empty());
    assert_eq!(
        app.result_texts
            .get(&0)
            .expect("restored read-only text")
            .text(),
        "home"
    );
    assert!(app.result_texts[&0].selection().is_none());
}

#[test]
fn tray_loss_during_action_close_retains_last_scan_and_restores_accessible_window() {
    let (mut app, session, id) = opened();
    drop(app.update(Message::ActivateResult(0)));
    drop(app.update(Message::TrayHostChanged(false)));
    assert_eq!(app.state.window_ids.len(), 1);
    assert_eq!(app.result_session(), session);
    assert!(app.deferred_action.is_some());
    assert!(app.update(Message::WindowClosed(id)).units() > 0);
    assert_eq!(app.desktop.borrow().results.len(), 1);
}

#[test]
fn cancellation_empty_and_failed_decode_never_create_overlay() {
    for kind in 0..3 {
        let mut app = app();
        drop(app.update(Message::LayoutChanged(layout(7))));
        app.state.host_available = true;
        if kind == 0 {
            drop(app.update(Message::InstanceCommand(Command::ScanScreen)));
            let id = crate::jobs::JobId(app.state.session);
            drop(app.update(Message::CaptureFinished(id, Ok(None))));
            assert!(app.state.capture_cancelled);
        } else {
            let id = captured(&mut app, Command::ScanScreen);
            let result = if kind == 1 {
                Ok(Arc::new(crate::decode::SpatialScan {
                    results: vec![],
                    detections: vec![],
                }))
            } else {
                Err("private decoder details".into())
            };
            drop(app.update(Message::SpatialFinished(id, 7, result)));
        }
        assert!(app.overlay.is_none());
        assert!(app.screenshot.is_none());
        assert!(app.state.window_ids.is_empty());
        assert!(
            app.state
                .error
                .as_ref()
                .is_none_or(|error| !error.contains("private"))
        );
    }
}

#[tokio::test]
async fn selection_auto_copy_and_manual_copy_use_exact_payload_without_os_writes() {
    use iced::futures::StreamExt;
    let (mut app, session, id) = opened();
    app.preferences.auto_copy_single = true;
    for (event, expect_copy) in [
        (crate::overlay::Event::Select(0), true),
        (crate::overlay::Event::Select(0), false),
        (crate::overlay::Event::Copy, true),
    ] {
        let mut copied = vec![];
        let task =
            app.handle_overlay_event_with_copy(session, id, event, |app, text, session, index| {
                app.copy_task_with(text, session, index, |text| {
                    copied.push(text);
                    async { Ok(()) }
                })
            });
        if let Some(mut stream) = iced_runtime::task::into_stream(task) {
            while let Some(action) = stream.next().await {
                assert!(
                    !matches!(action, iced_runtime::Action::Clipboard(_)),
                    "temporary overlay clipboard must survive its window"
                );
            }
        }
        assert_eq!(
            copied,
            if expect_copy {
                vec!["https://example.org"]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn old_copy_completion_does_not_mark_new_overlay_and_dismissal_keeps_current_feedback() {
    let (mut app, old, id) = opened();
    drop(app.update(Message::OverlayEvent(old, id, crate::overlay::Event::Close)));
    drop(app.update(Message::WindowClosed(id)));
    drop(app.update(Message::Copied(old, 0)));
    assert_eq!(app.copy_feedback, Some((old, 0)));
    let session = captured(&mut app, Command::ScanScreen);
    drop(app.update(Message::SpatialFinished(session, 7, Ok(scan()))));
    drop(app.update(Message::ClipboardFinished(
        old,
        0,
        Err(clipboard::ClipboardUnavailable),
    )));
    assert_eq!(app.copy_feedback, None);
    assert!(!app.clipboard_failed);
}

#[tokio::test]
async fn source_reset_aborts_unstarted_spatial_work_and_releases_its_capture() {
    use iced::futures::StreamExt;
    let mut app = app();
    app.state.host_available = true;
    drop(app.update(Message::InstanceCommand(Command::ScanScreen)));
    let id = crate::jobs::JobId(app.state.session);
    let image = Arc::new(crate::capture::Prepared::new(
        image::DynamicImage::new_rgb8(20, 20),
    ));
    let weak = Arc::downgrade(&image);
    let task = app.update(Message::CaptureFinished(id, Ok(Some(image))));
    app.reset_source();
    let mut stream = iced_runtime::task::into_stream(task).unwrap();
    assert!(
        stream.next().await.is_none(),
        "cancelled source must not start a decoder"
    );
    assert!(weak.upgrade().is_none());
}

#[test]
fn multiple_results_still_notify_explicit_action_failure() {
    let (mut app, _, id) = opened();
    app.state
        .results
        .push(crate::results::QrResult::from_payload(
            b"tel:+41441234567".to_vec(),
        ));
    drop(app.update(Message::ActivateResult(1)));
    let state = app.result_actions[&1].clone();
    drop(app.update(Message::WindowClosed(id))); // returned external task never polled
    drop(app.update(Message::ActionFinished(
        state.key,
        state.token,
        Err(crate::actions::ActionError::NoPhoneHandler),
    )));
    let snapshot = app.desktop.borrow();
    assert_eq!(
        snapshot.notification.as_ref().unwrap().title,
        "QR action failed"
    );
    assert!(
        snapshot
            .notification
            .as_ref()
            .unwrap()
            .body
            .contains("telephone")
    );
    assert!(
        snapshot.results[1]
            .feedback
            .as_ref()
            .unwrap()
            .contains("telephone")
    );
}

#[tokio::test]
async fn spatial_decode_waits_for_running_worker_and_source_reset_cancels_waiter() {
    use iced::futures::StreamExt;
    let mut app = app();
    app.state.host_available = true;
    drop(app.update(Message::InstanceCommand(Command::ScanScreen)));
    let id = crate::jobs::JobId(app.state.session);
    let held = app.spatial_gate.clone().acquire_owned().await.unwrap();
    let task = app.update(Message::CaptureFinished(
        id,
        Ok(Some(Arc::new(crate::capture::Prepared::new(
            image::DynamicImage::new_rgb8(20, 20),
        )))),
    ));
    let mut stream = iced_runtime::task::into_stream(task).unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(30), stream.next())
            .await
            .is_err(),
        "second spatial worker must not run before the first releases its permit"
    );
    app.reset_source();
    drop(held);
    assert!(stream.next().await.is_none());
}

#[test]
fn open_window_dismisses_overlay_without_discarding_last_scan() {
    let (mut app, session, _) = opened();
    drop(app.update(Message::InstanceCommand(Command::Show)));
    assert!(app.overlay.is_none());
    assert_eq!(app.state.window_ids.len(), 1);
    assert_eq!(app.result_session(), session);
    assert_eq!(app.state.results.len(), 1);
}

#[tokio::test]
async fn partial_copy_uses_windowless_adapter_for_subset_and_survives_dismissal() {
    use iced::{
        futures::StreamExt,
        widget::text_editor::{Action, Motion},
    };
    let (mut app, session, id) = opened();
    drop(app.update(Message::OverlayEvent(
        session,
        id,
        crate::overlay::Event::Select(0),
    )));
    drop(app.update(Message::OverlayEvent(
        session,
        id,
        crate::overlay::Event::Text(Action::Move(Motion::DocumentStart)),
    )));
    for _ in 0..5 {
        drop(app.update(Message::OverlayEvent(
            session,
            id,
            crate::overlay::Event::Text(Action::Select(Motion::Right)),
        )));
    }
    let mut copied = vec![];
    let copy = app.handle_overlay_event_with_copy(
        session,
        id,
        crate::overlay::Event::CopySelection,
        |app, text, session, index| {
            app.copy_task_with(text, session, index, |text| {
                copied.push(text);
                async { Ok(()) }
            })
        },
    );
    assert_eq!(copied, ["https"]);
    drop(app.update(Message::OverlayEvent(
        session,
        id,
        crate::overlay::Event::Close,
    )));
    drop(app.update(Message::WindowClosed(id)));
    // The fake writer task completes after teardown; no runtime clipboard action
    // or external process is executed by this test.
    let mut stream = iced_runtime::task::into_stream(copy).unwrap();
    while let Some(action) = stream.next().await {
        assert!(!matches!(action, iced_runtime::Action::Clipboard(_)));
        if let iced_runtime::Action::Output(message) = action {
            drop(app.update(message));
        }
    }
    assert_eq!(app.copy_feedback, Some((session, 0)));
    app.reset_source();
    let stale = app.handle_overlay_event_with_copy(
        session,
        id,
        crate::overlay::Event::CopySelection,
        |_, _, _, _| panic!("stale selection must not reach clipboard adapter"),
    );
    assert_eq!(stale.units(), 0);
}

#[test]
fn hiding_malformed_wifi_destroys_controller_selection_copy_source() {
    use iced::widget::text_editor::Action;
    let mut app = app();
    drop(app.update(Message::LayoutChanged(layout(7))));
    let session = captured(&mut app, Command::ScanScreen);
    let mut scan = (*scan()).clone();
    scan.results[0] =
        crate::results::QrResult::from_payload(b"WIFI:T:bad;S:home;P:synthetic-secret;;".to_vec());
    drop(app.update(Message::SpatialFinished(session, 7, Ok(Arc::new(scan)))));
    let id = app.overlay.as_ref().unwrap().id;
    drop(app.update(Message::WindowResized(
        id,
        iced::Size::new(1829.2858, 1142.8572),
    )));
    for event in [
        crate::overlay::Event::Select(0),
        crate::overlay::Event::Reveal,
        crate::overlay::Event::Text(Action::SelectAll),
    ] {
        drop(app.update(Message::OverlayEvent(session, id, event)));
    }
    assert!(
        app.overlay
            .as_ref()
            .unwrap()
            .model
            .selected_text()
            .unwrap()
            .contains("synthetic-secret")
    );
    drop(app.update(Message::OverlayEvent(
        session,
        id,
        crate::overlay::Event::Reveal,
    )));
    drop(app.handle_overlay_event_with_copy(
        session,
        id,
        crate::overlay::Event::CopySelection,
        |_, _, _, _| panic!("hiding must erase prior raw selection"),
    ));
    assert_eq!(app.overlay.as_ref().unwrap().model.selected_text(), None);
    let snapshot = format!("{:?}", app.desktop.borrow());
    assert!(!snapshot.contains("synthetic-secret"));
}

#[tokio::test]
async fn captured_synthetic_qr_runs_spatial_worker_and_releases_image_on_close() {
    use iced::futures::StreamExt;
    let mut app = app();
    let qr = qrcode::QrCode::new(b"synthetic payload")
        .unwrap()
        .render::<image::Luma<u8>>()
        .module_dimensions(8, 8)
        .build();
    let size = [qr.width(), qr.height()];
    drop(
        app.update(Message::LayoutChanged(crate::display::LayoutState::Ready(
            crate::display::Layout {
                revision: 3,
                outputs: vec![crate::display::OutputGeometry {
                    name: "fixture".into(),
                    logical_position: [0, 0],
                    logical_size: size,
                    pixel_size: size,
                    transform: 0,
                }],
            },
        ))),
    );
    app.state.host_available = true;
    drop(app.update(Message::InstanceCommand(Command::ScanScreen))); // portal task never polled
    let session = crate::jobs::JobId(app.state.session);
    let image = Arc::new(crate::capture::Prepared::new(
        image::DynamicImage::ImageLuma8(qr),
    ));
    let weak = Arc::downgrade(&image);
    let task = app.update(Message::CaptureFinished(session, Ok(Some(image))));
    let mut stream = iced_runtime::task::into_stream(task).unwrap();
    let Some(iced_runtime::Action::Output(message @ Message::SpatialFinished(_, 3, Ok(_)))) =
        stream.next().await
    else {
        panic!("spatial decode result")
    };
    drop(app.update(message)); // native open task never polled
    assert_eq!(app.state.results[0].payload, b"synthetic payload");
    assert!(app.state.window_ids.is_empty());
    let id = app.overlay.as_ref().unwrap().id;
    drop(app.update(Message::OverlayEvent(
        session,
        id,
        crate::overlay::Event::Close,
    )));
    drop(stream);
    assert!(
        weak.upgrade().is_none(),
        "dismiss releases frozen desktop pixels"
    );
    assert_eq!(app.desktop.borrow().results.len(), 1);
}

#[test]
fn keyboard_enter_selects_before_it_can_explicitly_activate_an_action() {
    let (mut app, session, id) = opened();
    drop(app.update(Message::OverlayEvent(
        session,
        id,
        crate::overlay::Event::ActivateFocused,
    )));
    assert!(app.result_actions.is_empty());
    for event in [
        crate::overlay::Event::Next(false),
        crate::overlay::Event::ActivateFocused,
    ] {
        drop(app.update(Message::OverlayEvent(session, id, event)));
    }
    assert_eq!(
        app.overlay.as_ref().unwrap().model.selected_result(),
        Some(0)
    );
    assert!(app.result_actions.is_empty());
    for event in [
        crate::overlay::Event::Next(false),
        crate::overlay::Event::Next(false),
        crate::overlay::Event::ActivateFocused,
    ] {
        drop(app.update(Message::OverlayEvent(session, id, event)));
    }
    assert!(app.overlay.is_none());
    assert!(app.deferred_action.is_some());
    assert!(matches!(
        app.result_actions[&0].status,
        crate::actions::ActionStatus::Pending
    ));
}

#[test]
fn capture_size_mismatch_and_stale_spatial_callbacks_cannot_create_hit_surfaces() {
    let mut app = app();
    drop(app.update(Message::LayoutChanged(layout(7))));
    app.state.host_available = true;
    drop(app.update(Message::InstanceCommand(Command::ScanScreen)));
    let old = crate::jobs::JobId(app.state.session);
    drop(app.update(Message::CaptureFinished(
        old,
        Ok(Some(Arc::new(crate::capture::Prepared::new(
            image::DynamicImage::new_rgb8(1280, 800),
        )))),
    )));
    assert_eq!(
        app.update(Message::SpatialFinished(old, 6, Ok(scan())))
            .units(),
        0
    );
    assert!(app.state.busy);
    drop(app.update(Message::SpatialFinished(old, 7, Ok(scan()))));
    assert!(app.overlay.is_none());
    assert_eq!(app.state.results.len(), 1);
    let current = captured(&mut app, Command::ScanScreen);
    drop(app.update(Message::SpatialFinished(old, 7, Ok(scan()))));
    assert!(app.state.results.is_empty());
    drop(app.update(Message::SpatialFinished(current, 7, Ok(scan()))));
    let id = app.overlay.as_ref().unwrap().id;
    drop(app.update(Message::WindowResized(
        id,
        iced::Size::new(1829.2858, 1142.8572),
    )));
    assert!(app.overlay.as_ref().unwrap().ready);
}

#[test]
fn retired_overlay_that_finishes_opening_late_is_closed_before_next_capture() {
    let (mut app, _, id) = opened();
    drop(app.update(Message::InstanceCommand(Command::ScanScreen)));
    // The first close may have raced native creation. Its later size-query
    // response proves the retired native window now exists and needs closing.
    let close = app.update(Message::WindowResized(
        id,
        iced::Size::new(1829.2858, 1142.8572),
    ));
    assert!(close.units() > 0);
    assert!(app.temporary_closes.contains(&id));
    assert!(app.overlay.is_none());
    assert!(app.update(Message::WindowClosed(id)).units() > 0); // capture task unpolled
    assert!(app.temporary_closes.is_empty());
}

#[test]
fn normal_window_that_finishes_opening_during_capture_shutdown_is_closed_again() {
    let mut app = app();
    drop(app.update(Message::InstanceCommand(Command::Show)));
    let id = app.state.window_ids[0];
    drop(app.update(Message::InstanceCommand(Command::ScanScreen)));
    assert!(app.update(Message::Opened(id)).units() > 0);
    assert!(app.temporary_closes.contains(&id));
}
