use super::*;
use flint_agent::{AgentKind, OptionChoice, SessionOption};
use flint_app::settings::Settings;

fn identity_options(model: &str, provider: &str) -> AgentEvent {
    let option = |id: &str, current: &str, values: &[&str]| SessionOption {
        id: id.into(),
        name: id.into(),
        category: Some(id.into()),
        description: None,
        current: current.into(),
        choices: values
            .iter()
            .map(|value| OptionChoice {
                value: (*value).into(),
                name: (*value).into(),
                description: None,
            })
            .collect(),
    };
    AgentEvent::SessionOptions(vec![
        option("model", model, &["model-a", "model-b"]),
        option("provider", provider, &["provider-a", "provider-b"]),
        option("mode", "ask", &["ask", "bypass"]),
    ])
}

fn droid(cx: &mut TestAppContext, options: Options) -> (Ui, Engine) {
    let ui = open_with(cx, options);
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            app.choose_agent(AgentKind::Droid, window, cx);
        })
    });
    let engine = ui.engine(cx);
    engine.send(cx, identity_options("model-a", "provider-a"));
    (ui, engine)
}

#[gpui_kit::test]
fn new_sessions_and_restart_retain_the_selected_agent_model_and_provider(cx: &mut TestAppContext) {
    let options = test_options();
    let home = options.home.clone().unwrap();
    let (ui, engine) = droid(cx, options.clone());
    ui.app.update(cx, |app, cx| {
        app.set_session_option("model", "model-b", cx);
        app.set_session_option("provider", "provider-b", cx);
    });
    assert!(ui.read(cx, |app, _| {
        app.settings.agent_options(AgentKind::Droid).is_empty()
    }));
    engine.send(cx, identity_options("model-b", "provider-b"));
    let saved = Settings::load(&home, &KeySources::none());
    assert_eq!(saved.default_agent, AgentKind::Droid);
    assert_eq!(saved.agent_options(AgentKind::Droid)["model"], "model-b");
    assert_eq!(
        saved.agent_options(AgentKind::Droid)["provider"],
        "provider-b"
    );
    ui.input(cx, "original");
    ui.press(cx, "enter");
    let old = ui.read(cx, |app, _| app.session().uid);
    ui.press(cx, "cmd-n");
    assert_eq!(ui.read(cx, |app, _| app.session().agent), AgentKind::Droid);
    assert_ne!(ui.read(cx, |app, _| app.session().uid), old);
    assert_eq!(
        ui.read(cx, |app, _| app.settings.agent_options(AgentKind::Droid)),
        saved.agent_options(AgentKind::Droid)
    );
    let restarted = open_with(cx, options);
    assert_eq!(
        restarted.read(cx, |app, _| app.session().agent),
        AgentKind::Droid
    );
    assert_eq!(
        restarted.read(cx, |app, _| app.settings.agent_options(AgentKind::Droid)),
        saved.agent_options(AgentKind::Droid)
    );
}

#[gpui_kit::test]
fn native_model_and_provider_settings_survive_new_sessions_and_restart(cx: &mut TestAppContext) {
    let options = test_options();
    let ui = open_with(cx, options.clone());
    let _engine = ui.engine(cx);
    ui.input(cx, "original native session");
    ui.press(cx, "enter");
    ui.press(cx, "cmd-,");
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            let form = app.settings_form.as_ref().unwrap();
            form.model.update(cx, |input, cx| {
                input.set_value("selected-native-model", window, cx)
            });
            form.base_url.update(cx, |input, cx| {
                input.set_value("https://selected-provider.example/v1", window, cx)
            });
        })
    });
    ui.click(cx, "settings-save");
    ui.press(cx, "cmd-n");
    let config = ui.read(cx, |app, _| {
        flint_app::engine::config_for(
            &app.session().workspace,
            &app.settings,
            app.key_path.as_deref(),
            &app.key_sources,
            app.approval,
        )
        .unwrap()
    });
    assert_eq!(config.model, "selected-native-model");
    assert_eq!(config.base_url, "https://selected-provider.example/v1");
    let restarted = open_with(cx, options);
    assert_eq!(
        restarted.read(cx, |app, _| app.settings.model.clone()),
        config.model
    );
    assert_eq!(
        restarted.read(cx, |app, _| app.settings.base_url.clone()),
        config.base_url
    );
}

#[gpui_kit::test]
fn older_conversations_seed_missing_identity_defaults_without_resetting_the_model(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = droid(cx, test_options());
    engine.send(cx, identity_options("model-b", "provider-b"));
    assert!(ui.read(cx, |app, _| {
        app.settings.agent_options(AgentKind::Droid).is_empty()
    }));
    ui.press(cx, "cmd-n");
    assert_eq!(
        ui.read(cx, |app, _| {
            app.settings.agent_options(AgentKind::Droid)["model"].clone()
        }),
        "model-b"
    );
    assert_eq!(
        ui.read(cx, |app, _| {
            app.settings.agent_options(AgentKind::Droid)["provider"].clone()
        }),
        "provider-b"
    );
}

#[gpui_kit::test]
fn switching_projects_retains_an_older_sessions_identity(cx: &mut TestAppContext) {
    let (ui, engine) = droid(cx, test_options());
    engine.send(cx, identity_options("model-b", "provider-b"));
    ui.app.update(cx, |app, cx| {
        app.set_project_folder(temp_dir("identity-project"), cx)
    });
    assert_eq!(ui.read(cx, |app, _| app.session().agent), AgentKind::Droid);
    assert_eq!(
        ui.read(cx, |app, _| {
            app.settings.agent_options(AgentKind::Droid)["model"].clone()
        }),
        "model-b"
    );
    assert_eq!(
        ui.read(cx, |app, _| {
            app.settings.agent_options(AgentKind::Droid)["provider"].clone()
        }),
        "provider-b"
    );
}

#[gpui_kit::test]
fn new_session_does_not_reuse_an_empty_session_from_another_agent(cx: &mut TestAppContext) {
    let ui = open(cx);
    let native_uid = ui.read(cx, |app, _| app.session().uid);
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            app.new_agent_session(AgentKind::Droid, window, cx);
        })
    });
    assert_eq!(ui.read(cx, |app, _| app.session().agent), AgentKind::Droid);
    assert_ne!(ui.read(cx, |app, _| app.session().uid), native_uid);
    assert_eq!(
        ui.read(cx, |app, _| app.sessions[0].agent),
        AgentKind::Flint
    );
}

#[gpui_kit::test]
fn clear_retains_the_current_agent(cx: &mut TestAppContext) {
    let (ui, _engine) = droid(cx, test_options());
    ui.input(cx, "original");
    ui.press(cx, "enter");
    ui.input(cx, "/clear");
    ui.press(cx, "enter");
    assert_eq!(ui.read(cx, |app, _| app.session().agent), AgentKind::Droid);
}

#[gpui_kit::test]
fn provider_changes_retain_the_confirmed_model_pair(cx: &mut TestAppContext) {
    let (ui, engine) = droid(cx, test_options());
    ui.app
        .update(cx, |app, cx| app.set_session_option("model", "model-a", cx));
    ui.app.update(cx, |app, cx| {
        app.set_session_option("provider", "provider-b", cx)
    });
    engine.send(cx, identity_options("model-b", "provider-b"));
    assert_eq!(
        ui.read(cx, |app, _| {
            app.settings.agent_options(AgentKind::Droid)["model"].clone()
        }),
        "model-b"
    );
}

#[gpui_kit::test]
fn rejected_choice_and_permission_modes_do_not_become_model_defaults(cx: &mut TestAppContext) {
    let (ui, engine) = droid(cx, test_options());
    ui.app
        .update(cx, |app, cx| app.set_session_option("model", "model-b", cx));
    engine.send(
        cx,
        AgentEvent::Error("Droid didn't change model: rejected".into()),
    );
    engine.send(cx, identity_options("model-b", "provider-a"));
    assert!(ui.read(cx, |app, _| {
        app.settings.agent_options(AgentKind::Droid).is_empty()
    }));
    ui.app
        .update(cx, |app, cx| app.set_session_option("mode", "bypass", cx));
    let mut confirmed = match identity_options("model-b", "provider-a") {
        AgentEvent::SessionOptions(list) => list,
        _ => unreachable!(),
    };
    confirmed[2].current = "bypass".into();
    engine.send(cx, AgentEvent::SessionOptions(confirmed));
    assert!(ui.read(cx, |app, _| {
        app.settings.agent_options(AgentKind::Droid).is_empty()
    }));
}

#[gpui_kit::test]
fn late_background_confirmation_cannot_override_a_newer_selection(cx: &mut TestAppContext) {
    let (ui, old_engine) = droid(cx, test_options());
    ui.app
        .update(cx, |app, cx| app.set_session_option("model", "model-b", cx));
    ui.press(cx, "cmd-n");
    let new_engine = ui.engine(cx);
    new_engine.send(cx, identity_options("model-b", "provider-a"));
    ui.app
        .update(cx, |app, cx| app.set_session_option("model", "model-a", cx));
    new_engine.send(cx, identity_options("model-a", "provider-a"));
    old_engine.send(cx, identity_options("model-b", "provider-a"));
    assert_eq!(
        ui.read(cx, |app, _| {
            app.settings.agent_options(AgentKind::Droid)["model"].clone()
        }),
        "model-a"
    );
}
