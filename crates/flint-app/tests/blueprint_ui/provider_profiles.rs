use super::*;
use flint_app::settings::{ProviderKind, Settings};

fn enter_provider(
    ui: &Ui,
    cx: &mut TestAppContext,
    name: &str,
    base: &str,
    model: &str,
    env: &str,
) {
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            let form = app.settings_form.as_ref().unwrap();
            for (input, value) in [
                (&form.provider_name, name),
                (&form.base_url, base),
                (&form.model, model),
                (&form.api_key_env, env),
            ] {
                input.update(cx, |input, cx| input.set_value(value, window, cx));
            }
        });
    });
}

#[gpui_kit::test]
fn custom_provider_profiles_can_be_added_switched_saved_and_reopened(cx: &mut TestAppContext) {
    let ui = open(cx);
    let before = ui.read(cx, |app, _| app.settings.clone());
    ui.press(cx, "secondary-,");
    ui.click_settings(cx, "preset-custom");
    enter_provider(
        &ui,
        cx,
        "First",
        "https://first.example/v1",
        "first-model",
        "FIRST_KEY",
    );
    ui.click_settings(cx, "settings-store-provider");
    ui.click_settings(cx, "preset-custom");
    enter_provider(
        &ui,
        cx,
        "Second",
        "https://second.example/api/v1",
        "second-model",
        "SECOND_KEY",
    );
    ui.click_settings(cx, "settings-store-provider");
    assert_eq!(ui.read(cx, |app, _| app.settings.clone()), before);
    ui.click_settings(cx, ("settings-use-provider", 0usize));
    ui.read(cx, |app, cx| {
        let draft = app.settings_form.as_ref().unwrap().draft(&app.settings, cx);
        assert_eq!(draft.provider, ProviderKind::Custom);
        assert_eq!(draft.base_url, "https://first.example/v1");
        assert_eq!(draft.model, "first-model");
        assert_eq!(draft.api_key_env, "FIRST_KEY");
        assert_eq!(draft.provider_profiles.len(), 2);
        assert_eq!(draft.approval, before.approval);
        assert_eq!(draft.sandbox, before.sandbox);
    });
    ui.click(cx, "settings-save");
    ui.read(cx, |app, _| {
        assert!(app.settings_form.is_none());
        assert_eq!(Settings::load(&app.home, &KeySources::none()), app.settings);
        assert_eq!(app.settings.provider_profiles.len(), 2);
    });
    ui.press(cx, "secondary-,");
    ui.click_settings(cx, ("settings-use-provider", 1usize));
    ui.click(cx, "settings-save");
    ui.read(cx, |app, _| {
        assert_eq!(app.settings.model, "second-model");
        assert_eq!(app.settings.base_url, "https://second.example/api/v1");
        assert_eq!(app.settings.api_key_env, "SECOND_KEY");
        assert_eq!(app.settings.provider_profiles.len(), 2);
    });
}

#[gpui_kit::test]
fn custom_provider_cancel_discards_profiles_and_add_custom_clears_previous_auth(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let before = ui.read(cx, |app, _| app.settings.clone());
    ui.press(cx, "secondary-,");
    ui.click(cx, "preset-omniroute");
    ui.click(cx, "settings-omniroute-no-auth");
    ui.click(cx, "preset-custom");
    ui.read(cx, |app, cx| {
        let draft = app.settings_form.as_ref().unwrap().draft(&app.settings, cx);
        assert_eq!(draft.provider, ProviderKind::Custom);
        assert!(!draft.omniroute_no_auth);
        assert!(draft.api_key_env.is_empty());
        assert!(draft.api_key_file.is_empty());
        assert!(draft.model.is_empty());
        assert!(draft.base_url.is_empty());
    });
    enter_provider(
        &ui,
        cx,
        "Temporary",
        "https://custom.example/v1",
        "custom-model",
        "CUSTOM_KEY",
    );
    ui.click_settings(cx, "settings-store-provider");
    ui.click(cx, "settings-cancel");
    assert_eq!(ui.read(cx, |app, _| app.settings.clone()), before);
    ui.press(cx, "secondary-,");
    assert!(ui.read(cx, |app, _| {
        app.settings_form
            .as_ref()
            .unwrap()
            .provider_profiles
            .is_empty()
    }));
}

#[gpui_kit::test]
fn custom_provider_save_updates_profile_and_remove_keeps_active_config(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.press(cx, "secondary-,");
    ui.click(cx, "preset-custom");
    enter_provider(
        &ui,
        cx,
        "Custom",
        "https://custom.example/v1",
        "first-model",
        "CUSTOM_KEY",
    );
    ui.click(cx, "settings-save");
    ui.press(cx, "secondary-,");
    enter_provider(
        &ui,
        cx,
        "Custom",
        "https://custom.example/v1",
        "updated-model",
        "CUSTOM_KEY",
    );
    ui.click(cx, "settings-save");
    ui.read(cx, |app, _| {
        assert_eq!(
            app.settings.provider_profiles["Custom"].model,
            "updated-model"
        );
    });
    ui.press(cx, "secondary-,");
    ui.click_settings(cx, ("settings-remove-provider", 0usize));
    ui.click(cx, "settings-save");
    ui.read(cx, |app, _| {
        assert!(app.settings.provider_profiles.is_empty());
        assert_eq!(app.settings.model, "updated-model");
        assert_eq!(Settings::load(&app.home, &KeySources::none()), app.settings);
    });
}

#[gpui_kit::test]
fn invalid_custom_provider_cannot_be_kept_or_saved(cx: &mut TestAppContext) {
    let ui = open(cx);
    let before = ui.read(cx, |app, _| app.settings.clone());
    ui.press(cx, "secondary-,");
    ui.click(cx, "preset-custom");
    enter_provider(
        &ui,
        cx,
        "Invalid",
        "https://user:password@custom.example/v1",
        "model",
        "CUSTOM_KEY",
    );
    ui.click_settings(cx, "settings-store-provider");
    ui.read(cx, |app, _| {
        let form = app.settings_form.as_ref().unwrap();
        assert!(form.error.is_some());
        assert!(form.provider_profiles.is_empty());
    });
    ui.click(cx, "settings-save");
    assert_eq!(ui.read(cx, |app, _| app.settings.clone()), before);
    assert!(ui.read(cx, |app, _| app.settings_form.is_some()));
}

#[gpui_kit::test]
fn custom_provider_connection_test_uses_unsaved_endpoint_without_inference(
    cx: &mut TestAppContext,
) {
    let provider = local_provider::LocalProvider::listing(std::time::Duration::ZERO);
    let ui = open(cx);
    let before = ui.read(cx, |app, _| app.settings.clone());
    ui.press(cx, "secondary-,");
    ui.click(cx, "preset-custom");
    enter_provider(
        &ui,
        cx,
        "Local",
        &provider.url,
        "fixture-model",
        "CUSTOM_KEY",
    );
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
        assert_eq!(app.settings, before);
    });
    assert_eq!(
        provider
            .model_requests
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    assert_eq!(
        provider
            .completion_requests
            .load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    ui.click(cx, "preset-openai");
    ui.read(cx, |app, _| {
        let form = app.settings_form.as_ref().unwrap();
        assert!(form.probe_result.is_none());
        assert!(form.probe.is_none());
    });
}
