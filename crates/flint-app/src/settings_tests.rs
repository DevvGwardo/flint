use pretty_assertions::assert_eq;

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
        Settings::load(dir.path()),
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
    let found = settings.resolve_key(Some(&key)).expect("key");
    assert_eq!(found.key, "sk-test");
    assert_eq!(
        settings.key_status(Some(&key)),
        KeyStatus::Found(key.display().to_string())
    );
}
