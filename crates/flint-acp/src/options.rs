//! The agent's session options (ACP `configOptions`) as flint
//! [`SessionOption`]s, and back.
//!
//! ACP has select options (a list of values) and boolean options. flint shows
//! both as choices; a boolean becomes the choices `"true"` / `"false"`, and
//! [`Options::wire_value`] turns those back into a boolean when it is set.

use std::collections::HashSet;

use agent_client_protocol::schema::v1::SessionConfigKind;
use agent_client_protocol::schema::v1::SessionConfigOption;
use agent_client_protocol::schema::v1::SessionConfigOptionCategory;
use agent_client_protocol::schema::v1::SessionConfigOptionValue;
use agent_client_protocol::schema::v1::SessionConfigSelectOption;
use agent_client_protocol::schema::v1::SessionConfigSelectOptions;
use flint_agent::OptionChoice;
use flint_agent::SessionOption;

/// The category flint uses for permission modes.
pub const MODE_CATEGORY: &str = "mode";

/// The agent's current options.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Options {
    pub list: Vec<SessionOption>,
    booleans: HashSet<String>,
}

impl Options {
    pub fn from_acp(options: &[SessionConfigOption]) -> Self {
        let mut booleans = HashSet::new();
        let list = options
            .iter()
            .filter_map(|option| {
                let id = option.id.0.to_string();
                let (current, choices) = match &option.kind {
                    SessionConfigKind::Select(select) => {
                        let choices = match &select.options {
                            SessionConfigSelectOptions::Ungrouped(options) => {
                                options.iter().map(choice).collect()
                            }
                            SessionConfigSelectOptions::Grouped(groups) => groups
                                .iter()
                                .flat_map(|group| group.options.iter().map(choice))
                                .collect(),
                            _ => Vec::new(),
                        };
                        (select.current_value.0.to_string(), choices)
                    }
                    SessionConfigKind::Boolean(boolean) => {
                        booleans.insert(id.clone());
                        let choices = [("true", "On"), ("false", "Off")]
                            .map(|(value, name)| OptionChoice {
                                value: value.to_string(),
                                name: name.to_string(),
                                description: None,
                            })
                            .to_vec();
                        (boolean.current_value.to_string(), choices)
                    }
                    // A kind flint doesn't know can't be shown or set.
                    _ => return None,
                };
                Some(SessionOption {
                    id,
                    name: option.name.clone(),
                    description: option.description.clone(),
                    category: option.category.as_ref().map(category_name),
                    current,
                    choices,
                })
            })
            .collect();
        Self { list, booleans }
    }

    pub fn get(&self, id: &str) -> Option<&SessionOption> {
        self.list.iter().find(|option| option.id == id)
    }

    /// The agent has a permission-mode option, so its mode decides approvals.
    pub fn has_mode(&self) -> bool {
        self.list
            .iter()
            .any(|option| option.category.as_deref() == Some(MODE_CATEGORY))
    }

    /// The ACP value for setting `id` to `value`; `None` if `value` isn't
    /// one of its choices.
    pub fn wire_value(&self, id: &str, value: &str) -> Option<SessionConfigOptionValue> {
        let option = self.get(id)?;
        if !option.choices.iter().any(|c| c.value == value) {
            return None;
        }
        Some(if self.booleans.contains(id) {
            SessionConfigOptionValue::boolean(value == "true")
        } else {
            SessionConfigOptionValue::value_id(value.to_string())
        })
    }
}

fn choice(option: &SessionConfigSelectOption) -> OptionChoice {
    OptionChoice {
        value: option.value.0.to_string(),
        name: option.name.clone(),
        description: option.description.clone(),
    }
}

fn category_name(category: &SessionConfigOptionCategory) -> String {
    match category {
        SessionConfigOptionCategory::Mode => MODE_CATEGORY.to_string(),
        SessionConfigOptionCategory::Model => "model".to_string(),
        SessionConfigOptionCategory::ModelConfig => "model_config".to_string(),
        SessionConfigOptionCategory::ThoughtLevel => "thought_level".to_string(),
        SessionConfigOptionCategory::Other(other) => other.clone(),
        _ => "other".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn select_grouped_and_boolean_options() {
        let raw: Vec<SessionConfigOption> = serde_json::from_value(json!([
            {"id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": "default",
             "options": [{"value": "default", "name": "Manual", "description": "Ask first"},
                         {"value": "plan", "name": "Plan"}]},
            {"id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "b",
             "options": [{"group": "g", "name": "Group", "options": [{"value": "a", "name": "A"}, {"value": "b", "name": "B"}]}]},
            {"id": "fast", "name": "Fast", "category": "model_config", "type": "boolean", "currentValue": false},
            {"id": "x", "name": "X", "category": "collaboration_mode", "type": "select", "currentValue": "p",
             "options": [{"value": "p", "name": "P"}]}
        ]))
        .expect("options");
        let options = Options::from_acp(&raw);
        let summary: Vec<(String, Option<String>, String, Vec<String>)> = options
            .list
            .iter()
            .map(|o| {
                (
                    o.id.clone(),
                    o.category.clone(),
                    o.current.clone(),
                    o.choices.iter().map(|c| c.name.clone()).collect(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (
                    "mode".into(),
                    Some("mode".into()),
                    "default".into(),
                    vec!["Manual".into(), "Plan".into()]
                ),
                (
                    "model".into(),
                    Some("model".into()),
                    "b".into(),
                    vec!["A".into(), "B".into()]
                ),
                (
                    "fast".into(),
                    Some("model_config".into()),
                    "false".into(),
                    vec!["On".into(), "Off".into()]
                ),
                (
                    "x".into(),
                    Some("collaboration_mode".into()),
                    "p".into(),
                    vec!["P".into()]
                ),
            ]
        );
        assert!(options.has_mode());
        assert_eq!(
            serde_json::to_value(options.wire_value("fast", "true")).ok(),
            Some(json!({"type": "boolean", "value": true}))
        );
        assert_eq!(
            serde_json::to_value(options.wire_value("mode", "plan")).ok(),
            Some(json!({"value": "plan"}))
        );
        assert_eq!(options.wire_value("mode", "nope"), None);
        assert_eq!(options.wire_value("missing", "plan"), None);
    }
}
