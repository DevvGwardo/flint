use pretty_assertions::assert_eq;

use super::KeySources;
use super::KeyStatus;
use super::Settings;
use flint_agent::ApprovalMode;

#[test]
fn defaults_point_at_a_public_endpoint() {
    let settings = Settings::default();
    assert_eq!(settings.base_url, "https://api.openai.com/v1");
    assert_eq!(settings.api_key_env, "");
    assert_eq!(settings.api_key_file, "");
    assert_eq!(settings.subagent_model, "");
    assert_eq!(settings.approval_mode(), ApprovalMode::AskForChanges);
}

#[test]
fn saved_auto_run_is_preserved() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(Settings::path(dir.path()), "approval = \"auto\"\n").expect("write");
    assert_eq!(
        Settings::load(dir.path(), &KeySources::none()).approval_mode(),
        ApprovalMode::Auto
    );
}

#[test]
fn missing_malformed_and_unknown_permissions_fail_safe() {
    let dir = tempfile::tempdir().unwrap();
    for config in [
        "",
        "approval = \"bogus\"",
        "approval = 42",
        "not valid toml [",
        "approval = \"AUTO\"",
        "approval = \"\"",
    ] {
        std::fs::write(Settings::path(dir.path()), config).unwrap();
        let settings = Settings::load(dir.path(), &KeySources::none());
        assert_eq!(settings.approval_mode(), ApprovalMode::AskForChanges);
        assert_ne!(settings.permission_choice_pending, Some(true));
    }
}

#[test]
fn legacy_permissions_are_preserved_without_first_run_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("legacy.key");
    std::fs::write(&key, "dummy").unwrap();
    let settings = Settings::load(
        dir.path(),
        &KeySources {
            legacy_key_file: Some(key),
            ..KeySources::none()
        },
    );
    assert_eq!(settings.approval_mode(), ApprovalMode::Auto);
    assert_eq!(settings.permission_choice_pending, Some(false));
}

#[test]
fn subagent_model_round_trips_and_is_wired_into_engine_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let settings = Settings {
        subagent_model: "  child-model  ".into(),
        ..Settings::default()
    };
    settings.save(dir.path()).expect("save");
    let sources = KeySources {
        env: |name| (name == "OPENAI_API_KEY").then(|| "test-key".into()),
        ..KeySources::none()
    };
    let loaded = Settings::load(dir.path(), &sources);
    assert_eq!(loaded.subagent_model, "  child-model  ");
    let config = crate::engine::config_for(
        dir.path(),
        &loaded,
        None,
        &sources,
        flint_agent::ApprovalMode::Auto,
    )
    .expect("config");
    assert_eq!(config.subagent_model.as_deref(), Some("child-model"));
    let sources = KeySources {
        env: |name| match name {
            "OPENAI_API_KEY" => Some("test-key".into()),
            "FLINT_SUBAGENT_MODEL" => Some("override-model".into()),
            _ => None,
        },
        ..KeySources::none()
    };
    let config = crate::engine::config_for(
        dir.path(),
        &loaded,
        None,
        &sources,
        flint_agent::ApprovalMode::Auto,
    )
    .expect("config");
    assert_eq!(config.subagent_model.as_deref(), Some("override-model"));
}

#[test]
fn an_existing_config_file_is_respected() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        Settings::path(dir.path()),
        "model = \"llama3.2\"\nbase_url = \"http://localhost:11434/v1\"\napi_key_env = \"MY_KEY\"\n",
    )
    .expect("write");
    assert_eq!(
        Settings::load(dir.path(), &KeySources::none()),
        Settings {
            model: "llama3.2".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            api_key_env: "MY_KEY".to_string(),
            ..Settings::default()
        }
    );
}

#[test]
fn the_key_file_override_wins_and_is_trimmed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let key = dir.path().join("key");
    std::fs::write(&key, "  sk-test\n").expect("write");
    let settings = Settings {
        api_key_file: dir.path().join("other").display().to_string(),
        ..Settings::default()
    };
    let found = settings
        .resolve_key(Some(&key), &KeySources::none())
        .expect("key");
    assert_eq!(found.key, "sk-test");
    assert_eq!(
        settings.key_status(Some(&key), &KeySources::none()),
        KeyStatus::Found(key.display().to_string())
    );
}

#[test]
fn an_early_install_resolves_to_the_local_shim_and_its_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    let legacy = dir.path().join("legacy.key");
    std::fs::write(&legacy, "legacy-key\n").expect("write");
    let sources = KeySources {
        legacy_key_file: Some(legacy.clone()),
        ..KeySources::none()
    };
    let settings = Settings::load(dir.path(), &sources);
    assert_eq!(settings.base_url, "http://127.0.0.1:18433/v1");
    assert_eq!(settings.model, "deepseek-v4.1-flash");
    let found = settings.resolve_key(None, &sources).expect("key");
    assert_eq!(found.key, "legacy-key");
}

#[test]
fn the_legacy_key_is_never_used_for_another_endpoint() {
    let dir = tempfile::tempdir().expect("tempdir");
    let legacy = dir.path().join("legacy.key");
    std::fs::write(&legacy, "legacy-key\n").expect("write");
    let sources = KeySources {
        legacy_key_file: Some(legacy),
        ..KeySources::none()
    };
    assert!(Settings::default().resolve_key(None, &sources).is_none());
}

#[test]
fn engine_overrides_cannot_route_legacy_credentials_to_another_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("legacy.key");
    std::fs::write(&key, "dummy").unwrap();
    let sources = KeySources {
        env: |name| (name == "FLINT_BASE_URL").then(|| "http://127.0.0.1:9/v1".into()),
        legacy_key_file: Some(key),
    };
    let settings = Settings::load(dir.path(), &sources);
    assert!(
        crate::engine::config_for(dir.path(), &settings, None, &sources, ApprovalMode::Auto)
            .is_err()
    );
}

#[test]
fn environment_keys_come_from_the_injected_lookup() {
    let sources = KeySources {
        env: |name| (name == "OPENAI_API_KEY").then(|| "env-key".to_string()),
        ..KeySources::none()
    };
    let found = Settings::default()
        .resolve_key(None, &sources)
        .expect("key");
    assert_eq!(
        (found.key.as_str(), found.source.as_str()),
        ("env-key", "$OPENAI_API_KEY")
    );
}

#[test]
fn subtle_text_meets_normal_text_contrast_on_primary_surfaces() {
    fn luminance(color: gpui_kit::Hsla) -> f32 {
        let color = color.to_rgb();
        let linear = |value: f32| {
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
    }
    let p = crate::theme::palette();
    for background in [p.bg, p.surface, p.raised, p.bubble] {
        let contrast = (luminance(p.text_subtle) + 0.05) / (luminance(background) + 0.05);
        assert!(contrast >= 4.5, "contrast {contrast} is below 4.5:1");
    }
}
