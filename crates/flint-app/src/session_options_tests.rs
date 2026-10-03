use flint_agent::OptionChoice;
use flint_agent::SessionOption;
use pretty_assertions::assert_eq;

use super::is_permissive;
use super::next_mode;
use super::slots;
use super::toggled_value;

fn option(id: &str, category: Option<&str>, current: &str, values: &[&str]) -> SessionOption {
    SessionOption {
        id: id.to_string(),
        name: id.to_string(),
        description: None,
        category: category.map(str::to_string),
        current: current.to_string(),
        choices: values
            .iter()
            .map(|v| OptionChoice {
                value: (*v).to_string(),
                name: (*v).to_string(),
                description: None,
            })
            .collect(),
    }
}

#[test]
fn options_land_in_their_chips() {
    let claude = [
        option(
            "mode",
            Some("mode"),
            "default",
            &[
                "auto",
                "default",
                "acceptEdits",
                "plan",
                "dontAsk",
                "bypassPermissions",
            ],
        ),
        option(
            "model",
            Some("model"),
            "default",
            &["default", "opus", "sonnet"],
        ),
        option("effort", Some("thought_level"), "high", &["low", "high"]),
        option("fast", Some("model_config"), "off", &["on", "off"]),
        option("agent", None, "default", &["default", "reviewer"]),
    ];
    let slots = slots(&claude);
    let ids = |o: &Option<SessionOption>| o.as_ref().map(|o| o.id.clone());
    assert_eq!(
        (
            ids(&slots.model),
            ids(&slots.reasoning),
            ids(&slots.mode),
            ids(&slots.fast)
        ),
        (
            Some("model".into()),
            Some("effort".into()),
            Some("mode".into()),
            Some("fast".into())
        )
    );
    assert_eq!(
        slots.more.iter().map(|o| o.id.clone()).collect::<Vec<_>>(),
        vec!["agent".to_string()]
    );
    assert_eq!(
        slots.fast.as_ref().and_then(toggled_value),
        Some("on".into())
    );
}

#[test]
fn shift_tab_skips_permissive_modes() {
    let claude = option(
        "mode",
        Some("mode"),
        "default",
        &[
            "auto",
            "default",
            "acceptEdits",
            "plan",
            "dontAsk",
            "bypassPermissions",
        ],
    );
    assert_eq!(next_mode(&claude), Some("acceptEdits".into()));
    let at_plan = SessionOption {
        current: "plan".into(),
        ..claude.clone()
    };
    assert_eq!(next_mode(&at_plan), Some("auto".into()));
    let from_bypass = SessionOption {
        current: "bypassPermissions".into(),
        ..claude
    };
    assert_eq!(next_mode(&from_bypass), Some("auto".into()));
    let codex = option(
        "mode",
        Some("mode"),
        "agent",
        &["read-only", "agent", "agent-full-access"],
    );
    assert_eq!(next_mode(&codex), Some("read-only".into()));
    assert!(
        is_permissive("agent-full-access")
            && is_permissive("bypassPermissions")
            && !is_permissive("plan")
    );
}

#[test]
fn droid_shift_tab_does_not_silently_enable_high_autonomy() {
    let droid = option(
        "autonomy_level",
        Some("mode"),
        "auto-medium",
        &["normal", "spec", "auto-low", "auto-medium", "auto-high"],
    );
    assert_eq!(next_mode(&droid), Some("normal".into()));
    assert_eq!(
        next_mode(&SessionOption {
            current: "normal".into(),
            ..droid
        }),
        Some("spec".into())
    );
    assert!(is_permissive("auto-high"));
    assert!(!is_permissive("auto-low"));
}
