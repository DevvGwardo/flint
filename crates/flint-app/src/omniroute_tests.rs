use super::*;

fn omniroute() -> Settings {
    Settings {
        provider: ProviderKind::OmniRoute,
        base_url: OMNIROUTE_BASE_URL.into(),
        model: OMNIROUTE_MODEL.into(),
        ..Settings::default()
    }
}

#[test]
fn omniroute_settings_are_backward_compatible_and_persist() {
    let old: Settings = toml::from_str("model = \"kept-model\"").unwrap();
    assert_eq!(old.provider, ProviderKind::OpenAiCompatible);
    assert!(!old.omniroute_no_auth);
    let dir = tempfile::tempdir().unwrap();
    for no_auth in [false, true] {
        let settings = Settings {
            omniroute_no_auth: no_auth,
            ..omniroute()
        };
        settings.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path(), &KeySources::none()), settings);
        let text = toml::to_string(&settings).unwrap();
        assert!(text.contains("provider = \"omniroute\""));
    }
}

#[test]
fn omniroute_never_borrows_upstream_or_legacy_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join("legacy-key");
    std::fs::write(&legacy, "legacy-fixture").unwrap();
    let sources = KeySources {
        env: |name| {
            matches!(name, "FLINT_API_KEY" | "OPENAI_API_KEY").then(|| "upstream-fixture".into())
        },
        legacy_key_file: Some(legacy),
    };
    let settings = omniroute();
    assert!(settings.resolve_key(None, &sources).is_none());
    assert_eq!(
        settings.key_status(None, &sources),
        KeyStatus::Missing("$OMNIROUTE_API_KEY".into())
    );
    let sources = KeySources {
        env: |name| (name == OMNIROUTE_KEY_ENV).then(|| "router-fixture".into()),
        ..KeySources::none()
    };
    assert_eq!(
        settings.resolve_key(None, &sources).unwrap().key,
        "router-fixture"
    );
    let key = dir.path().join("router-key");
    std::fs::write(&key, "file-fixture").unwrap();
    let settings = Settings {
        api_key_file: key.display().to_string(),
        ..settings
    };
    assert_eq!(
        settings.resolve_key(None, &sources).unwrap().key,
        "file-fixture"
    );
}

#[test]
fn omniroute_no_auth_is_explicit_and_only_allows_loopback_endpoints() {
    for base in [
        OMNIROUTE_BASE_URL,
        "http://localhost:20128/v1",
        "http://127.2.3.4:9000/custom/v1",
        "http://[::1]:20128/v1",
    ] {
        let settings = Settings {
            base_url: base.into(),
            omniroute_no_auth: true,
            ..omniroute()
        };
        assert!(settings.validate_provider().is_ok(), "{base}");
        assert!(
            settings
                .resolve_key(None, &KeySources::none())
                .unwrap()
                .key
                .is_empty()
        );
        assert_eq!(
            settings.key_status(None, &KeySources::none()),
            KeyStatus::NotRequired
        );
    }
    for base in [
        "https://router.example/v1",
        "http://192.168.1.10:20128/v1",
        "http://localhost.evil.example/v1",
        "http://localhost@router.example/v1",
        "http://user:password@localhost:20128/v1",
        "http://localhost:20128/v1?token=fixture",
        "file:///v1",
    ] {
        let settings = Settings {
            base_url: base.into(),
            omniroute_no_auth: true,
            ..omniroute()
        };
        assert!(settings.validate_provider().is_err(), "{base}");
        assert!(
            settings.resolve_key(None, &KeySources::none()).is_none(),
            "{base}"
        );
    }
    assert!(omniroute().resolve_key(None, &KeySources::none()).is_none());
}

#[test]
fn omniroute_no_auth_does_not_forward_even_explicit_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("fixture-key");
    std::fs::write(&key, "must-not-be-sent").unwrap();
    let sources = KeySources {
        env: |_| Some("must-not-be-sent".into()),
        ..KeySources::none()
    };
    let settings = Settings {
        omniroute_no_auth: true,
        api_key_file: key.display().to_string(),
        ..omniroute()
    };
    assert!(
        settings
            .resolve_key(Some(&key), &sources)
            .unwrap()
            .key
            .is_empty()
    );
}

#[test]
fn omniroute_engine_config_and_overrides_preserve_local_auth_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        omniroute_no_auth: true,
        ..omniroute()
    };
    let config = crate::engine::config_for(
        dir.path(),
        &settings,
        None,
        &KeySources::none(),
        flint_agent::ApprovalMode::AskForChanges,
    )
    .unwrap();
    assert_eq!(config.base_url, OMNIROUTE_BASE_URL);
    assert_eq!(config.model, "auto");
    assert!(config.api_key.is_empty());
    assert!(config.sandbox);
    let sources = KeySources {
        env: |name| (name == "FLINT_BASE_URL").then(|| "https://router.example/v1".into()),
        ..KeySources::none()
    };
    assert!(
        crate::engine::config_for(
            dir.path(),
            &settings,
            None,
            &sources,
            flint_agent::ApprovalMode::AskForChanges,
        )
        .unwrap_err()
        .to_string()
        .contains("loopback")
    );
    let settings = omniroute();
    assert!(
        crate::engine::config_for(
            dir.path(),
            &settings,
            None,
            &KeySources::none(),
            flint_agent::ApprovalMode::AskForChanges,
        )
        .unwrap_err()
        .to_string()
        .contains(OMNIROUTE_KEY_ENV)
    );
}

#[test]
fn omniroute_auth_changes_invalidate_credential_status_cache() {
    let mut cache = KeyStatusCache::default();
    let now = std::time::Instant::now();
    let settings = omniroute();
    assert!(matches!(
        cache.get(&settings, None, &KeySources::none(), now),
        KeyStatus::Missing(_)
    ));
    let settings = Settings {
        omniroute_no_auth: true,
        ..settings
    };
    assert_eq!(
        cache.get(&settings, None, &KeySources::none(), now),
        KeyStatus::NotRequired
    );
    let settings = Settings {
        provider: ProviderKind::OpenAiCompatible,
        ..settings
    };
    assert!(matches!(
        cache.get(&settings, None, &KeySources::none(), now),
        KeyStatus::Missing(_)
    ));
}
