use super::*;

fn custom() -> Settings {
    Settings {
        provider: ProviderKind::Custom,
        base_url: "https://provider.example/custom/v1".into(),
        model: "custom-model".into(),
        subagent_model: "custom-small-model".into(),
        api_key_env: "CUSTOM_PROVIDER_KEY".into(),
        ..Settings::default()
    }
}

#[test]
fn existing_settings_keep_their_provider_and_start_without_profiles() {
    let settings: Settings =
        toml::from_str("base_url = \"https://existing.example/v1\"\nmodel = \"existing-model\"\n")
            .unwrap();
    assert_eq!(settings.provider, ProviderKind::OpenAiCompatible);
    assert!(settings.provider_profiles.is_empty());
    assert_eq!(settings.model, "existing-model");
    assert_eq!(settings.base_url, "https://existing.example/v1");
}

#[test]
fn multiple_named_providers_round_trip_and_update_independently() {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = custom();
    settings.save_provider_profile(" My provider ").unwrap();
    settings.model = "second-model".into();
    settings.base_url = "https://second.example/v1".into();
    settings.api_key_env = "SECOND_PROVIDER_KEY".into();
    settings.save_provider_profile("Second").unwrap();
    settings.model = "updated-model".into();
    settings.save_provider_profile("Second").unwrap();
    assert_eq!(settings.provider_profiles.len(), 2);
    assert_eq!(
        settings.provider_profiles["My provider"].model,
        "custom-model"
    );
    assert_eq!(settings.provider_profiles["Second"].model, "updated-model");
    settings.save(dir.path()).unwrap();
    assert_eq!(Settings::load(dir.path(), &KeySources::none()), settings);
}

#[test]
fn applying_a_profile_changes_only_provider_fields() {
    let profile = ProviderProfile::from_settings(&custom());
    let mut settings = Settings {
        approval: "auto".into(),
        sandbox: false,
        jev_judge: false,
        queue_after_verification: true,
        auto_continue_fixes: true,
        effort: "high".into(),
        ..Settings::default()
    };
    let before = settings.clone();
    profile.apply_to(&mut settings);
    assert_eq!(ProviderProfile::from_settings(&settings), profile);
    let mut restored = settings.clone();
    ProviderProfile::from_settings(&before).apply_to(&mut restored);
    assert_eq!(restored, before);
}

#[test]
fn custom_providers_never_borrow_ambient_or_legacy_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join("legacy");
    std::fs::write(&legacy, "legacy-fixture").unwrap();
    let sources = KeySources {
        env: |name| {
            matches!(name, "FLINT_API_KEY" | "OPENAI_API_KEY")
                .then(|| "wrong-provider-fixture".into())
        },
        legacy_key_file: Some(legacy),
    };
    let mut settings = custom();
    for env in ["", "MISSING_PROVIDER_KEY"] {
        settings.api_key_env = env.into();
        settings.base_url = LEGACY_BASE_URL.into();
        assert!(settings.resolve_key(None, &sources).is_none());
    }
    let error = crate::engine::config_for(
        dir.path(),
        &settings,
        None,
        &sources,
        ApprovalMode::AskForChanges,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("custom provider"));
}

#[test]
fn selected_profile_routes_model_endpoint_and_own_key_to_the_engine() {
    let dir = tempfile::tempdir().unwrap();
    let sources = KeySources {
        env: |name| match name {
            "CUSTOM_PROVIDER_KEY" => Some("custom-fixture".into()),
            "SECOND_PROVIDER_KEY" => Some("second-fixture".into()),
            "OPENAI_API_KEY" => Some("must-not-use-fixture".into()),
            _ => None,
        },
        ..KeySources::none()
    };
    let mut settings = custom();
    settings.save_provider_profile("Custom").unwrap();
    settings.base_url = "https://second.example/v1".into();
    settings.model = "second-model".into();
    settings.api_key_env = "SECOND_PROVIDER_KEY".into();
    settings.save_provider_profile("Second").unwrap();
    for name in ["Second", "Custom"] {
        let profile = settings.provider_profiles[name].clone();
        profile.apply_to(&mut settings);
        let config = crate::engine::config_for(
            dir.path(),
            &settings,
            None,
            &sources,
            ApprovalMode::AskForChanges,
        )
        .unwrap();
        assert_eq!(config.base_url, profile.base_url);
        assert_eq!(config.model, profile.model);
        assert_eq!(
            config.subagent_model.as_deref(),
            Some(profile.subagent_model.as_str())
        );
        assert_eq!(
            config.api_key,
            if name == "Custom" {
                "custom-fixture"
            } else {
                "second-fixture"
            }
        );
    }
}

#[test]
fn custom_provider_key_files_and_missing_key_status_are_supported() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("provider-key");
    std::fs::write(&path, " file-fixture\n").unwrap();
    let mut settings = custom();
    settings.api_key_env.clear();
    assert!(matches!(
        settings.key_status(None, &KeySources::none()),
        KeyStatus::Missing(_)
    ));
    settings.api_key_file = path.display().to_string();
    assert_eq!(
        settings.resolve_key(None, &KeySources::none()).unwrap().key,
        "file-fixture"
    );
    let serialized = toml::to_string(&settings).unwrap();
    assert!(!serialized.contains("file-fixture"));
}

#[test]
fn invalid_profile_inputs_are_rejected_without_losing_existing_profiles() {
    let original = custom();
    for (name, base, model, env) in [
        (" ", "https://valid.example/v1", "model", "KEY"),
        ("Bad", "file:///tmp/api", "model", "KEY"),
        ("Bad", "https://user:secret@api.example/v1", "model", "KEY"),
        ("Bad", "https://api.example/v1?token=secret", "model", "KEY"),
        ("Bad", "https://api.example/v1", " ", "KEY"),
        ("Bad", "https://api.example/v1", "model", "not-a-variable"),
    ] {
        let mut settings = original.clone();
        settings.save_provider_profile("Kept").unwrap();
        settings.base_url = base.into();
        settings.model = model.into();
        settings.api_key_env = env.into();
        assert!(settings.save_provider_profile(name).is_err());
        assert_eq!(settings.provider_profiles.len(), 1);
    }
}

#[test]
fn omniroute_profiles_preserve_explicit_local_no_auth() {
    let mut settings = Settings {
        provider: ProviderKind::OmniRoute,
        base_url: OMNIROUTE_BASE_URL.into(),
        model: OMNIROUTE_MODEL.into(),
        omniroute_no_auth: true,
        ..Settings::default()
    };
    settings.save_provider_profile("Local router").unwrap();
    let profile = settings.provider_profiles["Local router"].clone();
    let mut other = custom();
    profile.apply_to(&mut other);
    assert_eq!(
        other.key_status(None, &KeySources::none()),
        KeyStatus::NotRequired
    );
    other.base_url = "https://remote.example/v1".into();
    assert!(other.save_provider_profile("Unsafe router").is_err());
}
