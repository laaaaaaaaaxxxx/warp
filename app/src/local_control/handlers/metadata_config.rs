//! Metadata/configuration mutation handlers for local-control actions.
use std::str::FromStr as _;

use ::local_control::protocol::{
    BackgroundParams, BooleanValueParams, ColorValueParams, Direction, DirectionParams, KeyParams,
    KeyValueParams, PaneTarget, RenameParams, TabGroupMoveParams, TabSelector, TabTarget,
    TargetSelector, ThemeNameParams, WindowTarget,
};
use ::local_control::{ActionKind, ControlError, ErrorCode, InstanceId};
use serde_json::json;
use settings::Setting as _;
use warp_core::ui::theme::AnsiColorIdentifier;
use warpui::{ModelContext, SingletonEntity as _, TypedActionView as _, ViewHandle, WindowId};

use super::metadata::{
    PaneEntry, TabEntry, WindowEntry, pane_entries_for_tabs, tab_entries_for_windows,
};
use super::settings_surfaces::{
    ALLOWLISTED_SETTING_KEYS, public_theme_name, rejected_setting_key, setting_summary_for_key,
};
use crate::WindowSettings;
use crate::local_control::LocalControlBridge;
use crate::local_control::handlers::ack;
use crate::local_control::resolver::{require_active_window_id_for_action, workspace_for_window};
use crate::pane_group::PaneId;
use crate::settings::{AccessibilitySettings, FontSettings, InputSettings, ThemeSettings};
use crate::tab::SelectedTabColor;
use crate::themes::theme::{SelectedSystemThemes, ThemeKind};
use crate::user_config::WarpConfig;
use crate::window_settings::ZoomLevel;
use crate::workspace::tab_group::TabGroupId;
use crate::workspace::{Workspace, WorkspaceAction};

pub(crate) fn tab_rename(
    instance_id: &Option<InstanceId>,
    target: &TargetSelector,
    action: &::local_control::Action,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let title = rename_title(action)?;
    let entry = select_single_tab_entry(target, ActionKind::TabRename, ctx)?;
    let tab_id = entry.pane_group.id().to_string();
    entry.pane_group.update(ctx, |pane_group, ctx| {
        pane_group.set_title(&title, ctx);
    });
    Ok(tab_mutation_result(
        instance_id,
        ActionKind::TabRename,
        tab_id,
        entry.window_id,
    ))
}

pub(crate) fn tab_reset_name(
    instance_id: &Option<InstanceId>,
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let entry = select_single_tab_entry(target, ActionKind::TabResetName, ctx)?;
    let tab_id = entry.pane_group.id().to_string();
    entry.pane_group.update(ctx, |pane_group, ctx| {
        pane_group.clear_title(ctx);
    });
    Ok(tab_mutation_result(
        instance_id,
        ActionKind::TabResetName,
        tab_id,
        entry.window_id,
    ))
}

pub(crate) fn tab_color_set(
    instance_id: &Option<InstanceId>,
    target: &TargetSelector,
    action: &::local_control::Action,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let color = color_value(action)?;
    let entry = select_single_tab_entry(target, ActionKind::TabColorSet, ctx)?;
    set_tab_color(entry.clone(), SelectedTabColor::Color(color), ctx)?;
    Ok(tab_mutation_result(
        instance_id,
        ActionKind::TabColorSet,
        entry.pane_group.id().to_string(),
        entry.window_id,
    ))
}

pub(crate) fn tab_color_clear(
    instance_id: &Option<InstanceId>,
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let entry = select_single_tab_entry(target, ActionKind::TabColorClear, ctx)?;
    set_tab_color(entry.clone(), SelectedTabColor::Cleared, ctx)?;
    Ok(tab_mutation_result(
        instance_id,
        ActionKind::TabColorClear,
        entry.pane_group.id().to_string(),
        entry.window_id,
    ))
}

pub(crate) fn pane_rename(
    instance_id: &Option<InstanceId>,
    target: &TargetSelector,
    action: &::local_control::Action,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let title = rename_title(action)?;
    let entry = select_single_pane_entry(target, ActionKind::PaneRename, ctx)?;
    set_pane_name(&entry, Some(title), ctx)?;
    Ok(pane_mutation_result(
        instance_id,
        ActionKind::PaneRename,
        entry.pane_id,
        entry.tab_id,
    ))
}

pub(crate) fn pane_reset_name(
    instance_id: &Option<InstanceId>,
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let entry = select_single_pane_entry(target, ActionKind::PaneResetName, ctx)?;
    set_pane_name(&entry, None, ctx)?;
    Ok(pane_mutation_result(
        instance_id,
        ActionKind::PaneResetName,
        entry.pane_id,
        entry.tab_id,
    ))
}

pub(crate) fn theme_set(
    instance_id: &Option<InstanceId>,
    action_kind: ActionKind,
    action: &::local_control::Action,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    match action_kind {
        ActionKind::ThemeSet => set_theme(theme_name(action)?, ctx)?,
        ActionKind::ThemeSystemSet => set_system_theme(boolean_value(action)?, ctx)?,
        ActionKind::ThemeLightSet => set_system_theme_variant(theme_name(action)?, true, ctx)?,
        ActionKind::ThemeDarkSet => set_system_theme_variant(theme_name(action)?, false, ctx)?,
        _ => {
            return Err(ControlError::new(
                ErrorCode::UnsupportedAction,
                format!("{} is not a theme mutation", action_kind.as_str()),
            ));
        }
    }
    Ok(ack(instance_id, action_kind))
}

pub(crate) fn appearance_mutation(
    instance_id: &Option<InstanceId>,
    action_kind: ActionKind,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    match action_kind {
        ActionKind::AppearanceFontSizeIncrease => {
            adjust_font_size(FontSizeAdjustment::Increase, ctx)?
        }
        ActionKind::AppearanceFontSizeDecrease => {
            adjust_font_size(FontSizeAdjustment::Decrease, ctx)?
        }
        ActionKind::AppearanceFontSizeReset => adjust_font_size(FontSizeAdjustment::Reset, ctx)?,
        ActionKind::AppearanceZoomIncrease => adjust_zoom(true, ctx)?,
        ActionKind::AppearanceZoomDecrease => adjust_zoom(false, ctx)?,
        ActionKind::AppearanceZoomReset => reset_zoom(ctx)?,
        _ => {
            return Err(ControlError::new(
                ErrorCode::UnsupportedAction,
                format!("{} is not an appearance mutation", action_kind.as_str()),
            ));
        }
    }
    Ok(ack(instance_id, action_kind))
}

pub(crate) fn setting_set(
    action: &::local_control::Action,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let (key, value) = key_value(action)?;
    set_allowlisted_setting(&key, value, ctx)?;
    Ok(json!({
        "action": ActionKind::SettingSet.as_str(),
        "setting": setting_summary_for_key(&key, ctx)?,
    }))
}

pub(crate) fn setting_toggle(
    action: &::local_control::Action,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let key = key_value_key(action)?;
    let current = setting_summary_for_key(&key, ctx)?;
    let Some(value) = current.value.as_bool() else {
        return Err(ControlError::new(
            ErrorCode::InvalidParams,
            format!("{key} is not a boolean setting and cannot be toggled"),
        ));
    };
    set_allowlisted_setting(&key, json!(!value), ctx)?;
    Ok(json!({
        "action": ActionKind::SettingToggle.as_str(),
        "setting": setting_summary_for_key(&key, ctx)?,
    }))
}

/// Resolves the tab group the targeted tab belongs to.
///
/// Groups are addressed through a member tab rather than a selector of their
/// own: a `TabGroupId` is a Uuid minted in memory and minted afresh on restart,
/// so it is an identity to read back, never a coordinate to type. Every group
/// holds at least one tab, so a member always reaches it.
fn tab_group_id_for_entry(
    entry: &TabEntry,
    action: ActionKind,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<TabGroupId, ControlError> {
    let workspace = workspace_for_window(entry.window_id, action, ctx)?;
    let tab_pane_group_id = entry.pane_group.id();
    let group_id = workspace.read(ctx, |workspace, _| {
        workspace
            .tabs
            .iter()
            .find(|tab| tab.pane_group.id() == tab_pane_group_id)
            .and_then(|tab| tab.group_id)
    });
    group_id.ok_or_else(|| {
        ControlError::new(
            ErrorCode::MissingTarget,
            format!(
                "{} requires a tab that belongs to a tab group",
                action.as_str()
            ),
        )
    })
}

/// Puts the targeted tab into a new group and reports the group it landed in.
///
/// The id is read back off the tab rather than minted here, because the action
/// also moves the tab -- new groups are placed below pinned items -- so its
/// index afterwards is not the index that went in.
pub(crate) fn tab_group_create(
    instance_id: &Option<InstanceId>,
    target: &TargetSelector,
    action: &::local_control::Action,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let background = background_flag(action)?;
    let entry = select_single_tab_entry(target, ActionKind::TabGroupCreate, ctx)?;
    let workspace = workspace_for_window(entry.window_id, ActionKind::TabGroupCreate, ctx)?;
    workspace.update(ctx, |workspace, ctx| {
        // Called directly instead of through `NewTabGroupFromTab`: the action
        // form ends by activating the grouped tab, which is right for the mouse
        // path that dispatches it and wrong for a caller naming a tab it is not
        // looking at.
        workspace.new_tab_group_from_tab(entry.index, !background, ctx);
        // Creating a group ends by deferring `RenameTabGroup`, which opens an
        // inline editor over the group header seeded with "New Group". That is
        // right for a mouse, wrong for a caller with no keyboard: the editor
        // hides the header and, whenever it later commits, writes its own
        // buffer over whatever name was set. Cancel it the same deferred way so
        // the cancel lands after the open.
        workspace.handle_action(&WorkspaceAction::CancelActiveRename, ctx);
    });
    let group_id = tab_group_id_for_entry(&entry, ActionKind::TabGroupCreate, ctx)?;
    let mut response = tab_mutation_result(
        instance_id,
        ActionKind::TabGroupCreate,
        entry.pane_group.id().to_string(),
        entry.window_id,
    );
    response["group"] = json!({ "id": group_id.0.to_string() });
    Ok(response)
}

/// Renames the group the targeted tab belongs to.
pub(crate) fn tab_group_rename(
    instance_id: &Option<InstanceId>,
    target: &TargetSelector,
    action: &::local_control::Action,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let title = rename_title(action)?;
    let entry = select_single_tab_entry(target, ActionKind::TabGroupRename, ctx)?;
    let group_id = tab_group_id_for_entry(&entry, ActionKind::TabGroupRename, ctx)?;
    let workspace = workspace_for_window(entry.window_id, ActionKind::TabGroupRename, ctx)?;
    workspace.update(ctx, |workspace, ctx| {
        // An inline rename editor left open over this group would overwrite the
        // name the moment it commits, so close it before writing.
        workspace.handle_action(&WorkspaceAction::CancelActiveRename, ctx);
        workspace.set_tab_group_name(group_id, title, ctx);
    });
    Ok(tab_mutation_result(
        instance_id,
        ActionKind::TabGroupRename,
        entry.pane_group.id().to_string(),
        entry.window_id,
    ))
}

/// Closes every tab in the group the targeted tab belongs to, the target
/// included.
pub(crate) fn tab_group_close(
    instance_id: &Option<InstanceId>,
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let entry = select_single_tab_entry(target, ActionKind::TabGroupClose, ctx)?;
    let group_id = tab_group_id_for_entry(&entry, ActionKind::TabGroupClose, ctx)?;
    let workspace = workspace_for_window(entry.window_id, ActionKind::TabGroupClose, ctx)?;
    workspace.update(ctx, |workspace, ctx| {
        workspace.handle_action(&WorkspaceAction::CloseTabGroup(group_id), ctx);
    });
    Ok(ack(instance_id, ActionKind::TabGroupClose))
}

/// Closes every tab above the group the targeted tab belongs to. The group
/// itself and everything below it stay.
pub(crate) fn tab_group_close_above(
    instance_id: &Option<InstanceId>,
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let entry = select_single_tab_entry(target, ActionKind::TabGroupCloseAbove, ctx)?;
    let group_id = tab_group_id_for_entry(&entry, ActionKind::TabGroupCloseAbove, ctx)?;
    let workspace = workspace_for_window(entry.window_id, ActionKind::TabGroupCloseAbove, ctx)?;
    workspace.update(ctx, |workspace, ctx| {
        workspace.handle_action(&WorkspaceAction::CloseTabsAboveGroup(group_id), ctx);
    });
    Ok(ack(instance_id, ActionKind::TabGroupCloseAbove))
}

/// Opens a tab inside the group the targeted tab belongs to.
pub(crate) fn tab_group_new_tab(
    instance_id: &Option<InstanceId>,
    target: &TargetSelector,
    action: &::local_control::Action,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let background = background_flag(action)?;
    let entry = select_single_tab_entry(target, ActionKind::TabGroupNewTab, ctx)?;
    let group_id = tab_group_id_for_entry(&entry, ActionKind::TabGroupNewTab, ctx)?;
    let workspace = workspace_for_window(entry.window_id, ActionKind::TabGroupNewTab, ctx)?;
    let new_tab_id = workspace.update(ctx, |workspace, ctx| {
        let index = workspace
            .new_tab_in_group(group_id, !background, ctx)
            .ok_or_else(|| {
                ControlError::new(
                    ErrorCode::MissingTarget,
                    "tab.group.new_tab could not open a tab in that group",
                )
            })?;
        workspace
            .get_pane_group_view(index)
            .map(|view| view.id().to_string())
            .ok_or_else(|| {
                ControlError::new(
                    ErrorCode::Internal,
                    "tab.group.new_tab did not produce a tab identifier",
                )
            })
    })?;
    let mut response = tab_mutation_result(
        instance_id,
        ActionKind::TabGroupNewTab,
        new_tab_id,
        entry.window_id,
    );
    response["group"] = json!({ "id": group_id.0.to_string() });
    Ok(response)
}

/// Dissolves the group the targeted tab belongs to. The tabs stay; only the
/// grouping goes.
pub(crate) fn tab_group_ungroup(
    instance_id: &Option<InstanceId>,
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let entry = select_single_tab_entry(target, ActionKind::TabGroupUngroup, ctx)?;
    let group_id = tab_group_id_for_entry(&entry, ActionKind::TabGroupUngroup, ctx)?;
    let workspace = workspace_for_window(entry.window_id, ActionKind::TabGroupUngroup, ctx)?;
    workspace.update(ctx, |workspace, ctx| {
        workspace.handle_action(&WorkspaceAction::UngroupTabs(group_id), ctx);
    });
    Ok(tab_mutation_result(
        instance_id,
        ActionKind::TabGroupUngroup,
        entry.pane_group.id().to_string(),
        entry.window_id,
    ))
}

/// Takes the targeted tab out of its group, leaving the group and its other
/// members alone.
pub(crate) fn tab_group_remove_tab(
    instance_id: &Option<InstanceId>,
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let entry = select_single_tab_entry(target, ActionKind::TabGroupRemoveTab, ctx)?;
    // Resolved for its membership check: a tab outside any group has nothing to
    // be removed from, and this is what says so.
    tab_group_id_for_entry(&entry, ActionKind::TabGroupRemoveTab, ctx)?;
    let workspace = workspace_for_window(entry.window_id, ActionKind::TabGroupRemoveTab, ctx)?;
    workspace.update(ctx, |workspace, ctx| {
        workspace.handle_action(&WorkspaceAction::RemoveTabFromGroup(entry.index), ctx);
    });
    Ok(tab_mutation_result(
        instance_id,
        ActionKind::TabGroupRemoveTab,
        entry.pane_group.id().to_string(),
        entry.window_id,
    ))
}

/// Moves the targeted tab into the group that the destination tab belongs to.
pub(crate) fn tab_group_move_tab(
    instance_id: &Option<InstanceId>,
    target: &TargetSelector,
    action: &::local_control::Action,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let TabGroupMoveParams { destination_tab } = action.params_as()?;
    let entry = select_single_tab_entry(target, ActionKind::TabGroupMoveTab, ctx)?;
    let destination =
        destination_tab_entry(&destination_tab, target, ActionKind::TabGroupMoveTab, ctx)?;
    if destination.pane_group.id() == entry.pane_group.id() {
        return Err(ControlError::new(
            ErrorCode::InvalidParams,
            "tab.group.move_tab was given the tab it is moving as the destination",
        ));
    }
    let group_id = tab_group_id_for_entry(&destination, ActionKind::TabGroupMoveTab, ctx)?;
    let workspace = workspace_for_window(entry.window_id, ActionKind::TabGroupMoveTab, ctx)?;
    workspace.update(ctx, |workspace, ctx| {
        workspace.handle_action(
            &WorkspaceAction::MoveTabToGroup {
                tab_index: entry.index,
                group_id,
            },
            ctx,
        );
    });
    let mut response = tab_mutation_result(
        instance_id,
        ActionKind::TabGroupMoveTab,
        entry.pane_group.id().to_string(),
        entry.window_id,
    );
    response["group"] = json!({ "id": group_id.0.to_string() });
    Ok(response)
}

/// Moves the whole group the targeted tab belongs to, one slot up or down.
///
/// A grouped tab cannot leave its group by reordering -- upstream refuses that
/// to keep a group's run contiguous -- so moving the group is how a grouped tab
/// changes neighbourhood. The response reports the group's index afterwards and
/// whether it actually shifted, because upstream declines some moves (a pinned
/// neighbour, an end of the list) without reporting anything.
pub(crate) fn tab_group_move(
    instance_id: &Option<InstanceId>,
    target: &TargetSelector,
    action: &::local_control::Action,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let DirectionParams { direction } = action.params_as()?;
    let workspace_action = match direction {
        Direction::Up => WorkspaceAction::MoveTabGroupUp,
        Direction::Down => WorkspaceAction::MoveTabGroupDown,
        Direction::Left | Direction::Right | Direction::Previous | Direction::Next => {
            return Err(ControlError::new(
                ErrorCode::InvalidParams,
                "tab.group.move only accepts up or down",
            ));
        }
    };
    let entry = select_single_tab_entry(target, ActionKind::TabGroupMove, ctx)?;
    let group_id = tab_group_id_for_entry(&entry, ActionKind::TabGroupMove, ctx)?;
    let workspace = workspace_for_window(entry.window_id, ActionKind::TabGroupMove, ctx)?;
    let index_before = group_first_index(&workspace, group_id, ctx);
    workspace.update(ctx, |workspace, ctx| {
        workspace.handle_action(&workspace_action(group_id), ctx);
    });
    let index_after = group_first_index(&workspace, group_id, ctx);
    let mut response = tab_mutation_result(
        instance_id,
        ActionKind::TabGroupMove,
        entry.pane_group.id().to_string(),
        entry.window_id,
    );
    response["group"] = json!({
        "id": group_id.0.to_string(),
        "index": index_after,
        "moved": index_before != index_after,
    });
    Ok(response)
}

/// Where the group's first member sits in the tab strip right now.
fn group_first_index(
    workspace: &ViewHandle<Workspace>,
    group_id: TabGroupId,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Option<usize> {
    workspace.read(ctx, |workspace, _| {
        workspace
            .tabs
            .iter()
            .position(|tab| tab.group_id == Some(group_id))
    })
}

/// Resolves the tab named in params, scoped to the same window the request
/// already picked, so one `--window` covers both ends of a move.
fn destination_tab_entry(
    id: &TabSelector,
    target: &TargetSelector,
    action: ActionKind,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<TabEntry, ControlError> {
    let destination_target = TargetSelector {
        window: target.window.clone(),
        tab: Some(TabTarget::Id { id: id.clone() }),
        pane: None,
        session: None,
    };
    select_single_tab_entry(&destination_target, action, ctx)
}

fn background_flag(action: &::local_control::Action) -> Result<bool, ControlError> {
    let BackgroundParams { background } = action.params_as()?;
    Ok(background)
}

fn rename_title(action: &::local_control::Action) -> Result<String, ControlError> {
    let RenameParams { title } = action.params_as()?;
    if title.trim().is_empty() {
        Err(ControlError::new(
            ErrorCode::InvalidParams,
            format!("{} title cannot be empty", action.kind.as_str()),
        ))
    } else {
        Ok(title.trim().to_owned())
    }
}

fn color_value(action: &::local_control::Action) -> Result<AnsiColorIdentifier, ControlError> {
    let ColorValueParams { color } = action.params_as()?;
    AnsiColorIdentifier::from_str(&color).map_err(|_| {
        ControlError::new(
            ErrorCode::InvalidParams,
            format!("{color} is not a supported tab color"),
        )
    })
}

fn theme_name(action: &::local_control::Action) -> Result<String, ControlError> {
    let ThemeNameParams { theme_name } = action.params_as()?;
    if theme_name.trim().is_empty() {
        Err(ControlError::new(
            ErrorCode::InvalidParams,
            format!("{} theme name cannot be empty", action.kind.as_str()),
        ))
    } else {
        Ok(theme_name.trim().to_owned())
    }
}

fn boolean_value(action: &::local_control::Action) -> Result<bool, ControlError> {
    Ok(action.params_as::<BooleanValueParams>()?.value)
}

fn key_value(
    action: &::local_control::Action,
) -> Result<(String, serde_json::Value), ControlError> {
    let KeyValueParams { key, value } = action.params_as()?;
    Ok((key, value))
}

fn key_value_key(action: &::local_control::Action) -> Result<String, ControlError> {
    Ok(action.params_as::<KeyParams>()?.key)
}

fn select_single_tab_entry(
    target: &TargetSelector,
    action: ActionKind,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<TabEntry, ControlError> {
    if target.pane.is_some() || target.session.is_some() {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!(
                "{} does not accept pane or session selectors",
                action.as_str()
            ),
        ));
    }
    let entries = select_tab_entries(target, action, ctx)?;
    match entries.as_slice() {
        [entry] => Ok(entry.clone()),
        [] => Err(ControlError::new(
            ErrorCode::MissingTarget,
            format!("{} requires a target tab", action.as_str()),
        )),
        _ => Err(ControlError::new(
            ErrorCode::AmbiguousTarget,
            format!("{} resolved multiple tabs", action.as_str()),
        )),
    }
}

fn select_single_pane_entry(
    target: &TargetSelector,
    action: ActionKind,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<PaneEntry, ControlError> {
    if target.session.is_some() {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!("{} does not accept session selectors", action.as_str()),
        ));
    }
    let tab = select_single_tab_entry_for_pane(target, action, ctx)?;
    let entries = pane_entries_for_tabs(vec![tab], ctx);
    if entries.is_empty() {
        return Err(ControlError::new(
            ErrorCode::MissingTarget,
            "target tab has no visible panes",
        ));
    }
    let selected = match target.pane.as_ref() {
        None | Some(PaneTarget::Active) => {
            let focused = entries
                .first()
                .map(|entry| {
                    entry
                        .pane_group
                        .read(ctx, |pane_group, ctx| pane_group.focused_pane_id(ctx))
                })
                .ok_or_else(|| {
                    ControlError::new(
                        ErrorCode::MissingTarget,
                        format!("{} requires an active pane", action.as_str()),
                    )
                })?;
            entries
                .into_iter()
                .filter(|entry| entry.pane_id == focused)
                .collect::<Vec<_>>()
        }
        Some(PaneTarget::Id { id }) => entries
            .into_iter()
            .filter(|entry| entry.pane_id.to_string() == id.0)
            .collect::<Vec<_>>(),
        Some(PaneTarget::Index { index }) => entries
            .into_iter()
            .filter(|entry| entry.index as u32 == *index)
            .collect::<Vec<_>>(),
    };
    match selected.as_slice() {
        [entry] => Ok(entry.clone()),
        [] => Err(ControlError::new(
            ErrorCode::StaleTarget,
            format!("{} cannot resolve the requested pane", action.as_str()),
        )),
        _ => Err(ControlError::new(
            ErrorCode::AmbiguousTarget,
            format!("{} resolved multiple panes", action.as_str()),
        )),
    }
}

fn select_single_tab_entry_for_pane(
    target: &TargetSelector,
    action: ActionKind,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<TabEntry, ControlError> {
    let entries = select_tab_entries(target, action, ctx)?;
    match entries.as_slice() {
        [entry] => Ok(entry.clone()),
        [] => Err(ControlError::new(
            ErrorCode::MissingTarget,
            format!("{} requires a target tab", action.as_str()),
        )),
        _ => Err(ControlError::new(
            ErrorCode::AmbiguousTarget,
            format!("{} resolved multiple tabs", action.as_str()),
        )),
    }
}

fn select_tab_entries(
    target: &TargetSelector,
    action: ActionKind,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<Vec<TabEntry>, ControlError> {
    let windows = select_window_ids(target, action, ctx)?
        .into_iter()
        .enumerate()
        .map(|(index, window_id)| WindowEntry { window_id, index })
        .collect::<Vec<_>>();
    let entries = tab_entries_for_windows(windows, action, ctx)?;
    match target.tab.as_ref() {
        None | Some(TabTarget::Active) => Ok(entries
            .into_iter()
            .filter(|entry| entry.index == entry.workspace_active_tab_index)
            .collect()),
        Some(TabTarget::Id { id }) => Ok(entries
            .into_iter()
            .filter(|entry| entry.pane_group.id().to_string() == id.0)
            .collect()),
        Some(TabTarget::Index { index }) => Ok(entries
            .into_iter()
            .filter(|entry| entry.index as u32 == *index)
            .collect()),
        Some(TabTarget::Title { title }) => Ok(entries
            .into_iter()
            .filter(|entry| {
                entry.pane_group.read(ctx, |pane_group, ctx| {
                    pane_group.display_title(ctx) == *title
                })
            })
            .collect()),
    }
}

fn select_window_ids(
    target: &TargetSelector,
    action: ActionKind,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<Vec<WindowId>, ControlError> {
    match target.window.as_ref() {
        None | Some(WindowTarget::Active) => Ok(vec![require_active_window_id_for_action(
            ctx.windows().active_window(),
            action,
        )?]),
        Some(WindowTarget::Id { id }) => ctx
            .window_ids()
            .find(|window_id| window_id.to_string() == id.0)
            .map(|window_id| vec![window_id])
            .ok_or_else(|| {
                ControlError::new(
                    ErrorCode::StaleTarget,
                    format!("{} cannot resolve the requested window id", action.as_str()),
                )
            }),
        Some(WindowTarget::Index { .. } | WindowTarget::Title { .. }) => Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!(
                "{} only supports active and opaque window id selectors",
                action.as_str()
            ),
        )),
    }
}

fn set_tab_color(
    entry: TabEntry,
    color: SelectedTabColor,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<(), ControlError> {
    let workspace = workspace_for_window(entry.window_id, ActionKind::TabColorSet, ctx)?;
    workspace.update(ctx, |workspace, ctx| {
        workspace.set_tab_color(entry.index, color, ctx);
    });
    Ok(())
}

fn set_pane_name(
    entry: &PaneEntry,
    title: Option<String>,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<(), ControlError> {
    entry.pane_group.update(ctx, |pane_group, ctx| {
        let Some(pane) = pane_group.pane_by_id(entry.pane_id) else {
            return Err(ControlError::new(
                ErrorCode::StaleTarget,
                "pane metadata mutation cannot resolve the requested pane",
            ));
        };
        pane.pane_configuration().update(ctx, |configuration, ctx| {
            if let Some(title) = title {
                configuration.set_custom_vertical_tabs_title(title, ctx);
            } else {
                configuration.clear_custom_vertical_tabs_title(ctx);
            }
        });
        ctx.emit(crate::pane_group::Event::AppStateChanged);
        Ok(())
    })
}

fn set_theme(
    theme_name: String,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<(), ControlError> {
    let theme = theme_kind_for_name(&theme_name, ctx)?;
    ThemeSettings::handle(ctx)
        .update(ctx, |theme_settings, ctx| {
            theme_settings.use_system_theme.set_value(false, ctx)?;
            theme_settings.theme_kind.set_value(theme, ctx)
        })
        .map_err(|err| settings_write_error(ActionKind::ThemeSet, err))
}

fn set_system_theme(
    enabled: bool,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<(), ControlError> {
    ThemeSettings::handle(ctx)
        .update(ctx, |theme_settings, ctx| {
            theme_settings.use_system_theme.set_value(enabled, ctx)
        })
        .map_err(|err| settings_write_error(ActionKind::ThemeSystemSet, err))
}

fn set_system_theme_variant(
    theme_name: String,
    light: bool,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<(), ControlError> {
    let theme = theme_kind_for_name(&theme_name, ctx)?;
    let action = if light {
        ActionKind::ThemeLightSet
    } else {
        ActionKind::ThemeDarkSet
    };
    ThemeSettings::handle(ctx)
        .update(ctx, |theme_settings, ctx| {
            let current = theme_settings.selected_system_themes.value().clone();
            let next = if light {
                SelectedSystemThemes {
                    light: theme,
                    dark: current.dark,
                }
            } else {
                SelectedSystemThemes {
                    light: current.light,
                    dark: theme,
                }
            };
            theme_settings.selected_system_themes.set_value(next, ctx)
        })
        .map_err(|err| settings_write_error(action, err))
}

enum FontSizeAdjustment {
    Increase,
    Decrease,
    Reset,
}

fn adjust_font_size(
    adjustment: FontSizeAdjustment,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<(), ControlError> {
    let current = *FontSettings::as_ref(ctx).monospace_font_size.value();
    let next = match adjustment {
        FontSizeAdjustment::Increase => (current + 1.0).clamp(5.0, 25.0),
        FontSizeAdjustment::Decrease => (current - 1.0).clamp(5.0, 25.0),
        FontSizeAdjustment::Reset => crate::settings::MonospaceFontSize::default_value(),
    };
    FontSettings::handle(ctx)
        .update(ctx, |font_settings, ctx| {
            font_settings.monospace_font_size.set_value(next, ctx)
        })
        .map_err(|err| settings_write_error(ActionKind::AppearanceFontSizeReset, err))
}

fn adjust_zoom(
    increase: bool,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<(), ControlError> {
    let current = *WindowSettings::as_ref(ctx).zoom_level.value();
    let next = adjacent_zoom_level(current, increase);
    WindowSettings::handle(ctx)
        .update(ctx, |window_settings, ctx| {
            window_settings.zoom_level.set_value(next, ctx)
        })
        .map_err(|err| settings_write_error(ActionKind::AppearanceZoomReset, err))
}

fn reset_zoom(ctx: &mut ModelContext<LocalControlBridge>) -> Result<(), ControlError> {
    WindowSettings::handle(ctx)
        .update(ctx, |window_settings, ctx| {
            window_settings
                .zoom_level
                .set_value(ZoomLevel::default_value(), ctx)
        })
        .map_err(|err| settings_write_error(ActionKind::AppearanceZoomReset, err))
}

fn adjacent_zoom_level(current: u16, increase: bool) -> u16 {
    let default_index = ZoomLevel::VALUES
        .iter()
        .position(|zoom| *zoom == ZoomLevel::default_value())
        .unwrap_or(0);
    let current_index = ZoomLevel::VALUES
        .iter()
        .position(|zoom| *zoom == current)
        .unwrap_or(default_index);
    let next_index = if increase {
        (current_index + 1).min(ZoomLevel::VALUES.len() - 1)
    } else {
        current_index.saturating_sub(1)
    };
    ZoomLevel::VALUES[next_index]
}

fn set_allowlisted_setting(
    key: &str,
    value: serde_json::Value,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<(), ControlError> {
    if !ALLOWLISTED_SETTING_KEYS.contains(&key) {
        return Err(rejected_setting_key(key));
    }
    match key {
        "appearance.themes.theme" => set_theme(string_setting_value(key, &value)?, ctx),
        "appearance.themes.system_theme" => set_system_theme(bool_setting_value(key, &value)?, ctx),
        "appearance.themes.light_theme" => {
            set_system_theme_variant(string_setting_value(key, &value)?, true, ctx)
        }
        "appearance.themes.dark_theme" => {
            set_system_theme_variant(string_setting_value(key, &value)?, false, ctx)
        }
        "appearance.text.font_name" => {
            let font_name = string_setting_value(key, &value)?;
            if font_name.trim().is_empty() {
                return Err(ControlError::new(
                    ErrorCode::InvalidParams,
                    "appearance.text.font_name cannot be empty",
                ));
            }
            FontSettings::handle(ctx)
                .update(ctx, |font_settings, ctx| {
                    font_settings.monospace_font_name.set_value(font_name, ctx)
                })
                .map_err(|err| settings_write_error(ActionKind::SettingSet, err))
        }
        "appearance.text.font_size" => {
            let font_size = valid_font_size(u32_setting_value(key, &value)?)?;
            FontSettings::handle(ctx)
                .update(ctx, |font_settings, ctx| {
                    font_settings.monospace_font_size.set_value(font_size, ctx)
                })
                .map_err(|err| settings_write_error(ActionKind::SettingSet, err))
        }
        "appearance.window.zoom_level" => {
            let zoom_level = valid_zoom_level(u32_setting_value(key, &value)?)?;
            WindowSettings::handle(ctx)
                .update(ctx, |window_settings, ctx| {
                    window_settings.zoom_level.set_value(zoom_level, ctx)
                })
                .map_err(|err| settings_write_error(ActionKind::SettingSet, err))
        }
        "terminal.input.syntax_highlighting" => {
            let enabled = bool_setting_value(key, &value)?;
            InputSettings::handle(ctx)
                .update(ctx, |input_settings, ctx| {
                    input_settings.syntax_highlighting.set_value(enabled, ctx)
                })
                .map_err(|err| settings_write_error(ActionKind::SettingSet, err))
        }
        "terminal.input.error_underlining_enabled" => {
            let enabled = bool_setting_value(key, &value)?;
            InputSettings::handle(ctx)
                .update(ctx, |input_settings, ctx| {
                    input_settings.error_underlining.set_value(enabled, ctx)
                })
                .map_err(|err| settings_write_error(ActionKind::SettingSet, err))
        }
        "accessibility.accessibility_verbosity" => {
            let verbosity = accessibility_verbosity_value(key, &value)?;
            AccessibilitySettings::handle(ctx)
                .update(ctx, |accessibility_settings, ctx| {
                    accessibility_settings
                        .a11y_verbosity
                        .set_value(verbosity, ctx)
                })
                .map_err(|err| settings_write_error(ActionKind::SettingSet, err))
        }
        _ => Err(rejected_setting_key(key)),
    }
}

fn theme_kind_for_name(
    name: &str,
    ctx: &ModelContext<LocalControlBridge>,
) -> Result<ThemeKind, ControlError> {
    let matches = WarpConfig::as_ref(ctx)
        .theme_config()
        .theme_items()
        .filter_map(|(kind, _)| (public_theme_name(kind) == name).then_some(kind.clone()))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [theme] => Ok(theme.clone()),
        [] => Err(ControlError::new(
            ErrorCode::InvalidParams,
            format!("{name} is not an available theme"),
        )),
        _ => Err(ControlError::new(
            ErrorCode::InvalidParams,
            format!("{name} matches multiple themes"),
        )),
    }
}

fn valid_font_size(value: u32) -> Result<f32, ControlError> {
    if (5..=25).contains(&value) {
        return Ok(value as f32);
    }
    Err(ControlError::new(
        ErrorCode::InvalidParams,
        "font size must be between 5 and 25",
    ))
}

fn valid_zoom_level(value: u32) -> Result<u16, ControlError> {
    let value = u16::try_from(value).map_err(|err| {
        ControlError::with_details(
            ErrorCode::InvalidParams,
            "zoom level is outside the supported range",
            err.to_string(),
        )
    })?;
    if ZoomLevel::VALUES.contains(&value) {
        return Ok(value);
    }
    Err(ControlError::new(
        ErrorCode::InvalidParams,
        "zoom level must be one of the supported zoom percentages",
    ))
}

fn bool_setting_value(key: &str, value: &serde_json::Value) -> Result<bool, ControlError> {
    value.as_bool().ok_or_else(|| {
        ControlError::new(
            ErrorCode::InvalidParams,
            format!("{key} requires a boolean value"),
        )
    })
}

fn string_setting_value(key: &str, value: &serde_json::Value) -> Result<String, ControlError> {
    value.as_str().map(str::to_owned).ok_or_else(|| {
        ControlError::new(
            ErrorCode::InvalidParams,
            format!("{key} requires a string value"),
        )
    })
}

fn u32_setting_value(key: &str, value: &serde_json::Value) -> Result<u32, ControlError> {
    if let Some(value) = value.as_u64().and_then(|value| u32::try_from(value).ok()) {
        return Ok(value);
    }
    Err(ControlError::new(
        ErrorCode::InvalidParams,
        format!("{key} requires a non-negative integer value"),
    ))
}

fn accessibility_verbosity_value(
    key: &str,
    value: &serde_json::Value,
) -> Result<warpui::accessibility::AccessibilityVerbosity, ControlError> {
    match string_setting_value(key, value)?.as_str() {
        "Verbose" | "verbose" | "VERBOSE" => {
            Ok(warpui::accessibility::AccessibilityVerbosity::Verbose)
        }
        "Concise" | "concise" | "CONCISE" => {
            Ok(warpui::accessibility::AccessibilityVerbosity::Concise)
        }
        _ => Err(ControlError::new(
            ErrorCode::InvalidParams,
            "accessibility.accessibility_verbosity must be Verbose or Concise",
        )),
    }
}

fn settings_write_error(action: ActionKind, err: anyhow::Error) -> ControlError {
    ControlError::with_details(
        ErrorCode::Internal,
        format!("{} failed to update app settings", action.as_str()),
        err.to_string(),
    )
}

fn tab_mutation_result(
    instance_id: &Option<InstanceId>,
    action: ActionKind,
    tab_id: String,
    window_id: WindowId,
) -> serde_json::Value {
    json!({
        "action": action.as_str(),
        "ok": true,
        "instance_id": instance_id.as_ref().map(|id| id.0.as_str()),
        "window_id": window_id.to_string(),
        "tab_id": tab_id,
    })
}

fn pane_mutation_result(
    instance_id: &Option<InstanceId>,
    action: ActionKind,
    pane_id: PaneId,
    tab_id: String,
) -> serde_json::Value {
    json!({
        "action": action.as_str(),
        "ok": true,
        "instance_id": instance_id.as_ref().map(|id| id.0.as_str()),
        "tab_id": tab_id,
        "pane_id": pane_id.to_string(),
    })
}
