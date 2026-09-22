use std::sync::Arc;

use warp_core::ui::appearance::Appearance;
use warp_editor::render::element::VerticalExpansionBehavior;
use warp_util::user_input::UserInput;
use warpui::elements::ScrollbarWidth;
use warpui::elements::new_scrollable::ScrollableAppearance;
use warpui::platform::WindowStyle;
use warpui::{App, TypedActionView, ViewHandle, WindowId};

use super::{CodeEditorRenderOptions, CodeEditorView, CodeEditorViewAction};
use crate::AuthStateProvider;
use crate::cloud_object::model::persistence::CloudModel;
use crate::editor::InteractionState;
use crate::notebooks::editor::keys::NotebookKeybindings;
use crate::server::server_api::team::MockTeamClient;
use crate::server::server_api::workspace::MockWorkspaceClient;
use crate::settings_view::keybindings::KeybindingChangedNotifier;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::vim_registers::VimRegisters;
use crate::workspace::ActiveSession;
use crate::workspace::sync_inputs::SyncedInputState;
use crate::workspaces::user_workspaces::UserWorkspaces;

fn initialize_editor(app: &mut App) -> (WindowId, ViewHandle<CodeEditorView>) {
    initialize_settings_for_tests(app);

    // Add all required singleton models for EditorView dependencies
    app.add_singleton_model(|_| Appearance::mock());
    app.add_singleton_model(|_| SyncedInputState::mock());
    app.add_singleton_model(|_| VimRegisters::new());
    app.add_singleton_model(|_| KeybindingChangedNotifier::mock());
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());

    // Add mocks required by rich text editor (used in CommentEditor)
    app.add_singleton_model(CloudModel::mock);
    app.add_singleton_model(|_| ActiveSession::default());
    app.add_singleton_model(NotebookKeybindings::new);

    // Add UserWorkspaces mock (required by EditorView)
    let team_client_mock = Arc::new(MockTeamClient::new());
    let workspace_client_mock = Arc::new(MockWorkspaceClient::new());
    app.add_singleton_model(|ctx| {
        UserWorkspaces::mock(
            team_client_mock.clone(),
            workspace_client_mock.clone(),
            vec![],
            ctx,
        )
    });

    let (window, editor_view) = app.add_window(WindowStyle::NotStealFocus, |ctx| {
        CodeEditorView::new(
            None,
            None,
            CodeEditorRenderOptions::new(VerticalExpansionBehavior::GrowToMaxHeight),
            ctx,
        )
        .with_horizontal_scrollbar_appearance(ScrollableAppearance::new(ScrollbarWidth::Auto, true))
    });

    (window, editor_view)
}

#[test]
fn test_interaction_state_prevents_editing() {
    App::test((), |mut app| async move {
        let (_window, editor_view) = initialize_editor(&mut app);

        let text = editor_view.update(&mut app, |view, ctx| {
            view.handle_action(&CodeEditorViewAction::UserTyped(UserInput::new("abc")), ctx);
            view.text(ctx)
        });

        assert_eq!(text.as_str(), "abc");

        // Set to be only selectable
        editor_view.update(&mut app, |view, ctx| {
            view.set_interaction_state(InteractionState::Selectable, ctx);
        });

        let text = editor_view.update(&mut app, |view, ctx| {
            view.handle_action(&CodeEditorViewAction::UserTyped(UserInput::new("def")), ctx);
            view.text(ctx)
        });

        assert_eq!(text.as_str(), "abc");
    });
}

#[test]
fn epy766_editor_event_clock() {
    App::test((), |mut app| async move {
        let (_, editor) = initialize_editor(&mut app);
        editor.update(&mut app, |view, ctx| {
            view.handle_action(
                &CodeEditorViewAction::UserTyped(UserInput::new("same same")),
                ctx,
            );
            view.set_selection_lsp_ranges(&[(0, 0, 0, 4)], ctx);
        });
        let first = editor.read(&app, |view, ctx| {
            (view.selected_text(ctx), view.selection_changed_at(ctx))
        });
        println!("EPY766 editor first={first:?}");
        assert_eq!(first.0.as_deref(), Some("same"));
        assert!(first.1.is_some());
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert_eq!(
            first,
            editor.read(&app, |view, ctx| (
                view.selected_text(ctx),
                view.selection_changed_at(ctx)
            ))
        );
        editor.update(&mut app, |view, ctx| {
            view.set_selection_lsp_ranges(&[(0, 5, 0, 9)], ctx);
        });
        let moved = editor.read(&app, |view, ctx| {
            (view.selected_text(ctx), view.selection_changed_at(ctx))
        });
        println!("EPY766 editor same text, different range={moved:?}");
        assert_eq!(moved.0, first.0);
        assert!(moved.1 > first.1);
        editor.update(&mut app, |view, ctx| view.clear_selection(ctx));
        assert!(
            editor
                .read(&app, |view, ctx| view.selected_text(ctx))
                .is_none()
        );
    });
}

#[test]
fn epy766_passive_editor_append_keeps_selection_time() {
    App::test((), |mut app| async move {
        let (_, editor) = initialize_editor(&mut app);
        editor.update(&mut app, |view, ctx| {
            view.handle_action(
                &CodeEditorViewAction::UserTyped(UserInput::new("same same")),
                ctx,
            );
            view.set_selection_lsp_ranges(&[(0, 0, 0, 4)], ctx);
        });
        let before = editor.read(&app, |view, ctx| {
            (
                view.selected_text(ctx),
                view.selection_lsp_ranges(ctx),
                view.selection_changed_at(ctx),
            )
        });
        std::thread::sleep(std::time::Duration::from_millis(5));
        editor.update(&mut app, |view, ctx| {
            view.model
                .update(ctx, |model, ctx| model.append_at_end(" background", ctx));
        });
        let after = editor.read(&app, |view, ctx| {
            (
                view.selected_text(ctx),
                view.selection_lsp_ranges(ctx),
                view.selection_changed_at(ctx),
            )
        });
        println!("EPY766 passive editor append before={before:?}, after={after:?}");
        assert_eq!(before.0.as_deref(), Some("same"));
        assert_eq!(
            before, after,
            "background content changes are not selection actions"
        );
    });
}
