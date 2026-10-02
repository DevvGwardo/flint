use super::*;
use pretty_assertions::assert_eq;

const TOOLS: [&str; 3] = ["read_file", "write_file", "run_command"];

fn leak(name: &str, form: LeakForm) -> Option<LeakedCall> {
    Some(LeakedCall {
        name: name.to_string(),
        form,
    })
}

#[test]
fn recognizes_common_leak_forms_for_offered_tools_only() {
    assert_eq!(
        detect_leaked_tool_call(
            r#"Let me look.<invoke name="read_file"><parameter name="path">a</parameter></invoke>"#,
            &TOOLS
        ),
        leak("read_file", LeakForm::Xml)
    );
    assert_eq!(
        detect_leaked_tool_call("<function=run_command>", &TOOLS),
        leak("run_command", LeakForm::FunctionTag)
    );
    assert_eq!(
        detect_leaked_tool_call(
            r#"<tool_call>{"name":"run_command","arguments":{"command":"ls"}}</tool_call>"#,
            &TOOLS
        ),
        leak("run_command", LeakForm::Json)
    );
    assert_eq!(
        detect_leaked_tool_call(
            "```json\n{\"name\": \"write_file\", \"arguments\": {\"path\": \"a\"}}\n```",
            &TOOLS
        ),
        leak("write_file", LeakForm::Json)
    );
    assert_eq!(
        detect_leaked_tool_call(r#"<invoke name="rm_rf">"#, &TOOLS),
        None
    );
    assert_eq!(
        detect_leaked_tool_call("I used read_file to check.", &TOOLS),
        None
    );
    assert_eq!(
        detect_leaked_tool_call(
            r#"{"name":"read_file","parameters":{"type":"object","properties":{}}}"#,
            &TOOLS
        ),
        None
    );
}
