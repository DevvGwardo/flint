use pretty_assertions::assert_eq;

use super::KeySources;
use super::KeyStatus;
use super::Settings;
use super::jev_key_set;
use super::key_with_typesafe_fallback;
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
fn agent_model_provider_defaults_round_trip_without_changing_native_settings() {
    use flint_agent::AgentKind;
    let dir = tempfile::tempdir().unwrap();
    let mut settings = Settings {
        default_agent: AgentKind::Droid,
        model: "native-selected-model".into(),
        base_url: "https://selected-provider.example/v1".into(),
        ..Settings::default()
    };
    settings.agent_options.insert(
        "droid".into(),
        [
            ("model".into(), "droid-selected-model".into()),
            ("provider".into(), "selected-provider".into()),
        ]
        .into(),
    );
    settings.save(dir.path()).unwrap();
    let loaded = Settings::load(dir.path(), &KeySources::none());
    assert_eq!(loaded, settings);
    assert_eq!(
        loaded.agent_options(AgentKind::Droid)["model"],
        "droid-selected-model"
    );
    assert!(loaded.agent_options(AgentKind::ClaudeCode).is_empty());
}

#[test]
fn existing_config_keeps_model_provider_and_defaults_to_native_agent() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        Settings::path(dir.path()),
        "model = \"selected-model\"\nbase_url = \"https://selected-provider.example/v1\"\n",
    )
    .unwrap();
    let loaded = Settings::load(dir.path(), &KeySources::none());
    assert_eq!(loaded.default_agent, flint_agent::AgentKind::Flint);
    assert_eq!(loaded.model, "selected-model");
    assert_eq!(loaded.base_url, "https://selected-provider.example/v1");
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
fn typesafe_private_file_fallback_is_trimmed_and_scoped_to_the_judge() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("typesafe.key");
    std::fs::write(&file, "  fixture-typesafe-key\n").expect("write");
    assert_eq!(
        key_with_typesafe_fallback("TYPESAFE_API_KEY", None, &file).as_deref(),
        Some("fixture-typesafe-key")
    );
    for name in ["FLINT_API_KEY", "OPENAI_API_KEY", "OTHER_PROVIDER_KEY"] {
        assert!(key_with_typesafe_fallback(name, None, &file).is_none());
    }
    assert_eq!(
        key_with_typesafe_fallback(
            "TYPESAFE_API_KEY",
            Some("  environment-override  ".into()),
            &file,
        )
        .as_deref(),
        Some("environment-override")
    );
    std::fs::write(&file, " \n").expect("empty");
    assert!(key_with_typesafe_fallback("TYPESAFE_API_KEY", None, &file).is_none());
    assert!(!jev_key_set(&KeySources::none()));
}

#[test]
fn typesafe_judge_uses_injected_credentials_without_changing_the_model_key() {
    let sources = KeySources {
        env: |name| match name {
            "OPENAI_API_KEY" => Some("fixture-model-key".into()),
            "TYPESAFE_API_KEY" => Some("fixture-judge-key".into()),
            _ => None,
        },
        ..KeySources::none()
    };
    assert!(jev_key_set(&sources));
    let dir = tempfile::tempdir().expect("tempdir");
    let config = crate::engine::config_for(
        dir.path(),
        &Settings::default(),
        None,
        &sources,
        ApprovalMode::Auto,
    )
    .expect("config");
    assert_eq!(config.api_key, "fixture-model-key");
    assert_eq!(config.jev.expect("judge").api_key, "fixture-judge-key");
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

#[test]
fn sandbox_and_mcp_servers_load_from_the_config_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(Settings::default().sandbox);
    std::fs::write(
        Settings::path(dir.path()),
        "sandbox = false\n\n[mcp_servers.github]\ncommand = \"github-mcp\"\nargs = [\"stdio\"]\n\
         env = { GITHUB_TOKEN = \"t\" }\n\n[mcp_servers.off]\ncommand = \"x\"\nenabled = false\n",
    )
    .expect("write");
    let settings = Settings::load(dir.path(), &KeySources::none());
    assert!(!settings.sandbox);
    let servers = settings.mcp_server_configs();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].name, "github");
    assert_eq!(servers[0].command, "github-mcp");
    assert_eq!(servers[0].args, ["stdio"]);
    assert_eq!(
        servers[0].env,
        [("GITHUB_TOKEN".to_string(), "t".to_string())]
    );
    // Saving keeps them.
    settings.save(dir.path()).expect("save");
    assert_eq!(Settings::load(dir.path(), &KeySources::none()), settings);
}
