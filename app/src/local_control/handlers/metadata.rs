//! Metadata response builders for local-control introspection actions.
#[cfg(test)]
#[path = "metadata_tests.rs"]
mod tests;
use ::local_control::protocol::{
    ActionNameParams, ActiveTargetChain, PaneTarget, SelectionRangesParams, SessionTarget,
    SurfaceListResult, SurfaceSummary, TabTarget, TargetSelector, WindowTarget,
};
use ::local_control::{
    Action, ActionKind, ActionMetadata, ControlError, ErrorCode, InstanceId, PROTOCOL_VERSION,
};
use serde::Serialize;
use serde_json::{Value, json};
use settings::Setting as _;
use warp_core::channel::ChannelState;
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warpui::{AppContext, ModelContext, SingletonEntity, ViewHandle, WindowId};

use crate::code_review::code_review_view::CodeReviewView;
use crate::drive::settings::WarpDriveSettings;
use crate::features::FeatureFlag;
use crate::local_control::LocalControlBridge;
use crate::local_control::resolver::{reject_target_families, require_active_window_id_for_action};
use crate::pane_group::{PaneGroup, PaneId};
use crate::remote_server::manager::RemoteServerManager;
use crate::settings::{AISettings, CodeSettings};
use crate::tab::SelectedTabColor;
use crate::terminal::cli_agent_sessions::CLIAgentSessionsModel;
use crate::workspace::Workspace;
use crate::workspace::tab_settings::TabSettings;

#[derive(Serialize)]
struct InstanceResponse<'a> {
    action: &'static str,
    instance_id: Option<&'a str>,
    pid: u32,
    channel: String,
    app_id: String,
    protocol_version: u32,
    actions: Vec<ActionMetadata>,
}

fn active_session_target(target: &TargetSelector) -> TargetSelector {
    if !matches!(target.session, Some(SessionTarget::Active)) {
        return target.clone();
    }
    TargetSelector {
        window: target.window.clone().or(Some(WindowTarget::Active)),
        tab: target.tab.clone().or(Some(TabTarget::Active)),
        pane: target.pane.clone().or(Some(PaneTarget::Active)),
        session: target.session.clone(),
    }
}

fn select_session_entries(
    entries: Vec<SessionEntry>,
    session: Option<&SessionTarget>,
    action: ActionKind,
) -> Result<Vec<SessionEntry>, ControlError> {
    match session {
        None => Ok(entries),
        Some(SessionTarget::Active) => explicit_matches(
            entries
                .into_iter()
                .filter(|entry| entry.is_active)
                .collect(),
            action,
            "active session",
            ErrorCode::MissingTarget,
        ),
        Some(SessionTarget::Id { id }) => explicit_matches(
            entries
                .into_iter()
                .filter(|entry| entry.pane_id.to_string() == id.0)
                .collect(),
            action,
            "session id",
            ErrorCode::StaleTarget,
        ),
    }
}

#[derive(Serialize)]
struct PingResponse<'a> {
    action: &'static str,
    ok: bool,
    instance_id: Option<&'a str>,
    protocol_version: u32,
}

#[derive(Serialize)]
struct VersionResponse<'a> {
    action: &'static str,
    instance_id: Option<&'a str>,
    protocol_version: u32,
    channel: String,
    app_id: String,
}

#[derive(Clone, Copy)]
pub(super) struct WindowEntry {
    pub(super) window_id: WindowId,
    pub(super) index: usize,
}

#[derive(Clone)]
pub(super) struct TabEntry {
    pub(super) window_id: WindowId,
    pub(super) window_index: usize,
    pub(super) index: usize,
    pub(super) workspace_active_tab_index: usize,
    pub(super) pane_group: ViewHandle<PaneGroup>,
}

#[derive(Clone)]
pub(super) struct PaneEntry {
    pub(super) window_id: WindowId,
    pub(super) window_index: usize,
    pub(super) tab_id: String,
    pub(super) tab_index: usize,
    pub(super) index: usize,
    pub(super) pane_group: ViewHandle<PaneGroup>,
    pub(super) pane_id: PaneId,
}

struct SessionEntry {
    window_id: WindowId,
    window_index: usize,
    tab_id: String,
    tab_index: usize,
    pane_id: PaneId,
    pane_index: usize,
    is_active: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum SurfaceDestination {
    Settings,
    CommandPalette,
    CommandSearch,
    ThemePicker,
    Keybindings,
    WarpDrive,
    ResourceCenter,
    AiAssistant,
    CodeReview,
    ProjectExplorer,
    GlobalSearch,
    ConversationList,
    LeftPanel,
    RightPanel,
    VerticalTabs,
    AgentManagement,
}

impl SurfaceDestination {
    const ALL: &[Self] = &[
        Self::Settings,
        Self::CommandPalette,
        Self::CommandSearch,
        Self::ThemePicker,
        Self::Keybindings,
        Self::WarpDrive,
        Self::ResourceCenter,
        Self::AiAssistant,
        Self::CodeReview,
        Self::ProjectExplorer,
        Self::GlobalSearch,
        Self::ConversationList,
        Self::LeftPanel,
        Self::RightPanel,
        Self::VerticalTabs,
        Self::AgentManagement,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::CommandPalette => "command_palette",
            Self::CommandSearch => "command_search",
            Self::ThemePicker => "theme_picker",
            Self::Keybindings => "keybindings",
            Self::WarpDrive => "warp_drive",
            Self::ResourceCenter => "resource_center",
            Self::AiAssistant => "ai_assistant",
            Self::CodeReview => "code_review",
            Self::ProjectExplorer => "project_explorer",
            Self::GlobalSearch => "global_search",
            Self::ConversationList => "conversation_list",
            Self::LeftPanel => "left_panel",
            Self::RightPanel => "right_panel",
            Self::VerticalTabs => "vertical_tabs",
            Self::AgentManagement => "agent_management",
        }
    }
}

pub(crate) fn instance(
    instance_id: &Option<InstanceId>,
) -> Result<serde_json::Value, ControlError> {
    to_json_value(InstanceResponse {
        action: ActionKind::InstanceList.as_str(),
        instance_id: instance_id.as_ref().map(|id| id.0.as_str()),
        pid: std::process::id(),
        channel: ChannelState::channel().to_string(),
        app_id: ChannelState::app_id().to_string(),
        protocol_version: PROTOCOL_VERSION,
        actions: ActionKind::implemented_metadata(),
    })
}

pub(crate) fn ping(instance_id: &Option<InstanceId>) -> Result<serde_json::Value, ControlError> {
    to_json_value(PingResponse {
        action: ActionKind::AppPing.as_str(),
        ok: true,
        instance_id: instance_id.as_ref().map(|id| id.0.as_str()),
        protocol_version: PROTOCOL_VERSION,
    })
}

pub(crate) fn version(instance_id: &Option<InstanceId>) -> Result<serde_json::Value, ControlError> {
    to_json_value(VersionResponse {
        action: ActionKind::AppVersion.as_str(),
        instance_id: instance_id.as_ref().map(|id| id.0.as_str()),
        protocol_version: PROTOCOL_VERSION,
        channel: ChannelState::channel().to_string(),
        app_id: ChannelState::app_id().to_string(),
    })
}

pub(crate) fn active(
    instance_id: &Option<InstanceId>,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    Ok(json!({
        "action": ActionKind::AppActive.as_str(),
        "active": active_chain(instance_id, ctx)?,
    }))
}

pub(crate) fn inspect(
    instance_id: &Option<InstanceId>,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    Ok(json!({
        "action": ActionKind::InstanceInspect.as_str(),
        "instance_id": instance_id.as_ref().map(|id| id.0.as_str()),
        "pid": std::process::id(),
        "channel": ChannelState::channel().to_string(),
        "app_id": ChannelState::app_id().to_string(),
        "protocol_version": PROTOCOL_VERSION,
        "active": active_chain(instance_id, ctx)?,
        "actions": ActionKind::implemented_metadata(),
    }))
}

pub(crate) fn action_list() -> serde_json::Value {
    json!({
        "action": ActionKind::ActionList.as_str(),
        "actions": ActionKind::implemented_metadata(),
    })
}

pub(crate) fn action_inspect(action: &Action) -> Result<serde_json::Value, ControlError> {
    let params = action_name_params(action)?;
    let metadata = action_metadata_for_name(&params.action)?;
    Ok(json!({
        "action": ActionKind::ActionInspect.as_str(),
        "metadata": metadata,
    }))
}

pub(crate) fn capability_list() -> serde_json::Value {
    json!({
        "action": ActionKind::CapabilityList.as_str(),
        "capabilities": ActionKind::implemented_metadata(),
    })
}

pub(crate) fn capability_inspect(action: &Action) -> Result<serde_json::Value, ControlError> {
    let params = action_name_params(action)?;
    let metadata = action_metadata_for_name(&params.action)?;
    Ok(json!({
        "action": ActionKind::CapabilityInspect.as_str(),
        "capability": metadata,
    }))
}

pub(crate) fn surface_list(
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    to_json_value(SurfaceListResult {
        surfaces: SurfaceDestination::ALL
            .iter()
            .copied()
            .map(|destination| {
                let unavailable_reason =
                    surface_unavailable_reason(destination, ctx).map(str::to_owned);
                SurfaceSummary {
                    name: destination.name().to_owned(),
                    is_available: unavailable_reason.is_none(),
                    unavailable_reason,
                }
            })
            .collect(),
    })
}

pub(crate) fn surface_unavailable_reason(
    destination: SurfaceDestination,
    ctx: &AppContext,
) -> Option<&'static str> {
    match destination {
        SurfaceDestination::Settings
        | SurfaceDestination::CommandPalette
        | SurfaceDestination::CommandSearch
        | SurfaceDestination::ThemePicker
        | SurfaceDestination::Keybindings
        | SurfaceDestination::ResourceCenter => None,
        SurfaceDestination::WarpDrive if !WarpDriveSettings::is_warp_drive_enabled(ctx) => {
            Some("Warp Drive is disabled")
        }
        SurfaceDestination::WarpDrive => None,
        SurfaceDestination::AiAssistant if !AISettings::as_ref(ctx).is_any_ai_enabled(ctx) => {
            Some("AI features are disabled")
        }
        SurfaceDestination::AiAssistant => None,
        SurfaceDestination::CodeReview | SurfaceDestination::RightPanel
            if !cfg!(feature = "local_fs") =>
        {
            Some("code review is unavailable without local filesystem support")
        }
        SurfaceDestination::CodeReview | SurfaceDestination::RightPanel => None,
        SurfaceDestination::ProjectExplorer
            if !cfg!(feature = "local_fs")
                || !*CodeSettings::as_ref(ctx).show_project_explorer.value() =>
        {
            Some("project explorer is unavailable or disabled")
        }
        SurfaceDestination::ProjectExplorer => None,
        SurfaceDestination::GlobalSearch
            if !cfg!(feature = "local_fs")
                || !FeatureFlag::GlobalSearch.is_enabled()
                || !*CodeSettings::as_ref(ctx).show_global_search.value() =>
        {
            Some("global search is unavailable or disabled")
        }
        SurfaceDestination::GlobalSearch => None,
        SurfaceDestination::ConversationList
            if !FeatureFlag::AgentViewConversationListView.is_enabled()
                || !AISettings::as_ref(ctx).is_any_ai_enabled(ctx)
                || !*AISettings::as_ref(ctx).show_conversation_history.value() =>
        {
            Some("agent conversation history is unavailable or disabled")
        }
        SurfaceDestination::ConversationList => None,
        SurfaceDestination::LeftPanel
            if surface_unavailable_reason(SurfaceDestination::ProjectExplorer, ctx).is_some()
                && surface_unavailable_reason(SurfaceDestination::GlobalSearch, ctx).is_some()
                && surface_unavailable_reason(SurfaceDestination::ConversationList, ctx)
                    .is_some()
                && surface_unavailable_reason(SurfaceDestination::WarpDrive, ctx).is_some() =>
        {
            Some("the left panel has no available views")
        }
        SurfaceDestination::LeftPanel => None,
        SurfaceDestination::VerticalTabs
            if !FeatureFlag::VerticalTabs.is_enabled()
                || !*TabSettings::as_ref(ctx).use_vertical_tabs.value() =>
        {
            Some("vertical tabs are unavailable or disabled")
        }
        SurfaceDestination::VerticalTabs => None,
        SurfaceDestination::AgentManagement
            if !FeatureFlag::AgentManagementView.is_enabled()
                || !AISettings::as_ref(ctx).is_any_ai_enabled(ctx) =>
        {
            Some("agent management is unavailable or disabled")
        }
        SurfaceDestination::AgentManagement => None,
    }
}

pub(crate) fn window_list(
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    reject_target_families(
        ActionKind::WindowList,
        target.tab.is_some() || target.pane.is_some() || target.session.is_some(),
        "tab, pane, or session selectors",
    )?;
    let active_window = ctx.windows().active_window();
    let mut windows = Vec::new();
    for entry in select_window_entries(target, false, ActionKind::WindowList, ctx)? {
        windows.push(json!({
            "window_id": entry.window_id.to_string(),
            "index": entry.index as u32,
            "is_active": Some(entry.window_id) == active_window,
            "has_workspace": workspace_for_window(entry.window_id, ActionKind::WindowList, ctx)?.is_some(),
        }));
    }
    Ok(json!({
        "action": ActionKind::WindowList.as_str(),
        "windows": windows,
    }))
}

pub(crate) fn window_inspect(
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    reject_target_families(
        ActionKind::WindowInspect,
        target.tab.is_some() || target.pane.is_some() || target.session.is_some(),
        "tab, pane, or session selectors",
    )?;
    let target = TargetSelector {
        window: target.window.clone().or(Some(WindowTarget::Active)),
        tab: None,
        pane: None,
        session: None,
    };
    let data = window_list(&target, ctx)?;
    let window = single_entry(data.get("windows"), ActionKind::WindowInspect)?;
    Ok(json!({
        "action": ActionKind::WindowInspect.as_str(),
        "window": window,
    }))
}

pub(crate) fn tab_list(
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    reject_target_families(
        ActionKind::TabList,
        target.pane.is_some() || target.session.is_some(),
        "pane or session selectors",
    )?;
    let entries = select_tab_entries(target, ActionKind::TabList, ctx)?;
    let tabs = entries
        .into_iter()
        .map(|entry| {
            json!({
                "tab_id": entry.pane_group.id().to_string(),
                "window_id": entry.window_id.to_string(),
                "window_index": entry.window_index as u32,
                "index": entry.index as u32,
                "is_active": entry.index == entry.workspace_active_tab_index,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "action": ActionKind::TabList.as_str(),
        "tabs": tabs,
    }))
}

pub(crate) fn tab_inspect(
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    reject_target_families(
        ActionKind::TabInspect,
        target.pane.is_some() || target.session.is_some(),
        "pane or session selectors",
    )?;
    let target = TargetSelector {
        window: target.window.clone(),
        tab: target.tab.clone().or(Some(TabTarget::Active)),
        pane: None,
        session: None,
    };
    let data = tab_list(&target, ctx)?;
    let tab = single_entry(data.get("tabs"), ActionKind::TabInspect)?;
    Ok(json!({
        "action": ActionKind::TabInspect.as_str(),
        "tab": tab,
    }))
}

/// Serializes 0-indexed LSP selection ranges for the `selections` field.
fn lsp_ranges_json(ranges: &[(usize, usize, usize, usize)]) -> Vec<serde_json::Value> {
    ranges
        .iter()
        .map(|(start_line, start_column, end_line, end_column)| {
            json!({
                "start_line": start_line,
                "start_column": start_column,
                "end_line": end_line,
                "end_column": end_column,
            })
        })
        .collect()
}

pub(crate) fn pane_list(
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    reject_target_families(
        ActionKind::PaneList,
        target.session.is_some(),
        "session selectors",
    )?;
    let entries = select_pane_entries(target, ActionKind::PaneList, ctx)?;
    let mut panes = Vec::new();
    for entry in entries {
        let (
            is_active,
            has_terminal_session,
            working_directory,
            working_directory_location,
            code_view,
            code_review_open,
            rich_input_open,
            ssh_destination,
            cli_agent,
            terminal_selection,
            input_selection,
        ) = entry.pane_group.read(ctx, |pane_group, ctx| {
            let terminal_view = pane_group.terminal_view_from_pane_id(entry.pane_id, ctx);
            // Working directory of the focused terminal session, if this is a terminal
            // pane. `pwd()` is not `local_fs`-gated, so this works in the OSS build.
            let working_directory = terminal_view.as_ref().and_then(|tv| tv.as_ref(ctx).pwd());
            // `pwd()` drops SSH host identity, which external tools need to address the same
            // working directory on the correct machine.
            let working_directory_location = terminal_view
                .as_ref()
                .and_then(|tv| tv.as_ref(ctx).pwd_as_local_or_remote(ctx));
            let code_view = pane_group.code_view_from_pane_id(entry.pane_id, ctx);
            // CLI agent Rich Input visibility. Keyed by terminal view id
            // in CLIAgentSessionsModel, so this is per-pane, unlike the
            // tab-level code_review_open below.
            let rich_input_open = terminal_view
                .as_ref()
                .is_some_and(|tv| CLIAgentSessionsModel::as_ref(ctx).is_input_open(tv.id()));
            // Which CLI agent, if any, is registered for this pane. None for
            // plain terminal and Warp Agent panes, where Rich Input does not
            // exist at all -- there rich_input_open=false means absent, not
            // closed, and external tools must not try to open it.
            let cli_agent = terminal_view.as_ref().and_then(|tv| {
                CLIAgentSessionsModel::as_ref(ctx)
                    .session(tv.id())
                    .map(|session| session.agent.to_serialized_name())
            });
            // How this pane's session reached its host, as reported by the
            // SSH wrapper. `remote_host_id` says which host Warp bound the
            // session to; this says what to run to get there again, which
            // is what split inheritance and session restoration replay.
            let ssh_destination = terminal_view.as_ref().and_then(|tv| {
                tv.as_ref(ctx)
                    .ssh_session_snapshot(ctx)
                    .map(|ssh_session| ssh_session.destination)
            });
            // Selected text, split by carrier. Deliberately not
            // `selected_text_from_focused_pane`: that helper answers only for the
            // focused pane, returns no carrier identity, and prefers the input
            // editor's selection over the terminal view's -- external tools need
            // exactly the distinction it collapses. Read per pane, so split panes
            // each report their own selection.
            let terminal_selection = terminal_view
                .as_ref()
                .and_then(|tv| tv.as_ref(ctx).selected_text(ctx));
            let input_selection = terminal_view
                .as_ref()
                .and_then(|tv| tv.as_ref(ctx).selected_text_from_input(ctx));
            (
                pane_group.focused_pane_id(ctx) == entry.pane_id,
                terminal_view.is_some(),
                working_directory,
                working_directory_location,
                code_view,
                // Per-tab Code Review panel visibility. The right panel is
                // CR-only (resource center / AI assistant use workspace-level
                // flags), and this flag is unconditionally set false whenever
                // the panel closes — unlike CodeReviewView.is_open, which can
                // stay true when the close path fails to find the cached view.
                pane_group.right_panel_open,
                rich_input_open,
                ssh_destination,
                cli_agent,
                terminal_selection,
                input_selection,
            )
        });
        // File path + cursor position of the active tab if this is a code-editor
        // pane. Uses only non-`local_fs`-gated accessors so it works in the OSS
        // build. `cursor_line`/`cursor_column` are 0-indexed (LSP convention).
        let (file_path, file_remote_host, cursor_line, cursor_column, editor_selection) = code_view
            .map(|cv| {
                cv.read(ctx, |cv, cx| {
                    let location = cv
                        .tab_at(cv.active_tab_index())
                        .and_then(|tab| tab.location());
                    let file_path = location.map(|location| location.display_path());
                    // A path alone is ambiguous across local and remote filesystems, so preserve
                    // the editor location's host identity separately.
                    let file_remote_host = location
                        .and_then(|location| location.as_remote())
                        .map(|remote| remote.host_id.clone());
                    let cursor = cv.active_cursor_position(cx);
                    // Editor selection rides the same read as file path and cursor:
                    // one pass, one active-tab decision, no chance of the two
                    // disagreeing about which tab they describe.
                    let editor_selection = cv
                        .selected_text(cx)
                        .map(|text| (text, cv.active_selection_ranges(cx)));
                    (
                        file_path,
                        file_remote_host,
                        cursor.map(|c| c.0),
                        cursor.map(|c| c.1),
                        editor_selection,
                    )
                })
            })
            .unwrap_or((None, None, None, None, None));
        // Code Review lives in the right panel, not the pane group, so its diff
        // editor is never a pane here. When one of its diff editors is focused,
        // expose that file path + 0-indexed cursor position on the active pane
        // (the pane group's focused pane, since Code Review sits outside it), so
        // external tools can drive jumps just like an editor pane's cursor.
        let mut cr_cursor: Option<(LocalOrRemotePath, usize, usize)> = None;
        if is_active && let Some(cr_views) = ctx.views_of_type::<CodeReviewView>(entry.window_id) {
            for cr in cr_views {
                cr_cursor = cr.read(ctx, |cr, cx| cr.focused_editor_cursor(cx));
                if cr_cursor.is_some() {
                    break;
                }
            }
        }
        let mut cr_selection: Option<(
            LocalOrRemotePath,
            String,
            Vec<(usize, usize, usize, usize)>,
        )> = None;
        if is_active && let Some(cr_views) = ctx.views_of_type::<CodeReviewView>(entry.window_id) {
            for cr in cr_views {
                cr_selection = cr.read(ctx, |cr, cx| cr.focused_editor_selection(cx));
                if cr_selection.is_some() {
                    break;
                }
            }
        }
        let (
            code_review_file_path,
            code_review_remote_host_id,
            code_review_cursor_line,
            code_review_cursor_column,
        ) = match cr_cursor {
            Some((location, line, column)) => {
                let remote_host_id = location
                    .as_remote()
                    .map(|remote| remote.host_id.to_string());
                (
                    Some(location.display_path()),
                    remote_host_id,
                    Some(line),
                    Some(column),
                )
            }
            None => (None, None, None, None),
        };
        // Editor panes take their host from the file location; terminal panes take it from the
        // session working directory. The server generates opaque host IDs per session, so expose
        // the manager's stable label as well rather than forcing external tools to guess.
        let remote_host = file_remote_host.or_else(|| {
            working_directory_location
                .as_ref()
                .and_then(|location| location.as_remote())
                .map(|remote| remote.host_id.clone())
        });
        let remote_host_label = remote_host.as_ref().and_then(|host_id| {
            RemoteServerManager::as_ref(ctx)
                .host_label(host_id)
                .map(str::to_string)
        });
        let remote_host_id = remote_host.map(|host_id| host_id.to_string());
        // Manual tab color, exposed per-pane the same way as `code_review_open`
        // (color is a tab-level attribute in Warp, not per-pane). Only an
        // explicit `Color(_)` counts as configured; `Unset` (directory-default
        // fallback) and `Cleared` both report unconfigured. See the workspace
        // `set_tab_color` / `tab_selected_color` pair.
        let color = workspace_for_window(entry.window_id, ActionKind::PaneList, ctx)?.and_then(
            |workspace| {
                workspace.read(ctx, |workspace, _| {
                    match workspace.tab_selected_color(entry.tab_index) {
                        Some(SelectedTabColor::Color(color)) => Some(color),
                        _ => None,
                    }
                })
            },
        );
        // One entry per carrier that currently holds a selection, because a single
        // pane can hold two at once -- the terminal view and the Rich Input editor
        // are independent selection sites. Upstream's focused-pane helper collapses
        // that by always preferring the input editor; here the caller picks by
        // carrier instead of guessing which one is newer. Empty array means nothing
        // is selected anywhere in this pane. Terminal and input carriers have no
        // ranges: a terminal has no file line numbers to report.
        let mut selections: Vec<serde_json::Value> = Vec::new();
        if let Some(text) = terminal_selection {
            selections.push(json!({ "carrier": "terminal", "text": text }));
        }
        if let Some(text) = input_selection {
            selections.push(json!({ "carrier": "input", "text": text }));
        }
        if let Some((text, ranges)) = editor_selection {
            selections.push(json!({
                "carrier": "editor",
                "text": text,
                "file_path": file_path.clone(),
                "ranges": lsp_ranges_json(&ranges),
            }));
        }
        if let Some((location, text, ranges)) = cr_selection {
            selections.push(json!({
                "carrier": "code_review",
                "text": text,
                "file_path": location.display_path(),
                "remote_host_id": location
                    .as_remote()
                    .map(|remote| remote.host_id.to_string()),
                "ranges": lsp_ranges_json(&ranges),
            }));
        }
        panes.push(json!({
            "pane_id": entry.pane_id.to_string(),
            "tab_id": entry.tab_id,
            "tab_index": entry.tab_index as u32,
            "window_id": entry.window_id.to_string(),
            "window_index": entry.window_index as u32,
            "index": entry.index as u32,
            "is_active": is_active,
            "has_terminal_session": has_terminal_session,
            "working_directory": working_directory,
            "file_path": file_path,
            "cursor_line": cursor_line,
            "cursor_column": cursor_column,
            "remote_host_id": remote_host_id,
            "remote_host_label": remote_host_label,
            "ssh_destination": ssh_destination,
            "code_review_file_path": code_review_file_path,
            "code_review_remote_host_id": code_review_remote_host_id,
            "code_review_cursor_line": code_review_cursor_line,
            "code_review_cursor_column": code_review_cursor_column,
            "code_review_open": code_review_open,
            "rich_input_open": rich_input_open,
            "cli_agent": cli_agent,
            "color_configured": color.is_some(),
            "color": color,
            "selections": selections,
        }));
    }
    Ok(json!({
        "action": ActionKind::PaneList.as_str(),
        "panes": panes,
    }))
}

/// Clears the selections `pane inspect` would have reported for the targeted
/// pane, and names the carriers it actually cleared.
///
/// Discovery reuses `select_pane_entries` and defaults to the active pane, so
/// this clears exactly what the read reports -- read and clear can never
/// disagree about which pane or which carriers they mean. There is deliberately
/// no carrier parameter: letting callers address a carrier separately would
/// reopen that gap.
pub(crate) fn selection_clear(
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    reject_target_families(
        ActionKind::SelectionClear,
        target.session.is_some(),
        "session selectors",
    )?;
    let target = TargetSelector {
        window: target.window.clone(),
        tab: target.tab.clone(),
        pane: target.pane.clone().or(Some(PaneTarget::Active)),
        session: None,
    };
    let entries = select_pane_entries(&target, ActionKind::SelectionClear, ctx)?;
    let mut cleared: Vec<serde_json::Value> = Vec::new();
    for entry in entries {
        let pane_id = entry.pane_id.to_string();
        let terminal_view = entry.pane_group.read(ctx, |pane_group, ctx| {
            pane_group.terminal_view_from_pane_id(entry.pane_id, ctx)
        });
        // 输入框那格故意不清:clear 收的是用完的上下文选中,而输入框里的是人正在写的
        // 底稿,从来不是上下文。要设它走 selection.set,那是调用方点名的动作。
        if let Some(terminal_view) = terminal_view
            && terminal_view.update(ctx, |terminal_view, ctx| {
                terminal_view.clear_selections_for_control(ctx)
            })
        {
            cleared.push(json!({ "pane_id": pane_id, "carrier": "terminal" }));
        }
        let code_view = entry.pane_group.read(ctx, |pane_group, ctx| {
            pane_group.code_view_from_pane_id(entry.pane_id, ctx)
        });
        if let Some(code_view) = code_view
            && code_view.update(ctx, |code_view, ctx| {
                code_view.clear_active_tab_selections(ctx)
            })
        {
            cleared.push(json!({ "pane_id": pane_id, "carrier": "editor" }));
        }
        // Code Review sits outside the pane group, so it is reachable only from
        // the focused pane -- the same condition the read side applies.
        let is_active = entry.pane_group.read(ctx, |pane_group, ctx| {
            pane_group.focused_pane_id(ctx) == entry.pane_id
        });
        if is_active && let Some(cr_views) = ctx.views_of_type::<CodeReviewView>(entry.window_id) {
            for cr in cr_views {
                if cr.update(ctx, |cr, cx| cr.clear_focused_editor_selection(cx)) {
                    cleared.push(json!({ "pane_id": pane_id, "carrier": "code_review" }));
                    break;
                }
            }
        }
    }
    Ok(json!({
        "action": ActionKind::SelectionClear.as_str(),
        "cleared": cleared,
    }))
}

/// Sets the selection in the targeted pane's editor, or in the focused Code
/// Review diff editor, from 0-indexed LSP ranges.
///
/// The inverse of the `selections` read: a range read out of one editor can be
/// handed straight back to another. This is what lets a round trip carry the
/// selection instead of collapsing it to a caret.
///
/// Terminal and Rich Input carriers are not settable and never will be from
/// here: a terminal has no file line numbers to address, so there is no
/// coordinate space for a caller to name a range in. That is a boundary, not a
/// deferral.
pub(crate) fn selection_set(
    target: &TargetSelector,
    params: &serde_json::Value,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    reject_target_families(
        ActionKind::SelectionSet,
        target.session.is_some(),
        "session selectors",
    )?;
    let parsed: SelectionRangesParams = serde_json::from_value(params.clone()).map_err(|_| {
        ControlError::new(
            ErrorCode::InvalidParams,
            format!(
                "{} received parameters with the wrong shape",
                ActionKind::SelectionSet.as_str()
            ),
        )
    })?;
    let ranges: Vec<(usize, usize, usize, usize)> = parsed
        .ranges
        .iter()
        .map(|r| (r.start_line, r.start_column, r.end_line, r.end_column))
        .collect();
    if ranges.is_empty() {
        return Err(ControlError::new(
            ErrorCode::InvalidParams,
            format!(
                "{} requires at least one range",
                ActionKind::SelectionSet.as_str()
            ),
        ));
    }
    let target = TargetSelector {
        window: target.window.clone(),
        tab: target.tab.clone(),
        pane: target.pane.clone().or(Some(PaneTarget::Active)),
        session: None,
    };
    let entries = select_pane_entries(&target, ActionKind::SelectionSet, ctx)?;
    let mut applied: Vec<serde_json::Value> = Vec::new();
    for entry in entries {
        let pane_id = entry.pane_id.to_string();
        // Code Review first, matching the read side's carrier order: when a diff
        // editor holds focus it is what the caller is looking at.
        let is_active = entry.pane_group.read(ctx, |pane_group, ctx| {
            pane_group.focused_pane_id(ctx) == entry.pane_id
        });
        let mut done = false;
        if is_active && let Some(cr_views) = ctx.views_of_type::<CodeReviewView>(entry.window_id) {
            for cr in cr_views {
                if cr.update(ctx, |cr, cx| {
                    cr.set_focused_editor_selection_ranges(&ranges, cx)
                }) {
                    applied.push(json!({ "pane_id": pane_id, "carrier": "code_review" }));
                    done = true;
                    break;
                }
            }
        }
        if done {
            continue;
        }
        let code_view = entry.pane_group.read(ctx, |pane_group, ctx| {
            pane_group.code_view_from_pane_id(entry.pane_id, ctx)
        });
        if let Some(code_view) = code_view
            && code_view.update(ctx, |code_view, ctx| {
                code_view.set_active_tab_selection_ranges(&ranges, ctx)
            })
        {
            applied.push(json!({ "pane_id": pane_id, "carrier": "editor" }));
            continue;
        }
        // 终端 pane 落到它的 Rich Input:终端正文本身没有文件行号可指,不可设。
        let terminal_view = entry.pane_group.read(ctx, |pane_group, ctx| {
            pane_group.terminal_view_from_pane_id(entry.pane_id, ctx)
        });
        if let Some(terminal_view) = terminal_view
            && terminal_view.update(ctx, |terminal_view, ctx| {
                terminal_view.set_input_selection_points(&ranges, ctx)
            })
        {
            applied.push(json!({ "pane_id": pane_id, "carrier": "input" }));
        }
    }
    Ok(json!({
        "action": ActionKind::SelectionSet.as_str(),
        "set": applied,
    }))
}

pub(crate) fn pane_inspect(
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    reject_target_families(
        ActionKind::PaneInspect,
        target.session.is_some(),
        "session selectors",
    )?;
    let target = TargetSelector {
        window: target.window.clone(),
        tab: target.tab.clone(),
        pane: target.pane.clone().or(Some(PaneTarget::Active)),
        session: None,
    };
    let data = pane_list(&target, ctx)?;
    let pane = single_entry(data.get("panes"), ActionKind::PaneInspect)?;
    Ok(json!({
        "action": ActionKind::PaneInspect.as_str(),
        "pane": pane,
    }))
}

pub(crate) fn session_list(
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let target = active_session_target(target);
    let pane_entries = select_pane_entries(&target, ActionKind::SessionList, ctx)?;
    let entries = select_session_entries(
        session_entries_for_panes(pane_entries, ctx),
        target.session.as_ref(),
        ActionKind::SessionList,
    )?;
    let sessions = session_values(entries);
    Ok(json!({
        "action": ActionKind::SessionList.as_str(),
        "sessions": sessions,
    }))
}

pub(crate) fn session_inspect(
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let target = active_session_target(target);
    let pane_entries = select_pane_entries(&target, ActionKind::SessionInspect, ctx)?;
    let entries = select_session_entries(
        session_entries_for_panes(pane_entries, ctx),
        target.session.as_ref().or(Some(&SessionTarget::Active)),
        ActionKind::SessionInspect,
    )?;
    let data = json!({ "sessions": session_values(entries) });
    let session = single_entry(data.get("sessions"), ActionKind::SessionInspect)?;
    Ok(json!({
        "action": ActionKind::SessionInspect.as_str(),
        "session": session,
    }))
}

pub(crate) fn action_metadata_for_name(action_name: &str) -> Result<ActionMetadata, ControlError> {
    ActionKind::implemented_metadata()
        .into_iter()
        .find(|metadata| metadata.name == action_name)
        .ok_or_else(|| {
            ControlError::new(
                ErrorCode::NotAllowlisted,
                "requested action is not an implemented local-control action",
            )
        })
}

fn action_name_params(action: &Action) -> Result<ActionNameParams, ControlError> {
    action.params_as()
}

fn to_json_value<T: Serialize>(response: T) -> Result<serde_json::Value, ControlError> {
    serde_json::to_value(response).map_err(|error| {
        ControlError::with_details(
            ErrorCode::Internal,
            "failed to serialize local-control metadata response",
            error.to_string(),
        )
    })
}

fn active_chain(
    instance_id: &Option<InstanceId>,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<ActiveTargetChain, ControlError> {
    let instance_id = instance_id.as_ref().map(|id| id.0.clone());
    let Some(window_id) = ctx.windows().active_window() else {
        return Ok(ActiveTargetChain {
            instance_id,
            window_id: None,
            tab_id: None,
            pane_id: None,
            session_id: None,
        });
    };
    let window_id_string = window_id.to_string();
    let Some(workspace) = workspace_for_window(window_id, ActionKind::AppActive, ctx)? else {
        return Ok(ActiveTargetChain {
            instance_id,
            window_id: Some(window_id_string),
            tab_id: None,
            pane_id: None,
            session_id: None,
        });
    };
    let (tab_id, pane_id, session_id) = workspace.read(ctx, |workspace, ctx| {
        let pane_group = workspace.active_tab_pane_group();
        let pane_group_ref = pane_group.as_ref(ctx);
        (
            Some(pane_group.id().to_string()),
            Some(pane_group_ref.focused_pane_id(ctx).to_string()),
            pane_group_ref
                .active_session_id(ctx)
                .map(|session_id| PaneId::from(session_id).to_string()),
        )
    });
    Ok(ActiveTargetChain {
        instance_id,
        window_id: Some(window_id_string),
        tab_id,
        pane_id,
        session_id,
    })
}

fn select_window_entries(
    target: &TargetSelector,
    force_active_default: bool,
    action: ActionKind,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<Vec<WindowEntry>, ControlError> {
    let entries = window_entries(ctx);
    match target.window.as_ref() {
        None if force_active_default => {
            let active =
                require_active_window_id_for_action(ctx.windows().active_window(), action)?;
            explicit_matches(
                entries
                    .into_iter()
                    .filter(|entry| entry.window_id == active)
                    .collect(),
                action,
                "active window",
                ErrorCode::MissingTarget,
            )
        }
        None => Ok(entries),
        Some(WindowTarget::Active) => {
            let active =
                require_active_window_id_for_action(ctx.windows().active_window(), action)?;
            explicit_matches(
                entries
                    .into_iter()
                    .filter(|entry| entry.window_id == active)
                    .collect(),
                action,
                "active window",
                ErrorCode::MissingTarget,
            )
        }
        Some(WindowTarget::Id { id }) => explicit_matches(
            entries
                .into_iter()
                .filter(|entry| entry.window_id.to_string() == id.0)
                .collect(),
            action,
            "window id",
            ErrorCode::StaleTarget,
        ),
        Some(WindowTarget::Index { index }) => explicit_matches(
            entries
                .into_iter()
                .filter(|entry| entry.index as u32 == *index)
                .collect(),
            action,
            "window index",
            ErrorCode::StaleTarget,
        ),
        Some(WindowTarget::Title { .. }) => Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!(
                "{} only supports active, opaque window id, and window index selectors",
                action.as_str()
            ),
        )),
    }
}

fn select_tab_entries(
    target: &TargetSelector,
    action: ActionKind,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<Vec<TabEntry>, ControlError> {
    let force_active_window = matches!(
        target.tab,
        Some(TabTarget::Active | TabTarget::Index { .. })
    ) || matches!(
        target.pane,
        Some(PaneTarget::Active | PaneTarget::Index { .. })
    );
    let windows = select_window_entries(target, force_active_window, action, ctx)?;
    let entries = tab_entries_for_windows(windows, action, ctx)?;
    let requires_active_tab_default = matches!(
        target.pane,
        Some(PaneTarget::Active | PaneTarget::Index { .. })
    );
    match target.tab.as_ref() {
        None if requires_active_tab_default => explicit_matches(
            entries
                .into_iter()
                .filter(|entry| entry.index == entry.workspace_active_tab_index)
                .collect(),
            action,
            "active tab",
            ErrorCode::MissingTarget,
        ),
        None => Ok(entries),
        Some(TabTarget::Active) => explicit_matches(
            entries
                .into_iter()
                .filter(|entry| entry.index == entry.workspace_active_tab_index)
                .collect(),
            action,
            "active tab",
            ErrorCode::MissingTarget,
        ),
        Some(TabTarget::Id { id }) => explicit_matches(
            entries
                .into_iter()
                .filter(|entry| entry.pane_group.id().to_string() == id.0)
                .collect(),
            action,
            "tab id",
            ErrorCode::StaleTarget,
        ),
        Some(TabTarget::Index { index }) => explicit_matches(
            entries
                .into_iter()
                .filter(|entry| entry.index as u32 == *index)
                .collect(),
            action,
            "tab index",
            ErrorCode::StaleTarget,
        ),
        Some(TabTarget::Title { .. }) => Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!(
                "{} only supports active, opaque tab id, and tab index selectors",
                action.as_str()
            ),
        )),
    }
}

fn select_pane_entries(
    target: &TargetSelector,
    action: ActionKind,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<Vec<PaneEntry>, ControlError> {
    let tabs = select_tab_entries(target, action, ctx)?;
    let entries = pane_entries_for_tabs(tabs, ctx);
    match target.pane.as_ref() {
        None => Ok(entries),
        Some(PaneTarget::Active) => explicit_matches(
            entries
                .into_iter()
                .filter(|entry| {
                    entry.pane_group.read(ctx, |pane_group, ctx| {
                        pane_group.focused_pane_id(ctx) == entry.pane_id
                    })
                })
                .collect(),
            action,
            "active pane",
            ErrorCode::MissingTarget,
        ),
        Some(PaneTarget::Id { id }) => explicit_matches(
            entries
                .into_iter()
                .filter(|entry| entry.pane_id.to_string() == id.0)
                .collect(),
            action,
            "pane id",
            ErrorCode::StaleTarget,
        ),
        Some(PaneTarget::Index { index }) => explicit_matches(
            entries
                .into_iter()
                .filter(|entry| entry.index as u32 == *index)
                .collect(),
            action,
            "pane index",
            ErrorCode::StaleTarget,
        ),
    }
}

fn window_entries(ctx: &mut ModelContext<LocalControlBridge>) -> Vec<WindowEntry> {
    let mut ids = ctx.window_ids().collect::<Vec<_>>();
    ids.sort_by_key(ToString::to_string);
    ids.into_iter()
        .enumerate()
        .map(|(index, window_id)| WindowEntry { window_id, index })
        .collect()
}

pub(super) fn tab_entries_for_windows(
    windows: Vec<WindowEntry>,
    action: ActionKind,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<Vec<TabEntry>, ControlError> {
    let mut entries = Vec::new();
    for window in windows {
        let Some(workspace) = workspace_for_window(window.window_id, action, ctx)? else {
            continue;
        };
        entries.extend(workspace.read(ctx, |workspace, _| {
            workspace
                .tab_views()
                .enumerate()
                .map(|(index, pane_group)| TabEntry {
                    window_id: window.window_id,
                    window_index: window.index,
                    index,
                    workspace_active_tab_index: workspace.active_tab_index(),
                    pane_group: pane_group.clone(),
                })
                .collect::<Vec<_>>()
        }));
    }
    Ok(entries)
}

pub(super) fn pane_entries_for_tabs(
    tabs: Vec<TabEntry>,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Vec<PaneEntry> {
    let mut entries = Vec::new();
    for tab in tabs {
        let tab_id = tab.pane_group.id().to_string();
        let pane_group = tab.pane_group.clone();
        let pane_ids = tab
            .pane_group
            .read(ctx, |pane_group, _| pane_group.visible_pane_ids());
        entries.extend(
            pane_ids
                .into_iter()
                .enumerate()
                .map(|(index, pane_id)| PaneEntry {
                    window_id: tab.window_id,
                    window_index: tab.window_index,
                    tab_id: tab_id.clone(),
                    tab_index: tab.index,
                    index,
                    pane_group: pane_group.clone(),
                    pane_id,
                }),
        );
    }
    entries
}

fn session_entries_for_panes(
    panes: Vec<PaneEntry>,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Vec<SessionEntry> {
    let mut entries = Vec::new();
    for pane in panes {
        let (has_terminal_session, is_active) = pane.pane_group.read(ctx, |pane_group, ctx| {
            (
                pane_group
                    .terminal_view_from_pane_id(pane.pane_id, ctx)
                    .is_some(),
                pane_group.active_session_id(ctx).map(PaneId::from) == Some(pane.pane_id),
            )
        });
        if has_terminal_session {
            entries.push(SessionEntry {
                window_id: pane.window_id,
                window_index: pane.window_index,
                tab_id: pane.tab_id,
                tab_index: pane.tab_index,
                pane_id: pane.pane_id,
                pane_index: pane.index,
                is_active,
            });
        }
    }
    entries
}

fn session_values(entries: Vec<SessionEntry>) -> Vec<Value> {
    entries
        .into_iter()
        .map(|entry| {
            json!({
                "session_id": entry.pane_id.to_string(),
                "pane_id": entry.pane_id.to_string(),
                "pane_index": entry.pane_index as u32,
                "tab_id": entry.tab_id,
                "tab_index": entry.tab_index as u32,
                "window_id": entry.window_id.to_string(),
                "window_index": entry.window_index as u32,
                "is_active": entry.is_active,
            })
        })
        .collect()
}

fn workspace_for_window(
    window_id: WindowId,
    action: ActionKind,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<Option<ViewHandle<Workspace>>, ControlError> {
    match ctx.views_of_type::<Workspace>(window_id) {
        None => Ok(None),
        Some(workspaces) => match workspaces.as_slice() {
            [] => Ok(None),
            [workspace] => Ok(Some(workspace.clone())),
            _ => Err(ControlError::new(
                ErrorCode::AmbiguousTarget,
                format!(
                    "{} resolved multiple workspaces in one window",
                    action.as_str()
                ),
            )),
        },
    }
}

fn explicit_matches<T>(
    matches: Vec<T>,
    action: ActionKind,
    selector: &str,
    missing_code: ErrorCode,
) -> Result<Vec<T>, ControlError> {
    match matches.len() {
        0 => Err(ControlError::new(
            missing_code,
            format!(
                "{} cannot resolve the requested {selector}",
                action.as_str()
            ),
        )),
        1 => Ok(matches),
        _ => Err(ControlError::new(
            ErrorCode::AmbiguousTarget,
            format!(
                "{} resolved multiple targets by {selector}",
                action.as_str()
            ),
        )),
    }
}

fn single_entry(value: Option<&Value>, action: ActionKind) -> Result<Value, ControlError> {
    let Some(items) = value.and_then(Value::as_array) else {
        return Err(ControlError::new(
            ErrorCode::Internal,
            format!("{} handler returned malformed metadata", action.as_str()),
        ));
    };
    match items.as_slice() {
        [item] => Ok(item.clone()),
        [] => Err(ControlError::new(
            ErrorCode::MissingTarget,
            format!("{} could not resolve a target", action.as_str()),
        )),
        _ => Err(ControlError::new(
            ErrorCode::AmbiguousTarget,
            format!("{} resolved multiple targets", action.as_str()),
        )),
    }
}
