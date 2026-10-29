use super::*;
use flint_app::settings::{OMNIROUTE_BASE_URL, OMNIROUTE_KEY_ENV, ProviderKind, Settings};

#[gpui_kit::test]
fn omniroute_preset_stages_route_and_scoped_auth_without_changing_saved_settings(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let before = ui.read(cx, |app, _| app.settings.clone());
    ui.press(cx, "secondary-,");
    ui.click(cx, "preset-omniroute");
    ui.read(cx, |app, cx| {
        assert_eq!(app.settings, before);
        let draft = app.settings_form.as_ref().unwrap().draft(&app.settings, cx);
        assert_eq!(draft.provider, ProviderKind::OmniRoute);
        assert_eq!(draft.base_url, OMNIROUTE_BASE_URL);
        assert_eq!(draft.model, "auto");
        assert!(draft.subagent_model.is_empty());
        assert_eq!(draft.api_key_env, OMNIROUTE_KEY_ENV);
        assert!(draft.api_key_file.is_empty());
        assert!(!draft.omniroute_no_auth);
        assert_eq!(draft.approval, before.approval);
        assert_eq!(draft.sandbox, before.sandbox);
    });
    ui.click(cx, "settings-cancel");
    assert_eq!(ui.read(cx, |app, _| app.settings.clone()), before);
}

#[gpui_kit::test]
fn omniroute_local_no_auth_saves_and_starts_without_ambient_provider_keys(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.press(cx, "secondary-,");
    ui.click(cx, "preset-omniroute");
    ui.click(cx, "settings-omniroute-no-auth");
    ui.click(cx, "settings-save");
    ui.read(cx, |app, _| {
        let saved = Settings::load(&app.home, &KeySources::none());
        assert_eq!(saved.provider, ProviderKind::OmniRoute);
        assert!(saved.omniroute_no_auth);
        let config = flint_app::engine::config_for(
            &app.session().workspace,
            &saved,
            None,
            &KeySources::none(),
            app.approval,
        )
        .unwrap();
        assert_eq!(config.model, "auto");
        assert!(config.api_key.is_empty());
        assert!(app.session().ops.is_none());
    });
}

#[gpui_kit::test]
fn omniroute_remote_no_auth_cannot_be_saved_or_probed(cx: &mut TestAppContext) {
    let ui = open(cx);
    let before = ui.read(cx, |app, _| app.settings.clone());
    ui.press(cx, "secondary-,");
    ui.click(cx, "preset-omniroute");
    ui.click(cx, "settings-omniroute-no-auth");
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            app.settings_form
                .as_ref()
                .unwrap()
                .base_url
                .update(cx, |input, cx| {
                    input.set_value("https://router.example/v1", window, cx);
                });
        });
    });
    ui.click(cx, "test-provider-connection");
    ui.read(cx, |app, _| {
        let form = app.settings_form.as_ref().unwrap();
        assert!(form.probe.is_none());
        assert!(form.error.as_ref().unwrap().contains("loopback"));
    });
    ui.click(cx, "settings-save");
    assert_eq!(ui.read(cx, |app, _| app.settings.clone()), before);
    assert!(ui.read(cx, |app, _| app.settings_form.is_some()));
}

#[gpui_kit::test]
fn omniroute_switching_presets_clears_router_auth_and_no_auth_mode(cx: &mut TestAppContext) {
    for preset in ["preset-openai", "preset-ollama"] {
        let ui = open(cx);
        ui.press(cx, "secondary-,");
        ui.click(cx, "preset-omniroute");
        ui.click(cx, "settings-omniroute-no-auth");
        ui.click(cx, preset);
        ui.read(cx, |app, cx| {
            let draft = app.settings_form.as_ref().unwrap().draft(&app.settings, cx);
            assert_eq!(draft.provider, ProviderKind::OpenAiCompatible);
            assert!(!draft.omniroute_no_auth);
            assert!(draft.api_key_env.is_empty());
            assert!(draft.api_key_file.is_empty());
        });
        assert!(!has(&ui, cx, "settings-omniroute-no-auth"));
    }
}

#[gpui_kit::test]
fn omniroute_connection_probe_uses_unsaved_local_route_and_never_runs_inference(
    cx: &mut TestAppContext,
) {
    let provider = local_provider::LocalProvider::listing(std::time::Duration::ZERO);
    let ui = open(cx);
    let before = ui.read(cx, |app, _| app.settings.clone());
    ui.press(cx, "secondary-,");
    ui.click(cx, "preset-omniroute");
    ui.click(cx, "settings-omniroute-no-auth");
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            let form = app.settings_form.as_ref().unwrap();
            form.base_url
                .update(cx, |input, cx| input.set_value(&provider.url, window, cx));
            form.model
                .update(cx, |input, cx| input.set_value("fixture-model", window, cx));
        });
    });
    ui.click(cx, "test-provider-connection");
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        settle(cx);
        if ui.read(cx, |app, _| {
            app.settings_form.as_ref().unwrap().probe_result.is_some()
        }) {
            break;
        }
    }
    ui.read(cx, |app, _| {
        let result = app
            .settings_form
            .as_ref()
            .unwrap()
            .probe_result
            .as_ref()
            .unwrap();
        assert!(result.reachable);
        assert_eq!(result.model_listed, Some(true));
        assert_eq!(result.credentials_accepted, None);
        assert_eq!(app.settings, before);
    });
    assert_eq!(
        provider
            .completion_requests
            .load(std::sync::atomic::Ordering::Relaxed),
        0
    );
}
