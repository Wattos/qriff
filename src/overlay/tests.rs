use super::*;
use crate::{decode::Detection, results::QrResult};
fn model(payloads: &[&str], indexes: &[usize]) -> Model {
    let image = Arc::new(Prepared::new(image::DynamicImage::new_rgba8(400, 200)));
    Model::new(
        JobId(7),
        3,
        image,
        SpatialScan {
            results: payloads
                .iter()
                .map(|p| QrResult::from_payload(p.as_bytes().to_vec()))
                .collect(),
            detections: indexes
                .iter()
                .map(|&result_index| Detection {
                    result_index,
                    corners: [[10., 10.], [40., 10.], [40., 40.], [10., 40.]],
                })
                .collect(),
        },
    )
}
#[test]
fn selection_auto_copy_is_once_per_payload_including_aliases() {
    let mut m = model(&["first", "second"], &[0, 0, 1]);
    assert_eq!(m.update(Event::Select(0), true), Effect::Copy(0));
    assert_eq!(m.update(Event::Select(1), true), Effect::None);
    assert_eq!(m.update(Event::Select(2), true), Effect::Copy(1));
    assert_eq!(m.update(Event::Select(0), true), Effect::None);
    assert_eq!(m.update(Event::Copy, true), Effect::Copy(0));
    assert_eq!(m.update(Event::Copy, true), Effect::Copy(0));
}
#[test]
fn enabling_auto_copy_does_not_retroactively_copy() {
    let mut m = model(&["first", "second"], &[0, 1]);
    assert_eq!(m.update(Event::Select(0), false), Effect::None);
    assert_eq!(m.update(Event::Hover(Some(0)), true), Effect::None);
    assert_eq!(m.update(Event::Select(0), true), Effect::None);
    assert_eq!(m.update(Event::Select(1), true), Effect::Copy(1));
}
#[test]
fn escape_deselects_then_dismisses_and_invalid_indexes_do_nothing() {
    let mut m = model(&["https://example.org"], &[0]);
    assert_eq!(m.update(Event::Select(9), true), Effect::None);
    assert_eq!(m.selected_result(), None);
    m.update(Event::Select(0), false);
    m.update(Event::Select(9), true);
    assert_eq!(m.selected_result(), Some(0));
    assert_eq!(m.update(Event::Escape, true), Effect::None);
    assert!(m.text.is_none());
    assert_eq!(m.update(Event::Escape, true), Effect::Dismiss);
    assert_eq!(m.update(Event::Activate, true), Effect::None);
}
#[test]
fn focus_cycles_visible_controls_and_cannot_arm_a_disappeared_action() {
    let mut m = model(&["https://example.org", "plain"], &[0, 1]);
    let mut targets = Vec::new();
    for _ in 0..3 {
        m.update(Event::Next(false), false);
        targets.push(m.focus);
    }
    assert_eq!(
        targets,
        vec![
            Some(Focus::Detection(0)),
            Some(Focus::Detection(1)),
            Some(Focus::Close)
        ]
    );
    m.update(Event::Next(true), false);
    assert_eq!(m.focus, Some(Focus::Detection(1)));
    m.update(Event::Select(0), false);
    m.focus = Some(Focus::Activate);
    m.update(Event::Select(1), false);
    assert_ne!(m.update(Event::ActivateFocused, false), Effect::Activate(1));
    assert!(!m.focus_targets().contains(&Focus::Activate));
    m.update(Event::Background, false);
    assert_eq!(m.selected_result(), None);
}
#[test]
fn activation_requires_explicit_validated_non_pending_action() {
    let mut m = model(&["https://example.org"], &[0]);
    for e in [
        Event::Hover(Some(0)),
        Event::Next(false),
        Event::ActivateFocused,
        Event::Reveal,
        Event::Text(text_editor::Action::SelectAll),
    ] {
        assert!(!matches!(m.update(e, false), Effect::Activate(_)));
    }
    assert_eq!(m.update(Event::Activate, false), Effect::Activate(0));
    m.set_action_state(crate::actions::ActionState {
        key: crate::actions::ActionKey {
            session: JobId(7),
            result_index: 0,
        },
        token: 1,
        status: crate::actions::ActionStatus::Pending,
    });
    assert_eq!(m.update(Event::Activate, false), Effect::None);
}
#[test]
fn wifi_text_is_read_only_and_hiding_drops_secret_and_selection() {
    for payload in [
        "WIFI:T:WPA;S:Fixture;P:secret-123;;",
        "WIFI:S:bad;P:secret-123;broken",
    ] {
        let mut m = model(&[payload], &[0]);
        m.update(Event::Select(0), false);
        assert!(!m.text.as_ref().unwrap().text().contains("secret-123"));
        m.update(Event::Reveal, false);
        assert!(m.text.as_ref().unwrap().text().contains("secret-123"));
        m.update(Event::Text(text_editor::Action::SelectAll), false);
        assert!(m.text.as_ref().unwrap().selection().is_some());
        m.update(
            Event::Text(text_editor::Action::Edit(text_editor::Edit::Insert('x'))),
            false,
        );
        assert!(m.text.as_ref().unwrap().text().contains("secret-123"));
        m.update(Event::Reveal, false);
        assert!(!m.text.as_ref().unwrap().text().contains("secret-123"));
        assert!(m.text.as_ref().unwrap().selection().is_none());
        m.update(Event::Background, false);
        assert!(m.text.is_none());
        m.update(Event::Text(text_editor::Action::SelectAll), false);
        assert!(m.text.is_none());
    }
}
#[test]
fn letterboxed_view_uses_same_transform_for_highlight_and_hit_region() {
    let mut m = model(&["plain"], &[0]);
    m.resize(iced::Size::new(200., 200.));
    let t = m.transform().unwrap();
    assert_eq!(t.to_surface([10., 10.]), iced::Point::new(5., 55.));
    assert_eq!(
        geometry::hit_test(&m.scan.detections, &t, iced::Point::new(10., 60.)),
        Some(0)
    );
    assert_eq!(
        geometry::hit_test(&m.scan.detections, &t, iced::Point::new(10., 10.)),
        None
    );
}
#[test]
fn keyboard_cycles_panel_controls_forward_and_reverse_with_pending_action_removed() {
    let mut m = model(&["WIFI:T:WPA;S:Fixture;P:secret-123;;"], &[0]);
    m.update(Event::Select(0), false);
    let mut visited = Vec::new();
    for _ in 0..6 {
        m.update(Event::Next(false), false);
        visited.push(m.focus());
    }
    assert_eq!(
        visited,
        vec![
            Some(Focus::Reveal),
            Some(Focus::Copy),
            Some(Focus::Activate),
            Some(Focus::Close),
            Some(Focus::Detection(0)),
            Some(Focus::Text)
        ]
    );
    m.update(Event::Next(true), false);
    assert_eq!(m.focus(), Some(Focus::Detection(0)));
    m.update(Event::Next(true), false);
    assert_eq!(m.focus(), Some(Focus::Close));
    m.update(Event::Next(true), false);
    assert_eq!(m.update(Event::ActivateFocused, false), Effect::Activate(0));
    m.set_action_state(crate::actions::ActionState {
        key: crate::actions::ActionKey {
            session: JobId(7),
            result_index: 0,
        },
        token: 2,
        status: crate::actions::ActionStatus::Pending,
    });
    assert_eq!(m.focus(), None);
    assert_eq!(m.update(Event::ActivateFocused, false), Effect::None);
}
#[test]
fn interacting_with_text_disarms_previously_focused_action() {
    let mut m = model(&["https://example.org"], &[0]);
    m.update(Event::Select(0), false);
    m.focus = Some(Focus::Activate);
    m.update(
        Event::Text(text_editor::Action::Click(iced::Point::new(2., 2.))),
        false,
    );
    assert_eq!(m.focus(), Some(Focus::Text));
    assert_eq!(m.update(Event::ActivateFocused, false), Effect::None);
}

#[test]
fn stale_action_presentation_cannot_disable_current_session_action() {
    let mut m = model(&["https://example.org"], &[0]);
    m.update(Event::Select(0), false);
    m.set_action_state(crate::actions::ActionState {
        key: crate::actions::ActionKey {
            session: JobId(6),
            result_index: 0,
        },
        token: 1,
        status: crate::actions::ActionStatus::Pending,
    });
    assert_eq!(m.update(Event::Activate, false), Effect::Activate(0));
}

#[tokio::test]
async fn native_text_focus_uses_current_overlay_identity_and_unfocuses_when_leaving_text() {
    use futures_util::StreamExt;
    use iced::advanced::widget::{Id, operation::Focusable};
    struct NativeFocus(bool);
    impl Focusable for NativeFocus {
        fn is_focused(&self) -> bool {
            self.0
        }
        fn focus(&mut self) {
            self.0 = true;
        }
        fn unfocus(&mut self) {
            self.0 = false;
        }
    }
    let mut m = model(&["plain"], &[0]);
    m.update(Event::Select(0), false);
    assert_eq!(m.focus(), Some(Focus::Text));
    let mut state = NativeFocus(false);
    let id = Id::new("overlay-text-7-3");
    let bounds = iced::Rectangle::default();
    let mut actions = iced_runtime::task::into_stream(m.focus_task()).unwrap();
    let Some(iced_runtime::Action::Widget(mut operation)) = actions.next().await else {
        panic!("native focus operation required")
    };
    operation.focusable(Some(&id), bounds, &mut state);
    assert!(state.is_focused());
    m.update(Event::Next(false), false);
    let mut actions = iced_runtime::task::into_stream(m.focus_task()).unwrap();
    let Some(iced_runtime::Action::Widget(mut operation)) = actions.next().await else {
        panic!("native unfocus operation required")
    };
    operation.focusable(Some(&id), bounds, &mut state);
    assert!(!state.is_focused());
}

#[tokio::test]
async fn native_focus_task_cannot_unfocus_a_newer_overlay_editor() {
    use futures_util::StreamExt;
    use iced::advanced::widget::{Id, operation::Focusable};
    struct CurrentEditor(bool);
    impl Focusable for CurrentEditor {
        fn is_focused(&self) -> bool {
            self.0
        }
        fn focus(&mut self) {
            self.0 = true;
        }
        fn unfocus(&mut self) {
            self.0 = false;
        }
    }
    let old = model(&["plain"], &[0]);
    let mut current = CurrentEditor(true);
    let id = Id::new("overlay-text-8-3");
    let mut actions = iced_runtime::task::into_stream(old.focus_task()).unwrap();
    let Some(iced_runtime::Action::Widget(mut operation)) = actions.next().await else {
        panic!("native focus operation required")
    };
    operation.focusable(Some(&id), iced::Rectangle::default(), &mut current);
    assert!(current.is_focused());
}

fn overlapping_panel_model() -> Model {
    let mut m = model(&["https://example.org/first", "second"], &[0, 1]);
    m.frozen = Arc::new(Prepared::new(image::DynamicImage::new_rgba8(1000, 800)));
    m.resize(iced::Size::new(1000., 800.));
    m.scan.detections[0].corners = [[100., 100.], [140., 100.], [140., 140.], [100., 140.]];
    m.scan.detections[1].corners = [[150., 200.], [190., 200.], [190., 240.], [150., 240.]];
    m
}
#[test]
fn keyboard_reveals_detection_covered_by_panel_before_enter_can_select_or_copy_it() {
    for backward in [false, true] {
        let mut m = overlapping_panel_model();
        let viewport = iced::Size::new(1000., 800.);
        m.update(Event::Select(0), false);
        assert_eq!(
            m.panel_bounds(viewport),
            Some(iced::Rectangle {
                x: 92.,
                y: 148.,
                width: 380.,
                height: 330.
            })
        );
        if !backward {
            for _ in 0..4 {
                assert_eq!(m.update(Event::Next(false), true), Effect::None);
            }
            assert_eq!(m.focus(), Some(Focus::Detection(0)));
        }
        assert_eq!(m.update(Event::Next(backward), true), Effect::None);
        assert_eq!(m.focus(), Some(Focus::Detection(1)));
        assert_eq!(m.selected_result(), Some(0));
        // The responsive view uses this same panel layout: no opaque panel is
        // drawn above the focused region before explicit Enter selects it.
        assert_eq!(m.panel_bounds(viewport), None);
        assert_eq!(m.update(Event::ActivateFocused, true), Effect::Copy(1));
        assert_eq!(m.selected_result(), Some(1));
        assert_eq!(m.focus(), Some(Focus::Text));
        assert!(m.panel_bounds(viewport).is_some());
    }
}
#[test]
fn covered_detection_navigation_preserves_access_to_selected_panel_controls_both_directions() {
    let mut m = overlapping_panel_model();
    let viewport = iced::Size::new(1000., 800.);
    m.update(Event::Select(0), false);
    m.update(Event::Next(true), false);
    assert_eq!(m.panel_bounds(viewport), None);
    m.update(Event::Next(false), false);
    assert_eq!(m.focus(), Some(Focus::Text));
    assert!(m.panel_bounds(viewport).is_some());
    assert_eq!(m.selected_result(), Some(0));
    m.update(Event::Next(true), false);
    assert_eq!(m.focus(), Some(Focus::Detection(1)));
    assert_eq!(m.panel_bounds(viewport), None);
    m.update(Event::Next(true), false);
    assert_eq!(m.focus(), Some(Focus::Detection(0)));
    assert!(m.panel_bounds(viewport).is_some());
    m.update(Event::Next(true), false);
    assert_eq!(m.focus(), Some(Focus::Close));
    m.update(Event::Next(true), false);
    assert_eq!(m.focus(), Some(Focus::Activate));
    assert!(m.panel_bounds(viewport).is_some());
}
#[test]
fn selecting_oversized_code_opens_panel_and_navigation_can_uncover_its_own_outline() {
    let mut m = overlapping_panel_model();
    m.scan.detections.truncate(1);
    m.scan.detections[0].corners = [[0., 0.], [1000., 0.], [1000., 800.], [0., 800.]];
    let viewport = iced::Size::new(1000., 800.);
    m.update(Event::Select(0), false);
    assert_eq!(m.focus(), Some(Focus::Text));
    assert_eq!(
        m.panel_bounds(viewport),
        Some(iced::Rectangle {
            x: 0.,
            y: 0.,
            width: 380.,
            height: 330.
        })
    );
    assert_eq!(m.update(Event::Copy, false), Effect::Copy(0));
    m.update(Event::Next(true), false);
    assert_eq!(m.focus(), Some(Focus::Detection(0)));
    assert_eq!(m.panel_bounds(viewport), None);
    m.update(Event::Next(false), false);
    assert_eq!(m.focus(), Some(Focus::Text));
    assert!(m.panel_bounds(viewport).is_some());
}

#[test]
fn enter_on_already_selected_edge_code_reopens_panel_without_another_auto_copy() {
    let mut m = model(&["plain"], &[0]);
    let viewport = iced::Size::new(100., 80.);
    m.resize(viewport);
    assert_eq!(m.update(Event::Select(0), true), Effect::Copy(0));
    assert_eq!(m.focus(), Some(Focus::Text));
    assert!(m.panel_bounds(viewport).is_some());
    m.update(Event::Next(true), true);
    assert_eq!(m.focus(), Some(Focus::Detection(0)));
    assert_eq!(m.panel_bounds(viewport), None);
    assert_eq!(m.update(Event::ActivateFocused, true), Effect::None);
    assert_eq!(m.focus(), Some(Focus::Text));
    assert!(m.panel_bounds(viewport).is_some());
    assert_eq!(m.selected_result(), Some(0));
}
#[test]
fn responsive_panel_visibility_uses_rendered_viewport_when_stored_size_differs() {
    let mut m = overlapping_panel_model();
    let viewport = iced::Size::new(200., 160.);
    m.update(Event::Select(0), false);
    m.update(Event::Next(true), false);
    m.update(Event::Next(true), false);
    assert_eq!(m.focus(), Some(Focus::Detection(0)));
    assert!(m.panel_bounds(iced::Size::new(1000., 800.)).is_some());
    // At the actual smaller render size the clamped panel fills the surface,
    // so the selected code must be uncovered even before resize is delivered.
    assert_eq!(m.panel_bounds(viewport), None);
    m.update(Event::Next(false), false);
    m.update(Event::Next(false), false);
    assert_eq!(m.focus(), Some(Focus::Text));
    assert_eq!(
        m.panel_bounds(viewport),
        Some(iced::Rectangle {
            x: 0.,
            y: 0.,
            width: 200.,
            height: 160.
        })
    );
}

#[test]
fn editor_copy_and_cut_shortcuts_request_guarded_selection_copy() {
    use iced::keyboard::{
        Key, Modifiers,
        key::{NativeCode, Physical},
    };
    for key in ["c", "x"] {
        let binding = view::editor_binding(text_editor::KeyPress {
            key: Key::Character(key.into()),
            modified_key: Key::Character(key.into()),
            physical_key: Physical::Unidentified(NativeCode::Unidentified),
            modifiers: Modifiers::CTRL,
            text: None,
            status: text_editor::Status::Focused { is_hovered: false },
        });
        assert!(matches!(
            binding,
            Some(text_editor::Binding::Custom(Event::CopySelection))
        ));
    }
}
#[test]
fn partial_copy_uses_only_current_visible_selection_and_hide_discards_it() {
    use text_editor::{Action, Motion};
    let mut m = model(&["prefix suffix"], &[0]);
    m.update(Event::Select(0), false);
    m.update(Event::Text(Action::Move(Motion::DocumentStart)), false);
    for _ in 0..6 {
        m.update(Event::Text(Action::Select(Motion::Right)), false);
    }
    assert_eq!(m.update(Event::CopySelection, false), Effect::CopySelection);
    assert_eq!(m.selected_text().as_deref(), Some("prefix"));
    m.update(Event::Background, false);
    assert_eq!(m.selected_text(), None);
    assert_eq!(m.update(Event::CopySelection, false), Effect::None);
    let mut m = model(&["WIFI:T:bad;S:test;P:synthetic-secret;;"], &[0]);
    m.update(Event::Select(0), false);
    m.update(Event::Reveal, false);
    m.update(Event::Text(Action::SelectAll), false);
    assert!(m.selected_text().unwrap().contains("synthetic-secret"));
    m.update(Event::Reveal, false);
    assert_eq!(m.selected_text(), None);
    assert_eq!(m.update(Event::CopySelection, false), Effect::None);
}
