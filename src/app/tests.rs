use super::*;
#[cfg(test)]
pub(super) mod capture_tests {
    use super::*;
    pub(crate) fn app() -> App {
        App {
            state: Default::default(),
            layout: crate::display::LayoutState::Unavailable,
            overlay: None,
            retired_overlays: Default::default(),
            overlay_closes: Vec::new(),
            deferred_action: None,
            capture_layout: None,
            pending_spatial: None,
            spatial_gate: Arc::new(tokio::sync::Semaphore::new(1)),
            spatial_abort: None,
            action_feedback: None,
            overlay_fallback: None,
            screen_results: false,
            windowless: false,
            clipboard_failed: false,
            result_texts: Default::default(),
            desktop: tokio::sync::watch::channel(Default::default()).0,
            focus: None,
            focus_ticket: 0,
            focus_ready: false,
            dark: false,
            accent: style::BLUE,
            width: 1020.0,
            more_open: false,
            settings_open: false,
            revealed: Default::default(),
            preferences: Default::default(),
            preferences_loaded: false,
            preference_path: None,
            preference_revision: 0,
            saving: None,
            settings_warning: None,
            auto_copy: Default::default(),
            copy_feedback: None,
            result_actions: Default::default(),
            action_token: 0,
            retained_result_session: None,
            primary: Arc::new(Mutex::new(None)),
            decoder: Arc::new(crate::jobs::Decoder::new()),
            screenshot: None,
            camera_preview: None,
            camera: None,
            retiring_cameras: Vec::new(),
            camera_devices: Vec::new(),
            selected_camera: None,
            camera_mode: false,
            selecting: false,
            crop_epoch: 0,
            pending_capture: None,
            capture_mode: crate::capture::CaptureMode::Quick,
            temporary_closes: Default::default(),
        }
    }
    #[test]
    fn reset_releases_capture_and_rejects_late_response() {
        let mut app = app();
        let old = app.state.begin_session();
        let image = Arc::new(crate::capture::Prepared::new(
            image::DynamicImage::new_rgb8(2, 2),
        ));
        app.screenshot = Some(image.clone());
        app.selecting = true;
        let id = app.reset_source();
        assert_ne!(id, old);
        assert!(app.screenshot.is_none());
        assert!(!app.selecting);
        let _ = app.update(Message::CaptureFinished(old, Ok(Some(image))));
        assert!(app.screenshot.is_none());
        assert!(app.state.busy);
    }
    #[test]
    fn camera_failure_cannot_be_overwritten_by_a_queued_decode() {
        let mut app = app();
        let id = app.state.begin_session();
        let _ = app.update(Message::CameraOutcome(Arc::new(crate::camera::Outcome {
            id,
            event: crate::camera::Event::Error("unplugged".into()),
        })));
        let _ = app.update(Message::DecodeFinished(Arc::new(
            crate::jobs::DecodeOutcome {
                id,
                result: Ok(vec![]),
            },
        )));
        assert_eq!(app.state.error.as_deref(), Some("Camera failed: unplugged"));
        assert!(!app.state.busy);
    }
    #[test]
    fn camera_frames_after_close_or_stop_cannot_restore_preview() {
        let mut app = app();
        let window = window::Id::unique();
        app.state.window_ids.push(window);
        app.state.windows = 1;
        app.state.host_available = true;
        let old = app.state.begin_session();
        let frame = Arc::new(crate::camera::Outcome {
            id: old,
            event: crate::camera::Event::Frame {
                preview: Some(iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4])),
                gray: Some(Arc::new(image::GrayImage::new(1, 1))),
            },
        });
        let _ = app.update(Message::CameraOutcome(frame.clone()));
        assert!(app.camera_preview.is_some());
        let _ = app.update(Message::WindowClosed(window));
        assert!(app.camera_preview.is_none());
        let _ = app.update(Message::CameraOutcome(frame));
        assert!(app.camera_preview.is_none());
        assert!(!app.state.busy);
    }
    #[test]
    fn close_releases_capture() {
        let mut app = app();
        app.state.host_available = true;
        app.state.windows = 1;
        let id = iced::window::Id::unique();
        app.state.window_ids.push(id);
        app.screenshot = Some(Arc::new(crate::capture::Prepared::new(
            image::DynamicImage::new_rgb8(2, 2),
        )));
        let _ = app.update(Message::WindowClosed(id));
        assert!(app.screenshot.is_none());
        assert!(!app.state.busy);
    }
}

#[cfg(test)]
mod temporary_capture_tests {
    use super::capture_tests::app;
    use super::*;
    #[test]
    fn intentional_last_window_close_preserves_capture_session() {
        let mut app = app();
        let window = window::Id::unique();
        app.state.window_ids.push(window);
        app.state.windows = 1;
        let _ = app.command(Command::ScanScreen);
        let session = app.state.session;
        assert!(app.temporary_closes.contains(&window));
        assert!(app.state.window_ids.is_empty());
        assert_eq!(app.state.windows, 0);
        let _ = app.update(Message::WindowClosed(window));
        assert_eq!(app.state.session, session);
        assert!(app.state.busy);
        assert!(app.temporary_closes.is_empty());
        assert_eq!(app.pending_capture, Some(crate::jobs::JobId(session)));
        let _ = app.update(Message::CaptureFinished(
            crate::jobs::JobId(session),
            Ok(None),
        ));
        assert!(!app.state.busy);
        assert_eq!(app.state.window_ids.len(), 1);
        assert!(app.pending_capture.is_none());
    }
    #[test]
    fn waits_for_every_close_and_restores_after_failure() {
        let mut app = app();
        let first = window::Id::unique();
        let second = window::Id::unique();
        app.state.window_ids = vec![first, second];
        app.state.windows = 2;
        let _ = app.command(Command::ScanScreen);
        let id = crate::jobs::JobId(app.state.session);
        assert_eq!(app.update(Message::WindowClosed(first)).units(), 0);
        assert_eq!(app.update(Message::TrayHostChanged(false)).units(), 0);
        assert_eq!(app.command(Command::Show).units(), 0);
        assert!(app.state.window_ids.is_empty());
        assert!(app.update(Message::WindowClosed(second)).units() > 0);
        assert_eq!(app.state.session, id.0);
        let _ = app.update(Message::CaptureFinished(id, Err("denied".into())));
        assert_eq!(
            app.state.error.as_deref(),
            Some("Could not capture the desktop.")
        );
        assert!(!app.state.busy);
        assert_eq!(app.state.window_ids.len(), 1);
    }
    #[test]
    fn source_switch_during_temporary_close_ignores_old_completion() {
        let mut app = app();
        let window = window::Id::unique();
        app.state.window_ids.push(window);
        app.state.windows = 1;
        let _ = app.command(Command::ScanScreen);
        let old = crate::jobs::JobId(app.state.session);
        let _ = app.command(Command::ScanCamera);
        let session = app.state.session;
        let _ = app.update(Message::WindowClosed(window));
        assert_eq!(app.state.session, session);
        assert_eq!(app.state.window_ids.len(), 1);
        let _ = app.update(Message::CaptureFinished(old, Err("old failure".into())));
        assert_ne!(app.state.error.as_deref(), Some("old failure"));
    }
}

#[cfg(test)]
mod final_review_tests {
    use super::capture_tests::app;
    use super::*;
    #[tokio::test]
    async fn captured_image_automatically_schedules_full_decode() {
        use iced::futures::StreamExt;
        let mut app = app();
        let id = app.state.begin_session();
        let image = Arc::new(crate::capture::Prepared::new(
            image::DynamicImage::new_rgb8(7, 9),
        ));
        let task = app.update(Message::CaptureFinished(id, Ok(Some(image.clone()))));
        assert!(Arc::ptr_eq(app.screenshot.as_ref().unwrap(), &image));
        let mut stream = iced_runtime::task::into_stream(task).unwrap();
        let mut scheduled = false;
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Output(message @ Message::DecodeRequested(_, _)) = action {
                if let Message::DecodeRequested(_, gray) = &message {
                    assert_eq!(gray.dimensions(), (7, 9));
                }
                let _ = app.update(message);
                scheduled = true;
            }
        }
        assert!(
            scheduled,
            "capture must schedule decoding without another click"
        );
        let outcome = app.decoder.subscribe().recv().await.unwrap();
        let _ = app.update(Message::DecodeFinished(outcome));
        assert!(app.state.scanned);
        assert!(!app.state.busy);
    }
    #[test]
    fn stop_retains_results_and_rejects_stale_camera_and_decode_work() {
        let mut app = app();
        let id = app.state.begin_session();
        app.camera_mode = true;
        app.camera_preview = Some(iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4]));
        let _ = app.update(Message::DecodeFinished(Arc::new(
            crate::jobs::DecodeOutcome {
                id,
                result: Ok(vec![crate::results::QrResult::from_payload(
                    b"keep".to_vec(),
                )]),
            },
        )));
        let _ = app.update(Message::StopCamera);
        assert_eq!(
            app.state.results.len(),
            1,
            "Stop must retain scanned results"
        );
        assert!(app.state.scanned);
        assert!(app.camera.is_none());
        assert!(app.camera_preview.is_none());
        assert!(!app.camera_mode);
        let _ = app.update(Message::CameraOutcome(Arc::new(crate::camera::Outcome {
            id,
            event: crate::camera::Event::Frame {
                preview: Some(iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4])),
                gray: Some(Arc::new(image::GrayImage::new(1, 1))),
            },
        })));
        let _ = app.update(Message::DecodeRequested(
            id,
            Arc::new(image::GrayImage::new(1, 1)),
        ));
        let _ = app.update(Message::DecodeFinished(Arc::new(
            crate::jobs::DecodeOutcome {
                id,
                result: Ok(vec![crate::results::QrResult::from_payload(
                    b"stale".to_vec(),
                )]),
            },
        )));
        assert_eq!(app.state.results.len(), 1);
        assert_eq!(app.state.results[0].payload, b"keep");
        assert!(app.camera_preview.is_none());
        assert!(!app.state.busy);
        let _ = app.command(Command::ScanCamera);
        assert!(app.state.results.is_empty());
    }
}

#[cfg(test)]
mod cancellation_feedback_tests {
    use super::*;
    #[test]
    fn cancelled_capture_has_distinct_status_cleared_by_next_operation() {
        let mut app = super::capture_tests::app();
        let id = app.state.begin_session();
        let _ = app.update(Message::CaptureFinished(id, Ok(None)));
        assert_eq!(app.status(), "Capture cancelled.");
        assert!(!app.state.busy);
        assert!(!app.state.scanned);
        assert!(app.state.error.is_none());
        let _ = app.command(Command::ScanCamera);
        assert_ne!(app.status(), "Capture cancelled.");
    }
}

#[cfg(test)]
mod quick_capture_tests {
    use super::capture_tests::app;
    use super::*;
    #[test]
    fn explicit_fallback_survives_window_close_then_next_scan_returns_to_quick() {
        let mut app = app();
        let window = window::Id::unique();
        app.state.window_ids.push(window);
        app.state.windows = 1;
        let _ = app.update(Message::DesktopCapture);
        let id = app.pending_capture.unwrap();
        assert_eq!(app.capture_mode, crate::capture::CaptureMode::Interactive);
        assert!(app.temporary_closes.contains(&window));
        assert!(app.update(Message::WindowClosed(window)).units() > 0);
        assert_eq!(app.capture_mode, crate::capture::CaptureMode::Interactive);
        let _ = app.update(Message::CaptureFinished(
            id,
            Err("Permission denied".into()),
        ));
        assert_eq!(
            app.state.error.as_deref(),
            Some("Could not capture the desktop.")
        );
        assert!(app.pending_capture.is_none());
        assert_eq!(app.state.window_ids.len(), 1);
        let _ = app.command(Command::ScanScreen);
        assert_eq!(app.capture_mode, crate::capture::CaptureMode::Quick);
    }
}

#[cfg(test)]
mod preference_tests {
    use super::*;
    #[test]
    fn toggle_during_load_wins_and_queued_saves_coalesce() {
        let mut app = super::capture_tests::app();
        assert_eq!(app.update(Message::AutoCopyChanged(true)).units(), 0);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert!(
            app.update(Message::PreferencesLoaded(
                crate::settings::LoadedPreferences {
                    preferences: Default::default(),
                    path: Some(path),
                    warning: None
                }
            ))
            .units()
                > 0
        );
        assert!(app.preferences.auto_copy_single);
        let first = app.saving.unwrap();
        assert_eq!(app.update(Message::AutoCopyChanged(false)).units(), 0);
        assert_eq!(app.update(Message::AutoCopyChanged(true)).units(), 0);
        assert_eq!(app.update(Message::AutoCopyChanged(false)).units(), 0);
        assert!(app.update(Message::PreferencesSaved(first, Ok(()))).units() > 0);
        assert_eq!(app.saving, Some(app.preference_revision));
        assert!(!app.preferences.auto_copy_single);
    }
    #[test]
    fn settings_warning_preserves_results_and_scan_error() {
        let mut app = super::capture_tests::app();
        app.state
            .results
            .push(crate::results::QrResult::from_payload(b"keep".to_vec()));
        app.state.error = Some("scan error".into());
        let _ = app.update(Message::PreferencesLoaded(
            crate::settings::LoadedPreferences {
                preferences: Default::default(),
                path: None,
                warning: Some("settings error".into()),
            },
        ));
        assert_eq!(app.settings_warning.as_deref(), Some("settings error"));
        assert_eq!(app.state.error.as_deref(), Some("scan error"));
        assert_eq!(app.state.results.len(), 1);
    }
    #[tokio::test]
    async fn accepted_single_requests_exact_copy_and_stale_or_duplicate_requests_none() {
        use iced::futures::StreamExt;
        let mut app = super::capture_tests::app();
        app.preferences.auto_copy_single = true;
        let id = app.state.begin_session();
        let outcome = Arc::new(crate::jobs::DecodeOutcome {
            id,
            result: Ok(vec![
                crate::results::QrResult::from_payload(b"WIFI:T:WPA;S:home;P:secret;;".to_vec()),
                crate::results::QrResult::from_payload(b"WIFI:T:WPA;S:home;P:secret;;".to_vec()),
            ]),
        });
        let task = app.update(Message::DecodeFinished(outcome.clone()));
        let mut stream = iced_runtime::task::into_stream(task).unwrap();
        let mut copied = false;
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Clipboard(iced_runtime::clipboard::Action::Write {
                contents,
                ..
            }) = action
            {
                assert_eq!(contents, "WIFI:T:WPA;S:home;P:secret;;");
                copied = true;
            }
        }
        assert!(copied);
        assert_eq!(
            app.update(Message::DecodeFinished(outcome.clone())).units(),
            0
        );
        assert_eq!(app.update(Message::AutoCopyChanged(true)).units(), 0);
        app.state.begin_session();
        assert_eq!(app.update(Message::DecodeFinished(outcome)).units(), 0);
    }
}

#[cfg(test)]
mod persistence_action_tests {
    use super::*;
    use iced::futures::StreamExt;
    async fn complete(task: Task<Message>) -> Message {
        let mut stream = iced_runtime::task::into_stream(task).unwrap();
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Output(message) = action {
                return message;
            }
        }
        panic!("save must report completion")
    }
    #[tokio::test]
    async fn serialized_writes_retain_last_toggle_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config/settings.json");
        let mut app = super::capture_tests::app();
        let _ = app.update(Message::PreferencesLoaded(
            crate::settings::LoadedPreferences {
                preferences: Default::default(),
                path: Some(path.clone()),
                warning: None,
            },
        ));
        let first = app.update(Message::AutoCopyChanged(true));
        let _ = app.update(Message::AutoCopyChanged(false));
        let _ = app.update(Message::AutoCopyChanged(true));
        let _ = app.update(Message::AutoCopyChanged(false));
        let message = complete(first).await;
        let last = app.update(message);
        let message = complete(last).await;
        assert_eq!(app.update(message).units(), 0);
        let saved: crate::settings::Preferences =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert!(!saved.auto_copy_single);
        assert!(app.saving.is_none());
    }
    #[tokio::test]
    async fn manual_copy_uses_exact_binary_text_and_multicode_never_autocopies() {
        let mut app = super::capture_tests::app();
        app.preferences.auto_copy_single = true;
        let id = app.state.begin_session();
        let binary = crate::results::QrResult::from_payload(vec![255, 0]);
        let task = app.update(Message::DecodeFinished(Arc::new(
            crate::jobs::DecodeOutcome {
                id,
                result: Ok(vec![
                    binary.clone(),
                    crate::results::QrResult::from_payload(b"other".to_vec()),
                ]),
            },
        )));
        assert_eq!(task.units(), 0);
        let mut stream =
            iced_runtime::task::into_stream(app.update(Message::CopyResult(0))).unwrap();
        let mut text = None;
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Clipboard(iced_runtime::clipboard::Action::Write {
                contents,
                ..
            }) = action
            {
                text = Some(contents);
            }
        }
        assert_eq!(text, Some(binary.display_text));
        assert_eq!(app.update(Message::AutoCopyChanged(true)).units(), 0);
    }
    #[test]
    fn save_failure_does_not_change_results_or_scan_error() {
        let mut app = super::capture_tests::app();
        app.state
            .results
            .push(crate::results::QrResult::from_payload(b"keep".to_vec()));
        app.state.error = Some("decode failure".into());
        app.saving = Some(3);
        app.preference_revision = 3;
        let _ = app.update(Message::PreferencesSaved(3, Err("disk full".into())));
        assert!(app.settings_warning.is_some());
        assert_eq!(app.state.results[0].payload, b"keep");
        assert_eq!(app.state.error.as_deref(), Some("decode failure"));
    }
}

#[cfg(test)]
mod explicit_action_tests {
    use super::*;
    // Advancing capture identity must not revive a pending action or lose its feedback.
    #[test]
    fn retained_camera_actions_survive_stop_and_failure() {
        for camera_error in [false, true] {
            for completion in [Ok(()), Err(crate::actions::ActionError::Authorization)] {
                let mut app = super::capture_tests::app();
                let id = app.reset_source();
                app.camera_mode = true;
                let payload = b"WIFI:T:nopass;S:home;;".to_vec();
                drop(app.update(Message::DecodeFinished(Arc::new(
                    crate::jobs::DecodeOutcome {
                        id,
                        result: Ok(vec![crate::results::QrResult::from_payload(
                            payload.clone(),
                        )]),
                    },
                ))));
                let task = app.update(Message::ActivateResult(0));
                assert!(task.units() > 0);
                drop(task); // Never poll the platform action.
                let pending = app.result_actions[&0].clone();
                if camera_error {
                    drop(
                        app.update(Message::CameraOutcome(Arc::new(crate::camera::Outcome {
                            id,
                            event: crate::camera::Event::Error("unplugged".into()),
                        }))),
                    );
                } else {
                    drop(app.update(Message::StopCamera));
                }
                assert_ne!(app.state.session, id.0);
                assert_eq!(app.state.results[0].payload, payload);
                assert_eq!(app.update(Message::ActivateResult(0)).units(), 0);
                assert_eq!(app.result_actions[&0].token, pending.token);
                drop(app.update(Message::DecodeFinished(Arc::new(
                    crate::jobs::DecodeOutcome {
                        id,
                        result: Ok(vec![crate::results::QrResult::from_payload(
                            b"stale".to_vec(),
                        )]),
                    },
                ))));
                assert_eq!(app.state.results.len(), 1);
                drop(app.update(Message::ActionFinished(
                    pending.key,
                    pending.token,
                    completion.clone(),
                )));
                match completion {
                    Ok(()) => assert!(matches!(
                        app.result_actions[&0].status,
                        crate::actions::ActionStatus::Succeeded
                    )),
                    Err(error) => assert!(
                        matches!(&app.result_actions[&0].status, crate::actions::ActionStatus::Failed(actual) if *actual == error)
                    ),
                }
                let scan_error = camera_error.then_some("Camera failed: unplugged");
                assert_eq!(app.state.error.as_deref(), scan_error);
                drop(app.update(Message::StopCamera));
                assert!(!matches!(
                    app.result_actions[&0].status,
                    crate::actions::ActionStatus::Pending
                ));
                let fresh = app.reset_source();
                drop(app.update(Message::DecodeFinished(Arc::new(
                    crate::jobs::DecodeOutcome {
                        id: fresh,
                        result: Ok(vec![crate::results::QrResult::from_payload(payload)]),
                    },
                ))));
                drop(app.update(Message::ActivateResult(0)));
                let token = app.result_actions[&0].token;
                drop(app.update(Message::ActionFinished(pending.key, pending.token, Ok(()))));
                assert_eq!(app.result_actions[&0].token, token);
                assert!(matches!(
                    app.result_actions[&0].status,
                    crate::actions::ActionStatus::Pending
                ));
            }
        }
    }
    #[test]
    fn copied_feedback_rejects_stale_session_and_out_of_range_index() {
        let mut app = super::capture_tests::app();
        let old = app.reset_source();
        let current = app.reset_source();
        app.state
            .results
            .push(crate::results::QrResult::from_payload(b"current".to_vec()));
        app.state.error = Some("scan error".into());
        drop(app.update(Message::Copied(current, 0)));
        assert_eq!(app.copy_feedback, Some((current, 0)));
        drop(app.update(Message::Copied(old, 0)));
        drop(app.update(Message::Copied(current, 1)));
        assert_eq!(app.copy_feedback, Some((current, 0)));
        assert_eq!(app.state.results[0].payload, b"current");
        assert_eq!(app.state.error.as_deref(), Some("scan error"));
    }
    #[test]
    fn click_only_duplicate_pending_and_stale_outcomes() {
        let mut app = super::capture_tests::app();
        let id = app.state.begin_session();
        let _ = app.update(Message::DecodeFinished(Arc::new(
            crate::jobs::DecodeOutcome {
                id,
                result: Ok(vec![crate::results::QrResult::from_payload(
                    b"https://example.org".to_vec(),
                )]),
            },
        )));
        assert!(app.result_actions.is_empty());
        let _ = app.update(Message::CopyResult(0));
        assert!(app.result_actions.is_empty());
        assert!(app.update(Message::ActivateResult(0)).units() > 0);
        let first = app.result_actions[&0].token;
        assert_eq!(app.update(Message::ActivateResult(0)).units(), 0);
        let key = crate::actions::ActionKey {
            session: id,
            result_index: 0,
        };
        app.state.error = Some("scan error".into());
        app.settings_warning = Some("settings warning".into());
        let _ = app.update(Message::ActionFinished(
            key,
            first,
            Err(crate::actions::ActionError::Authorization),
        ));
        assert!(matches!(
            app.result_actions[&0].status,
            crate::actions::ActionStatus::Failed(_)
        ));
        assert_eq!(app.state.results.len(), 1);
        assert!(app.update(Message::CopyResult(0)).units() > 0);
        assert!(app.update(Message::ActivateResult(0)).units() > 0);
        let next = app.result_actions[&0].token;
        let _ = app.update(Message::ActionFinished(key, first, Ok(())));
        assert_eq!(app.result_actions[&0].token, next);
        assert!(matches!(
            app.result_actions[&0].status,
            crate::actions::ActionStatus::Pending
        ));
        assert_eq!(app.state.error.as_deref(), Some("scan error"));
        assert_eq!(app.settings_warning.as_deref(), Some("settings warning"));
        app.reset_source();
        let _ = app.update(Message::ActionFinished(key, next, Ok(())));
        assert!(app.result_actions.is_empty());
    }
    #[test]
    fn all_typed_actions_require_explicit_click_and_plain_text_is_inert() {
        for payload in [
            "https://example.org",
            "tel:+123456",
            "WIFI:T:nopass;S:home;;",
        ] {
            let mut app = super::capture_tests::app();
            app.state
                .results
                .push(crate::results::QrResult::from_payload(
                    payload.as_bytes().to_vec(),
                ));
            assert!(app.result_actions.is_empty());
            assert!(app.update(Message::ActivateResult(0)).units() > 0);
        }
        let mut app = super::capture_tests::app();
        app.state
            .results
            .push(crate::results::QrResult::from_payload(b"123456".to_vec()));
        assert_eq!(app.update(Message::ActivateResult(0)).units(), 0);
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    fn breakpoint_includes_760_and_stacks_below() {
        assert!(!view::wide(759.0));
        assert!(view::wide(760.0));
    }
    #[test]
    fn wifi_credentials_hidden_in_valid_and_invalid_cards_until_explicit_reveal() {
        let valid = crate::results::QrResult::from_payload(
            b"WIFI:T:WPA;S:Guest;P:private-fixture;;".to_vec(),
        );
        assert_eq!(view::payload(&valid, false), "Guest");
        let invalid =
            crate::results::QrResult::from_payload(b"WIFI:T:WPA;P:private-fixture;;".to_vec());
        assert!(!view::payload(&invalid, false).contains("private-fixture"));
        assert!(view::payload(&invalid, true).contains("private-fixture"));
    }
    #[test]
    fn source_change_hides_secrets_and_closes_more_after_desktop_command() {
        let mut app = capture_tests::app();
        let _ = app.update(Message::RevealPassword(0));
        assert!(app.revealed.contains(&0));
        app.reset_source();
        assert!(app.revealed.is_empty());
        let _ = app.update(Message::ToggleMore);
        assert!(app.more_open);
        let _ = app.update(Message::DesktopCapture);
        assert!(!app.more_open);
        assert!(matches!(
            app.capture_mode,
            crate::capture::CaptureMode::Interactive
        ));
    }
    #[test]
    fn menus_are_exclusive_and_copy_does_not_replace_scan_error() {
        let mut app = capture_tests::app();
        let _ = app.update(Message::ToggleMore);
        let _ = app.update(Message::ToggleSettings);
        assert!(!app.more_open && app.settings_open);
        app.state.error = Some("capture failure".into());
        app.state
            .results
            .push(crate::results::QrResult::from_payload(b"safe".to_vec()));
        let _ = app.update(Message::Copied(crate::jobs::JobId(app.state.session), 0));
        assert_eq!(app.state.error.as_deref(), Some("capture failure"));
        assert!(app.copy_feedback.is_some());
    }
}

#[test]
fn escape_dismisses_menus_without_discarding_results() {
    let mut app = capture_tests::app();
    app.state
        .results
        .push(crate::results::QrResult::from_payload(b"retained".to_vec()));
    let _ = app.update(Message::ToggleSettings);
    let _ = app.update(Message::CancelCrop);
    assert!(!app.settings_open);
    let _ = app.update(Message::ToggleMore);
    let _ = app.update(Message::CancelCrop);
    assert!(!app.more_open);
    assert_eq!(app.state.results.len(), 1);
}

#[test]
fn keyboard_focus_cycles_visible_controls_and_activates_menu_without_os_action() {
    let mut app = capture_tests::app();
    for expected in [
        focus::Target::Scan,
        focus::Target::Camera,
        focus::Target::More,
    ] {
        let _ = app.update(Message::FocusNext(false));
        assert_eq!(app.focus, Some(expected));
    }
    let _ = app.update(Message::FocusVisible(
        app.state.session,
        app.focus_ticket,
        true,
    ));
    let _ = app.update(Message::KeyboardActivate);
    assert!(app.more_open);
    assert!(focus::targets(&app).contains(&focus::Target::Desktop));
    app.focus = Some(focus::Target::Scan);
    let _ = app.update(Message::FocusNext(true));
    assert_eq!(app.focus, Some(focus::Target::AutoCopy));
    app.reset_source();
    assert_eq!(app.focus, None);
}
#[test]
fn captured_keyboard_input_does_not_activate_scanner_controls() {
    use iced::{
        event::Status,
        keyboard::{Key, key::Named},
    };
    assert!(focus::key_message(&Key::Named(Named::Enter), false, Status::Captured).is_none());
    assert!(matches!(
        focus::key_message(&Key::Named(Named::Tab), true, Status::Ignored),
        Some(Message::FocusNext(true))
    ));
    assert!(matches!(
        focus::key_message(&Key::Named(Named::Enter), false, Status::Ignored),
        Some(Message::KeyboardActivate)
    ));
}

#[test]
fn offscreen_keyboard_target_cannot_activate_before_visibility_is_processed() {
    let mut app = capture_tests::app();
    for _ in 0..8 {
        app.state
            .results
            .push(crate::results::QrResult::from_payload(
                b"https://example.org/".to_vec(),
            ));
    }
    app.focus = Some(focus::Target::Copy(6));
    let _ = app.update(Message::FocusNext(false));
    assert_eq!(app.focus, Some(focus::Target::Activate(7)));
    let _ = app.update(Message::KeyboardActivate);
    assert!(
        app.result_actions.is_empty(),
        "an offscreen action must wait for visibility"
    );
    let _ = app.update(Message::FocusVisible(
        app.state.session,
        app.focus_ticket,
        true,
    ));
    let _ = app.update(Message::KeyboardActivate); // Task deliberately dropped: no OS action.
    assert!(app.result_actions.contains_key(&7));
    assert!(!app.result_actions.contains_key(&6));
}

#[test]
fn keyboard_visibility_completion_is_session_and_traversal_aware() {
    let mut app = capture_tests::app();
    for _ in 0..8 {
        app.state
            .results
            .push(crate::results::QrResult::from_payload(
                b"https://example.org/".to_vec(),
            ));
    }
    app.focus = Some(focus::Target::Copy(6));
    let _ = app.update(Message::FocusNext(false));
    let stale = app.focus_ticket;
    let _ = app.update(Message::FocusNext(false));
    assert_eq!(app.focus, Some(focus::Target::Copy(7)));
    let _ = app.update(Message::FocusVisible(app.state.session, stale, true));
    assert!(!app.focus_ready);
    let _ = app.update(Message::FocusVisible(
        app.state.session,
        app.focus_ticket,
        true,
    ));
    assert!(app.focus_ready);
    assert!(matches!(
        app.focus.unwrap().message(false),
        Message::CopyResult(7)
    ));
    let _ = app.update(Message::ScrollInput);
    assert!(!app.focus_ready);
    let _ = app.update(Message::KeyboardActivate);
    assert!(app.copy_feedback.is_none());
    assert!(app.result_actions.is_empty());
}

#[test]
fn delayed_preference_warning_requires_new_visibility_before_keyboard_action() {
    for delayed_load in [false, true] {
        let mut app = capture_tests::app();
        for _ in 0..8 {
            app.state
                .results
                .push(crate::results::QrResult::from_payload(
                    b"https://example.org/".to_vec(),
                ));
        }
        app.focus = Some(focus::Target::Copy(6));
        let _ = app.update(Message::FocusNext(false));
        let confirmed_ticket = app.focus_ticket;
        let _ = app.update(Message::FocusVisible(
            app.state.session,
            confirmed_ticket,
            true,
        ));
        assert!(app.focus_ready);
        if delayed_load {
            let _ = app.update(Message::PreferencesLoaded(
                crate::settings::LoadedPreferences {
                    preferences: Default::default(),
                    path: None,
                    warning: Some("Settings unavailable".into()),
                },
            ));
        } else {
            app.preference_revision = 5;
            app.saving = Some(5);
            let _ = app.update(Message::PreferencesSaved(
                5,
                Err("permission denied".into()),
            ));
        }
        assert!(app.settings_warning.is_some());
        assert!(
            !app.focus_ready,
            "new statusbar warning shrinks the confirmed viewport"
        );
        let _ = app.update(Message::FocusVisible(
            app.state.session,
            confirmed_ticket,
            true,
        ));
        assert!(
            !app.focus_ready,
            "previous geometry cannot authorize activation"
        );
        let _ = app.update(Message::KeyboardActivate);
        assert!(
            app.result_actions.is_empty(),
            "Enter must only request visibility"
        );
        let _ = app.update(Message::FocusVisible(
            app.state.session,
            app.focus_ticket,
            true,
        ));
        let _ = app.update(Message::KeyboardActivate); // Drop returned future: no OS action.
        assert!(app.result_actions.contains_key(&7));
        assert_eq!(app.result_actions.len(), 1);
    }
}

#[test]
fn camera_readiness_changes_on_preview_insertion_but_not_replacement_frames() {
    let mut app = capture_tests::app();
    app.camera_mode = true;
    app.width = 560.;
    app.focus_ready = true;
    let frame = || {
        Message::CameraOutcome(Arc::new(crate::camera::Outcome {
            id: crate::jobs::JobId(0),
            event: crate::camera::Event::Frame {
                preview: Some(iced::widget::image::Handle::from_rgba(
                    1,
                    1,
                    vec![0, 0, 0, 255],
                )),
                gray: None,
            },
        }))
    };
    let _ = app.update(frame());
    assert!(
        !app.focus_ready,
        "inserting the preview shifts the narrow results"
    );
    app.focus_ready = true;
    let ticket = app.focus_ticket;
    for _ in 0..20 {
        let _ = app.update(frame());
    }
    assert!(
        app.focus_ready,
        "fixed-size preview replacements must not starve keyboard activation"
    );
    assert_eq!(app.focus_ticket, ticket);
    let _ = app.update(Message::CameraOutcome(Arc::new(crate::camera::Outcome {
        id: crate::jobs::JobId(0),
        event: crate::camera::Event::Stopped,
    })));
    assert!(!app.focus_ready);
}

#[test]
fn synchronous_settings_warning_invalidates_confirmed_viewport() {
    let mut app = capture_tests::app();
    app.preferences_loaded = true;
    app.focus_ready = true;
    let confirmed = app.focus_ticket;
    let _ = app.update(Message::AutoCopyChanged(true)); // No config path: no disk task.
    assert!(app.settings_warning.is_some());
    assert!(!app.focus_ready);
    assert_ne!(app.focus_ticket, confirmed);
}

#[test]
fn tray_screen_capture_never_opens_results_window() {
    let mut app = capture_tests::app();
    app.state.host_available = true;
    let _ = app.update(Message::InstanceCommand(Command::ScanScreen));
    let id = crate::jobs::JobId(app.state.session);
    let image = Arc::new(crate::capture::Prepared::new(
        image::DynamicImage::new_rgb8(2, 2),
    ));
    let _ = app.update(Message::CaptureFinished(id, Ok(Some(image))));
    assert!(
        app.state.window_ids.is_empty(),
        "tray capture must remain windowless"
    );
}

#[test]
fn selectable_payload_is_read_only_and_hides_secrets_again() {
    use iced::widget::text_editor::{Action, Edit};
    let mut app = capture_tests::app();
    let id = app.reset_source();
    let result =
        crate::results::QrResult::from_payload(b"WIFI:T:WPA;S:My network;P:verysecret;;".to_vec());
    let _ = app.update(Message::DecodeFinished(Arc::new(
        crate::jobs::DecodeOutcome {
            id,
            result: Ok(vec![result]),
        },
    )));
    assert_eq!(app.result_texts[&0].text(), "My network");
    let _ = app.update(Message::SelectText(id, 0, Action::SelectAll));
    assert_eq!(
        app.result_texts[&0].selection().as_deref(),
        Some("My network")
    );
    let _ = app.update(Message::SelectText(id, 0, Action::Edit(Edit::Insert('x'))));
    assert_eq!(app.result_texts[&0].text(), "My network");
    let _ = app.update(Message::RevealPassword(0));
    assert!(app.result_texts[&0].text().contains("verysecret"));
    let _ = app.update(Message::RevealPassword(0));
    assert!(!app.result_texts[&0].text().contains("verysecret"));
    assert!(app.result_texts[&0].selection().is_none());
    let old = id;
    let id = app.reset_source();
    let result = crate::results::QrResult::from_payload(b"new result".to_vec());
    let _ = app.update(Message::DecodeFinished(Arc::new(
        crate::jobs::DecodeOutcome {
            id,
            result: Ok(vec![result]),
        },
    )));
    let _ = app.update(Message::SelectText(old, 0, Action::SelectAll));
    assert!(app.result_texts[&0].selection().is_none());
}

#[test]
fn tray_results_notify_and_reject_old_buttons_without_os_actions() {
    let mut app = capture_tests::app();
    app.state.host_available = true;
    let _ = app.update(Message::InstanceCommand(Command::ScanScreen));
    let capture = crate::jobs::JobId(app.state.session);
    let image = Arc::new(crate::capture::Prepared::new(
        image::DynamicImage::new_rgb8(2, 2),
    ));
    drop(app.update(Message::CaptureFinished(capture, Ok(Some(image)))));
    let id = crate::jobs::JobId(app.state.session);
    let _ = app.update(Message::SpatialFinished(
        id,
        0,
        Ok(Arc::new(crate::decode::SpatialScan {
            results: vec![crate::results::QrResult::from_payload(
                b"https://example.org".to_vec(),
            )],
            detections: vec![],
        })),
    ));
    let snapshot = app.desktop.borrow().clone();
    assert_eq!(snapshot.notification.unwrap().title, "QR code found");
    assert_eq!(snapshot.results[0].action, Some("Open URL"));
    assert!(app.state.window_ids.is_empty());
    let _ = app.update(Message::InstanceCommand(Command::ScanScreen));
    let rejected = app.update(Message::InstanceCommand(Command::ActivateResult(id.0, 0)));
    assert_eq!(rejected.units(), 0);
    assert!(app.result_actions.is_empty());
    let _ = app.update(Message::CaptureFinished(
        crate::jobs::JobId(app.state.session),
        Err("capture error".into()),
    ));
    assert!(app.state.window_ids.is_empty());
    assert_eq!(
        app.desktop.borrow().notification.as_ref().unwrap().title,
        "QR scan failed"
    );
}

#[test]
fn camera_keeps_window_and_tray_region_uses_native_capture() {
    let mut app = capture_tests::app();
    let task = app.update(Message::InstanceCommand(Command::ScanCamera));
    drop(task); // enumeration never polled
    assert!(!app.windowless);
    assert_eq!(app.state.window_ids.len(), 1);
    let task = app.update(Message::InstanceCommand(Command::ScanRegion));
    drop(task);
    assert!(app.windowless);
    assert_eq!(app.capture_mode, crate::capture::CaptureMode::Region);
    assert!(app.state.window_ids.is_empty());
    let session = app.state.session;
    drop(app.update(Message::InstanceCommand(Command::ScanRegion)));
    assert_eq!(
        app.state.session, session,
        "duplicate clicks must not spawn another selector"
    );
}

#[test]
fn multiple_action_failures_retain_desktop_feedback() {
    let mut app = capture_tests::app();
    let id = app.reset_source();
    let _ = app.update(Message::DecodeFinished(Arc::new(
        crate::jobs::DecodeOutcome {
            id,
            result: Ok(vec![
                crate::results::QrResult::from_payload(b"https://example.org".to_vec()),
                crate::results::QrResult::from_payload(b"tel:+41123456789".to_vec()),
            ]),
        },
    )));
    for index in 0..2 {
        drop(app.update(Message::ActivateResult(index)));
        let action = &app.result_actions[&index];
        let _ = app.update(Message::ActionFinished(
            action.key,
            action.token,
            Err(if index == 0 {
                crate::actions::ActionError::Authorization
            } else {
                crate::actions::ActionError::NoPhoneHandler
            }),
        ));
    }
    let snapshot = app.desktop.borrow();
    assert_eq!(snapshot.results.len(), 2);
    assert_eq!(
        snapshot.results[0].feedback.as_deref(),
        Some("Permission to connect was denied. Try network settings.")
    );
    assert_eq!(
        snapshot.results[1].feedback.as_deref(),
        Some("No telephone app is configured. Copy the number to use it elsewhere.")
    );
}

#[test]
fn malformed_wifi_hidden_buffer_rejects_queued_selection() {
    use iced::widget::text_editor::Action;
    let mut app = capture_tests::app();
    let id = app.reset_source();
    let _ = app.update(Message::DecodeFinished(Arc::new(
        crate::jobs::DecodeOutcome {
            id,
            result: Ok(vec![crate::results::QrResult::from_payload(
                b"WIFI:T:WPA;P:secret;;".to_vec(),
            )]),
        },
    )));
    assert!(!app.result_texts[&0].text().contains("secret"));
    let _ = app.update(Message::RevealPassword(0));
    assert!(app.result_texts[&0].text().contains("secret"));
    let _ = app.update(Message::SelectText(id, 0, Action::SelectAll));
    let _ = app.update(Message::RevealPassword(0));
    let _ = app.update(Message::SelectText(id, 0, Action::SelectAll));
    assert!(!app.result_texts[&0].text().contains("secret"));
    assert!(app.result_texts[&0].selection().is_none());
}
