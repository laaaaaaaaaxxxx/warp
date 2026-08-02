use ::local_control::{ActionKind, ErrorCode};

use super::{ensure_surface_available, validate_staged_input_text};
use crate::features::FeatureFlag;
use crate::local_control::handlers::metadata::SurfaceDestination;

#[test]
fn staged_input_accepts_multiline_but_rejects_control_sequences() {
    for text in ["safe staged text", "line\nbreak", "tab\tbreak", "multi\nline\twith\ttabs\n"] {
        assert!(validate_staged_input_text(ActionKind::InputInsert, text).is_ok());
    }

    for text in ["line\rbreak", "crlf\r\nbreak", "\u{1b}[31m", "bell\u{7}"] {
        let error = validate_staged_input_text(ActionKind::InputInsert, text).err();
        assert!(error.is_some_and(|error| error.code == ErrorCode::InvalidParams));
    }
}

#[test]
fn unavailable_surface_open_returns_structured_error() {
    let flag_guard = FeatureFlag::AgentManagementView.override_enabled(false);
    warpui::App::test((), |mut app| async move {
        let error = app
            .update(|ctx| {
                ensure_surface_available(
                    ActionKind::SurfaceAgentManagementOpen,
                    SurfaceDestination::AgentManagement,
                    ctx,
                )
            })
            .expect_err("disabled surface is rejected");
        assert_eq!(error.code, ErrorCode::UnsupportedAction);
        assert!(error.message.contains("surface.agent_management.open"));
    });
    drop(flag_guard);
}
