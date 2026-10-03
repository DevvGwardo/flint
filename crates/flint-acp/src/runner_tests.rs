//! The runner against the fake agent in [`crate::test_support`].

use flint_agent::AgentEvent;
use flint_agent::ApprovalDecision;
use flint_agent::ApprovalMode;
use flint_agent::Op;
use flint_agent::ToolKind;
use flint_agent::TurnEndReason;
use pretty_assertions::assert_eq;
use serde_json::json;

use crate::test_support::outline;
use crate::test_support::start;

#[tokio::test]
async fn maps_a_turn_of_updates() {
    let (harness, mut fake) = start(ApprovalMode::Auto);
    fake.handshake().await;
    harness.send(Op::UserMessage("make hello.py".into())).await;
    let prompt = fake.expect("session/prompt").await;
    assert_eq!(
        prompt["params"]["prompt"],
        json!([{"type": "text", "text": "make hello.py"}])
    );
    let hello = format!("{}/hello.py", harness.workspace.display());
    fake.update(json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "plan"}})).await;
    fake.update(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Writing it."}})).await;
    fake.update(
        json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Write hello.py",
        "kind": "edit", "status": "pending", "rawInput": {"file_path": "hello.py"}}),
    )
    .await;
    fake.update(
        json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed",
        "content": [{"type": "diff", "path": hello, "oldText": null, "newText": "print('hi')\n"}]}),
    )
    .await;
    fake.update(
        json!({"sessionUpdate": "tool_call", "toolCallId": "t2", "title": "python3 hello.py",
        "kind": "execute", "status": "in_progress"}),
    )
    .await;
    fake.update(
        json!({"sessionUpdate": "tool_call_update", "toolCallId": "t2", "status": "completed",
        "content": [{"type": "content", "content": {"type": "text", "text": "hi\n"}}]}),
    )
    .await;
    fake.update(json!({"sessionUpdate": "plan", "entries": []}))
        .await;
    fake.update(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Done."}})).await;
    fake.respond(
        &prompt,
        json!({"stopReason": "end_turn", "usage": {"totalTokens": 30, "inputTokens": 20, "outputTokens": 10}}),
    )
    .await;

    assert_eq!(
        outline(&harness.next_turn().await),
        vec![
            "options ",
            "turn 1",
            "step 0",
            "thinking plan",
            "text Writing it.",
            "start t1 Edit Write hello.py",
            "finish t1 ok=true \"\" diff hello.py +1 -0",
            "start t2 Command python3 hello.py",
            "finish t2 ok=true \"hi\\n\"",
            "step 1",
            "text Done.",
            "usage 20 10",
            "finished Completed",
        ]
    );
}

#[tokio::test]
async fn permission_round_trip_picks_the_matching_option() {
    let (harness, mut fake) = start(ApprovalMode::AskForChanges);
    fake.handshake().await;
    harness.send(Op::UserMessage("clean up".into())).await;
    let prompt = fake.expect("session/prompt").await;

    fake.permission("p1", "t1", "rm -rf build").await;
    let approval = harness
        .until(|e| matches!(e, AgentEvent::ApprovalRequested { .. }))
        .await;
    assert_eq!(
        approval,
        AgentEvent::ApprovalRequested {
            call_id: "t1".into(),
            kind: ToolKind::Command,
            summary: "rm -rf build".into()
        }
    );
    harness
        .send(Op::Approval {
            call_id: "t1".into(),
            decision: ApprovalDecision::Deny,
        })
        .await;
    let answer = fake.recv().await;
    assert_eq!(answer["id"], "p1");
    assert_eq!(
        answer["result"]["outcome"],
        json!({"outcome": "selected", "optionId": "no"})
    );

    // "Always" sticks: the next request is answered without asking.
    fake.permission("p2", "t2", "ls").await;
    harness
        .until(|e| matches!(e, AgentEvent::ApprovalRequested { call_id, .. } if call_id == "t2"))
        .await;
    harness
        .send(Op::Approval {
            call_id: "t2".into(),
            decision: ApprovalDecision::ApproveAlways,
        })
        .await;
    assert_eq!(fake.recv().await["result"]["outcome"]["optionId"], "always");
    fake.permission("p3", "t3", "pwd").await;
    assert_eq!(fake.recv().await["result"]["outcome"]["optionId"], "yes");

    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;
    let events = harness.next_turn().await;
    assert_eq!(
        events.last(),
        Some(&AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed
        })
    );
}

#[tokio::test]
async fn interrupt_sends_cancel_and_ends_interrupted() {
    let (harness, mut fake) = start(ApprovalMode::AskForChanges);
    fake.handshake().await;
    harness.send(Op::UserMessage("long task".into())).await;
    let prompt = fake.expect("session/prompt").await;
    fake.update(
        json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "sleep 100",
        "kind": "execute", "status": "in_progress"}),
    )
    .await;
    fake.permission("p1", "t2", "rm x").await;
    harness
        .until(|e| matches!(e, AgentEvent::ApprovalRequested { .. }))
        .await;
    harness.send(Op::Interrupt).await;
    // The pending permission is cancelled, then session/cancel arrives.
    let cancelled = fake.recv().await;
    assert_eq!(
        cancelled["result"]["outcome"],
        json!({"outcome": "cancelled"})
    );
    let cancel = fake.expect("session/cancel").await;
    assert_eq!(cancel["params"]["sessionId"], "s1");
    fake.respond(&prompt, json!({"stopReason": "cancelled"}))
        .await;
    let events = harness.next_turn().await;
    let tail: Vec<String> = outline(&events).into_iter().rev().take(3).collect();
    assert_eq!(
        tail,
        vec![
            "finished Interrupted",
            "finish t2 ok=false \"Interrupted.\"",
            "finish t1 ok=false \"Interrupted.\"",
        ]
    );
}

#[tokio::test]
async fn fs_requests_stay_in_the_workspace_and_writes_become_diffs() {
    let (harness, mut fake) = start(ApprovalMode::Auto);
    fake.handshake().await;
    std::fs::write(harness.workspace.join("a.txt"), "one\ntwo\nthree\n").expect("write");
    harness.send(Op::UserMessage("edit a.txt".into())).await;
    let prompt = fake.expect("session/prompt").await;
    let ws = harness.workspace.display().to_string();

    fake.send(
        json!({"jsonrpc": "2.0", "id": 1, "method": "fs/read_text_file",
        "params": {"sessionId": "s1", "path": format!("{ws}/a.txt"), "line": 2, "limit": 1}}),
    )
    .await;
    assert_eq!(fake.recv().await["result"]["content"], "two");
    fake.send(
        json!({"jsonrpc": "2.0", "id": 2, "method": "fs/read_text_file",
        "params": {"sessionId": "s1", "path": "/etc/hosts"}}),
    )
    .await;
    let refused = fake.recv().await;
    assert!(
        refused["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("outside the workspace")),
        "{refused}"
    );

    // A write during an edit call becomes that call's diff.
    fake.update(
        json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Edit a.txt",
        "kind": "edit", "status": "in_progress"}),
    )
    .await;
    fake.send(json!({"jsonrpc": "2.0", "id": 3, "method": "fs/write_text_file",
        "params": {"sessionId": "s1", "path": format!("{ws}/a.txt"), "content": "one\n2\nthree\n"}}))
        .await;
    assert_eq!(fake.recv().await["result"], json!({}));
    fake.update(
        json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed"}),
    )
    .await;
    // A write outside any edit call gets its own row.
    fake.send(
        json!({"jsonrpc": "2.0", "id": 4, "method": "fs/write_text_file",
        "params": {"sessionId": "s1", "path": format!("{ws}/new/b.txt"), "content": "b\n"}}),
    )
    .await;
    fake.recv().await;
    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;

    let events = harness.next_turn().await;
    let rows: Vec<String> = outline(&events)
        .into_iter()
        .filter(|r| r.starts_with("finish ") || r.starts_with("start "))
        .collect();
    assert_eq!(
        rows,
        vec![
            "start t1 Edit Edit a.txt",
            "finish t1 ok=true \"\" diff a.txt +1 -1",
            "start fs-write-1 Edit new/b.txt",
            "finish fs-write-1 ok=true \"Wrote new/b.txt (+1 -0)\" diff new/b.txt +1 -0",
        ]
    );
    assert_eq!(
        std::fs::read_to_string(harness.workspace.join("a.txt")).expect("read"),
        "one\n2\nthree\n"
    );
}

#[tokio::test]
async fn crash_mid_turn_reports_a_clear_error() {
    let (harness, mut fake) = start(ApprovalMode::Auto);
    fake.handshake().await;
    harness.send(Op::UserMessage("hi".into())).await;
    fake.expect("session/prompt").await;
    fake.update(
        json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "ls",
        "kind": "execute", "status": "in_progress"}),
    )
    .await;
    if let Ok(mut stderr) = harness.stderr.lock() {
        stderr.push_str("Error: Please run /login to authenticate\n");
    }
    drop(fake);
    let events = harness.next_turn().await;
    let error = events.iter().find_map(|e| match e {
        AgentEvent::Error(message) => Some(message.clone()),
        _ => None,
    });
    assert!(
        error
            .as_deref()
            .is_some_and(|m| m.contains("isn't logged in") && m.contains("Run `claude` once")),
        "{error:?}"
    );
    assert!(matches!(
        events.last(),
        Some(AgentEvent::TurnFinished {
            reason: TurnEndReason::Failed(_),
            ..
        })
    ));
    assert!(outline(&events).contains(&"finish t1 ok=false \"Interrupted.\"".to_string()));
}

#[tokio::test]
async fn auth_error_on_new_session_says_how_to_log_in() {
    let (harness, mut fake) = start(ApprovalMode::Auto);
    let init = fake.expect("initialize").await;
    fake.respond(
        &init,
        json!({"protocolVersion": 1, "agentCapabilities": {}}),
    )
    .await;
    let new = fake.expect("session/new").await;
    fake.send(json!({"jsonrpc": "2.0", "id": new["id"],
        "error": {"code": -32000, "message": "Authentication required"}}))
        .await;
    let error = harness.until(|e| matches!(e, AgentEvent::Error(_))).await;
    assert_eq!(
        error,
        AgentEvent::Error(
            "Claude Code isn't logged in (Authentication required). Run `claude` once in a \
             terminal to log in, then retry."
                .into()
        )
    );
}

/// The ACP terminal extension: the agent asks flint to run a command, reads
/// its output, waits for its exit and releases it. Each command becomes a
/// read-only terminal tab (the `TerminalStarted`/`Output`/`Exited` events).
#[tokio::test]
async fn runs_the_terminals_the_agent_asks_for() {
    let (harness, mut fake) = start(ApprovalMode::Auto);
    let init = fake.expect("initialize").await;
    assert_eq!(
        init["params"]["clientCapabilities"]["terminal"],
        json!(true),
        "the terminal capability is advertised"
    );
    assert_eq!(
        init["params"]["clientCapabilities"]["_meta"]["terminal_output"],
        json!(true),
        "the terminal output extension is opted into"
    );
    fake.respond(
        &init,
        json!({"protocolVersion": 1, "agentCapabilities": {"loadSession": false}}),
    )
    .await;
    let new = fake.expect("session/new").await;
    fake.respond(&new, json!({"sessionId": "s1"})).await;

    let ask = |id: &str, method: &str, params: serde_json::Value| json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    fake.send(ask(
        "t1",
        "terminal/create",
        json!({"sessionId": "s1", "command": "printf", "args": ["hi\\n"]}),
    ))
    .await;
    let created = fake.answered("t1").await;
    let terminal_id = created["result"]["terminalId"].as_str().expect("id");
    assert!(terminal_id.starts_with("flint-term-"), "{created}");

    let started = harness
        .until(|e| matches!(e, AgentEvent::TerminalStarted { .. }))
        .await;
    let AgentEvent::TerminalStarted { label, cwd, .. } = started else {
        panic!("terminal started")
    };
    assert_eq!(label, "claude: printf hi\\n");
    assert_eq!(cwd.as_deref(), Some(harness.workspace.as_path()));

    // Wait for the exit first, so the output below is the whole of it.
    fake.send(ask(
        "t2",
        "terminal/wait_for_exit",
        json!({"sessionId": "s1", "terminalId": terminal_id}),
    ))
    .await;
    let waited = fake.answered("t2").await;
    assert_eq!(waited["result"]["exitCode"], json!(0));

    fake.send(ask(
        "t3",
        "terminal/output",
        json!({"sessionId": "s1", "terminalId": terminal_id}),
    ))
    .await;
    let output = fake.answered("t3").await;
    assert_eq!(output["result"]["output"], json!("hi\n"));
    assert_eq!(output["result"]["truncated"], json!(false));

    let events: Vec<AgentEvent> = vec![
        harness
            .until(|e| matches!(e, AgentEvent::TerminalOutput { .. }))
            .await,
        harness
            .until(|e| matches!(e, AgentEvent::TerminalExited { .. }))
            .await,
    ];
    assert!(matches!(&events[0], AgentEvent::TerminalOutput { data, .. } if data == "hi\n"));
    assert!(matches!(
        &events[1],
        AgentEvent::TerminalExited {
            exit_code: Some(0),
            ..
        }
    ));

    fake.send(ask(
        "t4",
        "terminal/release",
        json!({"sessionId": "s1", "terminalId": terminal_id}),
    ))
    .await;
    assert!(fake.answered("t4").await["result"].is_object());
    // The terminal is gone: further requests fail.
    fake.send(ask(
        "t5",
        "terminal/output",
        json!({"sessionId": "s1", "terminalId": terminal_id}),
    ))
    .await;
    assert!(fake.answered("t5").await["error"].is_object());
}

/// Commands run in the workspace, and never outside it.
#[tokio::test]
async fn refuses_a_terminal_outside_the_workspace() {
    let (harness, mut fake) = start(ApprovalMode::Auto);
    fake.handshake().await;
    fake.send(
        json!({"jsonrpc": "2.0", "id": "t1", "method": "terminal/create",
        "params": {"sessionId": "s1", "command": "pwd", "cwd": "/"}}),
    )
    .await;
    let refused = fake.answered("t1").await;
    assert!(
        refused["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("outside the workspace")),
        "{refused}"
    );
    let _ = harness;
}

/// The client kills a command when the agent asks, and reports the signal.
#[tokio::test]
async fn kills_a_terminal_on_request() {
    let (harness, mut fake) = start(ApprovalMode::Auto);
    fake.handshake().await;
    fake.send(
        json!({"jsonrpc": "2.0", "id": "t1", "method": "terminal/create",
        "params": {"sessionId": "s1", "command": "sleep", "args": ["30"]}}),
    )
    .await;
    let created = fake.answered("t1").await;
    let terminal_id = created["result"]["terminalId"].as_str().expect("id");
    fake.send(
        json!({"jsonrpc": "2.0", "id": "t2", "method": "terminal/kill",
        "params": {"sessionId": "s1", "terminalId": terminal_id}}),
    )
    .await;
    assert!(fake.answered("t2").await["result"].is_object());
    let exited = harness
        .until(|e| matches!(e, AgentEvent::TerminalExited { .. }))
        .await;
    assert!(matches!(exited, AgentEvent::TerminalExited { .. }));
}
