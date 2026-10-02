use pretty_assertions::assert_eq;

use super::KeySources;
use super::KeyStatus;
use super::Settings;

#[test]
fn defaults_point_at_a_public_endpoint() {
    let settings = Settings::default();
    assert_eq!(settings.base_url, "https://api.openai.com/v1");
    assert_eq!(settings.api_key_env, "");
    assert_eq!(settings.api_key_file, "");
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
