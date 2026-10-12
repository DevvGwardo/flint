use super::*;
use flint_app::settings::Settings;

#[gpui_kit::test]
fn jev_recovered_guard_guidance_collapses_and_keeps_evidence_inspectable(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(
        cx,
        AgentEvent::HarnessNudge {
            reason: flint_agent::NudgeReason::Verify,
            message: "Run a current verification command before ending.".into(),
        },
    );
    engine.send(
        cx,
        AgentEvent::JevVerdict {
            question: flint_agent::JevQuestion::Verified,
            probability: 0.08,
            confirmed: false,
        },
    );
    engine.send(
        cx,
        AgentEvent::TextDelta("Checks passed; review the result.".into()),
    );
    engine.send(
        cx,
        AgentEvent::TaskAssessed {
            turn_id: 1,
            task: Box::new(super::task_outcomes_ui::task_fixture(
                flint_agent::TaskOutcome::NeedsReview,
            )),
        },
    );
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    let (header, nudge, jev) = ui.read(cx, |app, _| {
        let view = &app.session().view;
        (
            view.turns[0].header.unwrap(),
            view.items
                .iter()
                .position(|item| matches!(item, Item::Nudge { .. }))
                .unwrap(),
            view.items
                .iter()
                .position(|item| matches!(item, Item::Jev { .. }))
                .unwrap(),
        )
    });
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("nudge", nudge)).is_none());
        assert!(window.try_find(("jev", jev)).is_none());
    });
    ui.click(cx, ("worked", header));
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find(("nudge", nudge)).label(),
            Some("Guard guidance · verify before done")
        );
        assert_eq!(
            window.find(("jev", jev)).label(),
            Some("JEV · verification · 8% · verified")
        );
        assert!(window.try_find(("nudge-evidence", nudge)).is_none());
    });
    ui.click(cx, ("nudge", nudge));
    assert!(ui.with(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(("nudge-evidence", nudge)).is_some()
    }));
    assert!(engine.sent().is_empty());
}

fn judge_configured(ui: &Ui, cx: &mut TestAppContext) -> bool {
    ui.read(cx, |app, _| {
        flint_app::engine::config_for(
            &app.session().workspace,
            &app.settings,
            app.key_path.as_deref(),
            &app.key_sources,
            app.approval,
        )
        .unwrap()
        .jev
        .is_some()
    })
}

#[gpui_kit::test]
fn jev_composer_toggle_persists_both_choices_and_preserves_the_draft(cx: &mut TestAppContext) {
    let mut options = test_options();
    options.key_sources = Some(KeySources {
        env: |name| (name == "TYPESAFE_API_KEY").then(|| "fixture-judge-key".into()),
        ..KeySources::none()
    });
    let home = options.home.clone().unwrap();
    let ui = open_with(cx, options.clone());
    assert!(judge_configured(&ui, cx));
    ui.input(cx, "Keep this unsent draft.");
    ui.click(cx, "jev-toggle");
    assert!(!judge_configured(&ui, cx));
    assert_eq!(ui.composer_text(cx), "Keep this unsent draft.");
    assert!(ui.read(cx, |app, _| {
        app.session().ops.is_none() && app.session().view.items.is_empty()
    }));
    assert!(!Settings::load(&home, &KeySources::none()).jev_judge);
    let restarted = open_with(cx, options);
    assert!(!restarted.read(cx, |app, _| app.settings.jev_judge));
    restarted.click(cx, "jev-toggle");
    assert!(judge_configured(&restarted, cx));
    assert!(Settings::load(&home, &KeySources::none()).jev_judge);
}

#[gpui_kit::test]
fn jev_composer_toggle_keeps_the_saved_value_on_failure_and_can_retry(cx: &mut TestAppContext) {
    let options = test_options();
    let home = options.home.clone().unwrap();
    Settings::default().save(&home).unwrap();
    let before = std::fs::read(Settings::path(&home)).unwrap();
    let ui = open_with(cx, options);
    let blocked = home.join("config.toml.tmp");
    std::fs::create_dir(&blocked).unwrap();
    ui.click(cx, "jev-toggle");
    assert!(ui.read(cx, |app, _| {
        app.settings.jev_judge
            && app
                .store_error
                .as_deref()
                .is_some_and(|error| error.starts_with("Couldn't save JEV setting:"))
    }));
    assert_eq!(std::fs::read(Settings::path(&home)).unwrap(), before);
    std::fs::remove_dir(blocked).unwrap();
    ui.click(cx, "jev-toggle");
    assert!(ui.read(cx, |app, _| !app.settings.jev_judge
        && app.store_error.is_none()));
    assert!(!Settings::load(&home, &KeySources::none()).jev_judge);
    ui.app.update(cx, |app, _| {
        app.store_error = Some("Unrelated storage problem".into());
    });
    ui.click(cx, "jev-toggle");
    assert_eq!(
        ui.read(cx, |app, _| app.store_error.clone()),
        Some("Unrelated storage problem".into())
    );
}

#[gpui_kit::test]
fn jev_composer_toggle_stages_next_turn_configuration_without_interrupting(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    let turn_id = ui.read(cx, |app, _| app.session().view.turn_id);
    ui.input(cx, "Unsent follow-up");
    ui.click(cx, "jev-toggle");
    assert!(matches!(engine.sent().as_slice(), [Op::SetJev(None)]));
    assert!(ui.read(cx, |app, _| {
        !app.settings.jev_judge
            && app.session().view.running
            && app.session().view.turn_id == turn_id
            && app.session().ops.is_some()
    }));
    assert_eq!(ui.composer_text(cx), "Unsent follow-up");
}

#[gpui_kit::test]
fn jev_settings_updates_existing_native_engines_without_changing_approvals(
    cx: &mut TestAppContext,
) {
    let mut options = test_options();
    options.key_sources = Some(KeySources {
        env: |name| (name == "TYPESAFE_API_KEY").then(|| "fixture-judge-key".into()),
        ..KeySources::none()
    });
    let ui = open_with(cx, options);
    let engine = ui.engine(cx);
    let approval = ui.read(cx, |app, _| app.approval);
    ui.press(cx, "secondary-,");
    ui.scroll_settings(cx);
    ui.click(cx, ("settings-jev", 1usize));
    ui.click(cx, "settings-save");
    assert!(matches!(engine.sent().as_slice(), [Op::SetJev(None)]));
    ui.click(cx, "jev-toggle");
    assert!(
        matches!(engine.sent().as_slice(), [Op::SetJev(Some(config))] if config.model == "jev-latest")
    );
    assert_eq!(ui.read(cx, |app, _| app.approval), approval);
}

#[gpui_kit::test]
fn jev_settings_and_composer_share_one_saved_preference(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.click(cx, "jev-toggle");
    ui.press(cx, "secondary-,");
    assert!(!ui.read(cx, |app, _| app.settings_form.as_ref().unwrap().jev_judge));
    ui.scroll_settings(cx);
    ui.click(cx, ("settings-jev", 0usize));
    ui.click(cx, "settings-save");
    assert!(ui.read(cx, |app, _| app.settings.jev_judge));
    ui.click(cx, "jev-toggle");
    assert!(!ui.read(cx, |app, _| app.settings.jev_judge));
}

#[gpui_kit::test]
fn jev_settings_cancel_and_escape_do_not_change_the_saved_preference(cx: &mut TestAppContext) {
    let options = test_options();
    let home = options.home.clone().unwrap();
    let ui = open_with(cx, options);
    for close in ["settings-cancel", "escape"] {
        ui.press(cx, "secondary-,");
        ui.scroll_settings(cx);
        ui.click(cx, ("settings-jev", 1usize));
        // A direct action cannot override an open Settings draft.
        ui.app.update(cx, |app, cx| app.toggle_jev_judge(cx));
        assert!(ui.read(cx, |app, _| {
            app.settings.jev_judge && !app.settings_form.as_ref().unwrap().jev_judge
        }));
        if close == "escape" {
            ui.press(cx, close);
        } else {
            ui.click(cx, close);
        }
        assert!(ui.read(cx, |app, _| app.settings.jev_judge
            && app.settings_form.is_none()));
        assert!(!Settings::path(&home).exists());
    }
}

#[gpui_kit::test]
fn jev_settings_save_failure_retains_the_draft_and_saved_preference(cx: &mut TestAppContext) {
    let options = test_options();
    let home = options.home.clone().unwrap();
    let ui = open_with(cx, options);
    ui.press(cx, "secondary-,");
    ui.scroll_settings(cx);
    ui.click(cx, ("settings-jev", 1usize));
    std::fs::create_dir(home.join("config.toml.tmp")).unwrap();
    ui.click(cx, "settings-save");
    assert!(ui.read(cx, |app, _| {
        let form = app.settings_form.as_ref().unwrap();
        app.settings.jev_judge
            && !form.jev_judge
            && form
                .error
                .as_deref()
                .is_some_and(|error| error.starts_with("Couldn't save settings:"))
    }));
    assert!(!Settings::path(&home).exists());
}

#[gpui_kit::test]
fn jev_composer_control_is_native_only_and_never_governs_acp_agents(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.app.update(cx, |app, cx| {
        let ix = app.active;
        app.sessions[ix].agent = flint_agent::AgentKind::Droid;
        cx.notify();
    });
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("jev-toggle").is_none());
        ui.app.update(cx, |app, cx| app.toggle_jev_judge(cx));
    });
    assert!(ui.read(cx, |app, _| app.settings.jev_judge));
    assert!(engine.sent().is_empty());
}
