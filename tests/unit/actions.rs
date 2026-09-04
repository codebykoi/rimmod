use super::*;

#[test]
fn failed_reorder_is_reported_as_a_persistent_error() {
    let mut app = App::default();

    app.apply_mod_list_action(ModListAction::Reorder {
        mod_ids: vec![ModId::from_index(0)],
        kind: ModListKind::Enabled,
        before_mod_id: None,
    });

    assert!(
        app.action_error
            .as_deref()
            .is_some_and(|error| error.contains("Could not reorder mod"))
    );
}

#[test]
fn modifier_selection_builds_and_replaces_a_group() {
    let mut app = App::default();
    let first = ModId::from_index(1);
    let second = ModId::from_index(2);
    let third = ModId::from_index(3);

    app.apply_mod_list_action(ModListAction::Select {
        mod_id: first,
        kind: ModListKind::Disabled,
        mode: SelectionMode::Replace,
    });
    app.apply_mod_list_action(ModListAction::Select {
        mod_id: third,
        kind: ModListKind::Disabled,
        mode: SelectionMode::Toggle,
    });

    assert_eq!(app.state.selected_mod_ids, [first, third]);
    assert_eq!(app.state.selected_mod_id, Some(third));

    app.apply_mod_list_action(ModListAction::Select {
        mod_id: second,
        kind: ModListKind::Disabled,
        mode: SelectionMode::Range(vec![first, second]),
    });

    assert_eq!(app.state.selected_mod_ids, [first, second]);
    assert_eq!(app.state.selected_mod_id, Some(second));
}

#[test]
fn selecting_a_mod_in_the_other_list_starts_a_new_group() {
    let mut app = App::default();
    let disabled_mod = ModId::from_index(1);
    let enabled_mod = ModId::from_index(2);

    app.apply_mod_list_action(ModListAction::Select {
        mod_id: disabled_mod,
        kind: ModListKind::Disabled,
        mode: SelectionMode::Replace,
    });
    app.apply_mod_list_action(ModListAction::Select {
        mod_id: enabled_mod,
        kind: ModListKind::Enabled,
        mode: SelectionMode::Toggle,
    });

    assert_eq!(app.state.selected_mod_ids, [enabled_mod]);
    assert_eq!(app.state.selection_kind, Some(ModListKind::Enabled));
}

#[test]
fn load_order_changes_can_be_undone_and_redone() {
    let first = ModId::from_index(0);
    let second = ModId::from_index(1);
    let active = ModId::from_index(2);
    let mut app = App {
        mods: ModCollection::new(Vec::new(), vec![first, second], vec![active], Vec::new()),
        ..App::default()
    };

    app.apply_mod_list_action(ModListAction::Select {
        mod_id: first,
        kind: ModListKind::Disabled,
        mode: SelectionMode::Range(vec![first, second]),
    });
    app.apply_mod_list_action(ModListAction::Transfer {
        mod_ids: vec![first, second],
        from: ModListKind::Disabled,
        to: ModListKind::Enabled,
        before_mod_id: None,
    });

    assert_eq!(app.mods.disabled_ids(), []);
    assert_eq!(app.mods.enabled_ids(), [active, first, second]);
    assert_eq!(app.undo_history.len(), 1);
    assert!(app.redo_history.is_empty());

    app.undo_load_order_change();

    assert_eq!(app.mods.disabled_ids(), [first, second]);
    assert_eq!(app.mods.enabled_ids(), [active]);
    assert_eq!(app.state.selection_kind, Some(ModListKind::Disabled));
    assert_eq!(app.state.selected_mod_ids, [first, second]);
    assert!(app.undo_history.is_empty());
    assert_eq!(app.redo_history.len(), 1);

    app.redo_load_order_change();

    assert_eq!(app.mods.disabled_ids(), []);
    assert_eq!(app.mods.enabled_ids(), [active, first, second]);
    assert_eq!(app.state.selection_kind, Some(ModListKind::Enabled));
    assert_eq!(app.state.selected_mod_ids, [first, second]);
    assert_eq!(app.undo_history.len(), 1);
    assert!(app.redo_history.is_empty());
}

#[test]
fn a_new_change_after_undo_clears_redo_history() {
    let first = ModId::from_index(0);
    let second = ModId::from_index(1);
    let mut app = App {
        mods: ModCollection::new(Vec::new(), vec![first, second], Vec::new(), Vec::new()),
        ..App::default()
    };

    app.apply_mod_list_action(ModListAction::Reorder {
        mod_ids: vec![second],
        kind: ModListKind::Disabled,
        before_mod_id: Some(first),
    });
    app.undo_load_order_change();

    assert_eq!(app.redo_history.len(), 1);

    app.apply_mod_list_action(ModListAction::Reorder {
        mod_ids: vec![first],
        kind: ModListKind::Disabled,
        before_mod_id: None,
    });

    assert_eq!(app.mods.disabled_ids(), [second, first]);
    assert!(app.redo_history.is_empty());
}

#[test]
fn keyboard_shortcuts_undo_and_redo_a_load_order_change() {
    let disabled_mod = ModId::from_index(0);
    let mut app = App {
        mods: ModCollection::new(Vec::new(), vec![disabled_mod], Vec::new(), Vec::new()),
        ..App::default()
    };

    app.apply_mod_list_action(ModListAction::Transfer {
        mod_ids: vec![disabled_mod],
        from: ModListKind::Disabled,
        to: ModListKind::Enabled,
        before_mod_id: None,
    });

    let context = egui::Context::default();
    let undo_input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Z,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }],
        ..egui::RawInput::default()
    };

    context
        .run_ui(undo_input, |ui| {
            app.handle_history_shortcuts(ui.ctx());
        })
        .drop_without_applying_deltas();

    assert_eq!(app.mods.disabled_ids(), [disabled_mod]);
    assert!(app.mods.enabled_ids().is_empty());

    let redo_input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Y,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }],
        ..egui::RawInput::default()
    };

    context
        .run_ui(redo_input, |ui| {
            app.handle_history_shortcuts(ui.ctx());
        })
        .drop_without_applying_deltas();

    assert!(app.mods.disabled_ids().is_empty());
    assert_eq!(app.mods.enabled_ids(), [disabled_mod]);
}

#[test]
fn failed_open_is_reported_as_a_persistent_error() {
    let mut app = App::default();

    app.handle_open_result(
        "folder Z:\\missing".to_owned(),
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "simulated open failure",
        )),
    );

    assert!(
        app.action_error
            .as_deref()
            .is_some_and(|error| error.contains("simulated open failure"))
    );
}

#[test]
fn missing_game_path_is_reported_as_a_persistent_launch_error() {
    let mut app = App::default();

    app.run_game();

    assert!(
        app.action_error
            .as_deref()
            .is_some_and(|error| error.contains("Could not start RimWorld"))
    );
}
