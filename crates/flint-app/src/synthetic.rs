//! Automation-only synthetic workloads for the perf harness
//! (`tools/blueprint/perf.py`): a long session (`--demo-long N`), a
//! high-rate streaming test (`--stream-test RATE`) and a scroll test
//! (`--scroll-test`). Each test writes its frame log and quits.

use std::time::Duration;

use flint_agent::AgentEvent;
use flint_agent::FileDiff;
use flint_agent::NudgeReason;
use flint_agent::ToolKind;
use flint_agent::TurnEndReason;
use flint_agent::Usage;
use gpui_kit::*;
use serde_json::json;

use crate::app::FlintApp;
use crate::automation;

/// Events for `turns` complete turns with big command outputs and diffs.
pub fn long_session(turns: usize) -> Vec<(String, Vec<AgentEvent>)> {
    (0..turns)
        .map(|t| {
            let mut events = vec![
                AgentEvent::TurnStarted { turn_id: t as u64 + 1 },
                AgentEvent::StepStarted {
                    turn_id: t as u64 + 1,
                    step: 1,
                },
                AgentEvent::ReasoningDelta(format!("Turn {t}: inspect, change, verify. ").repeat(6)),
                tool_start(&format!("c{t}"), ToolKind::Command, "cargo test --workspace"),
            ];
            let output: String = (0..200)
                .map(|n| format!("test module_{t}::case_{n} ... ok ({}ms)\n", n % 17))
                .collect();
            events.push(AgentEvent::ToolOutputDelta {
                call_id: format!("c{t}"),
                chunk: output.clone(),
            });
            events.push(tool_finish(&format!("c{t}"), output, None));
            events.push(tool_start(&format!("e{t}"), ToolKind::Edit, "src/lib.rs"));
            let diff: String = (0..40)
                .map(|n| {
                    if n % 3 == 0 {
                        format!("-    let old_{n} = compute({n});\n+    let new_{n} = compute_fast({n});\n")
                    } else {
                        format!("     context line {n}\n")
                    }
                })
                .collect();
            events.push(tool_finish(
                &format!("e{t}"),
                "Applied edit".into(),
                Some(FileDiff {
                    path: format!("src/module_{}.rs", t % 12),
                    unified: format!("@@ -1,40 +1,40 @@\n{diff}"),
                    added: 14,
                    removed: 14,
                    created: false,
                }),
            ));
            if t % 7 == 0 {
                events.push(AgentEvent::HarnessNudge {
                    reason: NudgeReason::Verify,
                    message: "Run the tests after editing.".into(),
                });
            }
            events.push(AgentEvent::TextDelta(format!(
                "## Turn {t}\n\nUpdated `src/module_{}.rs` and verified with `cargo test`.\n\n\
                 - replaced `compute` with `compute_fast`\n- all 200 tests pass\n",
                t % 12
            )));
            events.push(AgentEvent::Usage(Usage {
                input_tokens: 12_000,
                cached_input_tokens: 9_000,
                output_tokens: 600,
                reasoning_tokens: 200,
            }));
            events.push(AgentEvent::TurnFinished {
                turn_id: t as u64 + 1,
                reason: TurnEndReason::Completed,
            });
            (format!("Synthetic task {t}: speed up module {}", t % 12), events)
        })
        .collect()
}

fn tool_start(call_id: &str, kind: ToolKind, summary: &str) -> AgentEvent {
    AgentEvent::ToolCallStarted {
        call_id: call_id.into(),
        name: "tool".into(),
        kind,
        args: json!({}),
        summary: summary.into(),
    }
}

fn tool_finish(call_id: &str, output: String, diff: Option<FileDiff>) -> AgentEvent {
    AgentEvent::ToolCallFinished {
        call_id: call_id.into(),
        output,
        exit_code: Some(0),
        success: true,
        diff,
        duration_ms: 1_200,
    }
}

impl FlintApp {
    /// Replays `turns` synthetic turns into the active session at once.
    pub(crate) fn load_long_session(&mut self, turns: usize, cx: &mut Context<Self>) {
        let ix = self.active;
        let mut clock = self.now();
        for (prompt, events) in long_session(turns) {
            let change = self.sessions[ix].view.push_user(prompt);
            self.sessions[ix].apply(change);
            for event in events {
                clock += Duration::from_millis(40);
                let uid = self.sessions[ix].uid;
                self.apply_events(uid, vec![event], clock, cx);
            }
        }
    }

    /// Streams `rate` deltas per second (text and command output,
    /// alternating) for five seconds from a producer thread through the real
    /// engine path (`attach_engine`), then writes the frame log and quits.
    pub(crate) fn start_stream_test(&mut self, rate: u32, cx: &mut Context<Self>) {
        let ix = self.active;
        let change = self.sessions[ix].view.push_user("Stream test".to_string());
        self.sessions[ix].apply(change);
        let (ops_tx, _ops_rx) = async_channel::unbounded();
        let (events_tx, events_rx) = async_channel::unbounded();
        self.attach_engine(
            ix,
            flint_agent::SessionHandle {
                ops: ops_tx,
                events: events_rx,
            },
            cx,
        );
        std::thread::spawn(move || {
            let send = |event| events_tx.send_blocking(event).is_ok();
            send(AgentEvent::TurnStarted { turn_id: 1 });
            send(AgentEvent::StepStarted {
                turn_id: 1,
                step: 1,
            });
            send(tool_start("s1", ToolKind::Command, "cargo build --verbose"));
            let started = std::time::Instant::now();
            let per_ms = rate as f64 / 1000.;
            let mut n = 0u64;
            while started.elapsed() < Duration::from_secs(5) {
                let due = (started.elapsed().as_secs_f64() * 1000. * per_ms) as u64;
                while n < due {
                    n += 1;
                    let event = if n.is_multiple_of(2) {
                        // Markdown-shaped prose: a paragraph break every 40 words.
                        let sep = if n.is_multiple_of(80) { "\n\n" } else { " " };
                        AgentEvent::TextDelta(format!("word{n}{sep}"))
                    } else {
                        AgentEvent::ToolOutputDelta {
                            call_id: "s1".into(),
                            chunk: format!("   Compiling crate_{n} v0.1.{n}\n"),
                        }
                    };
                    if !send(event) {
                        return;
                    }
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            automation::note_stream_rate(n as f64 / started.elapsed().as_secs_f64());
            send(AgentEvent::TurnFinished {
                turn_id: 1,
                reason: TurnEndReason::Completed,
            });
        });
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(5600))
                .await;
            this.update(cx, |_, cx| {
                automation::write_frames("stream");
                cx.quit();
            })
            .ok();
        })
        .detach();
    }

    /// Scrolls the (long) active transcript from the top for four seconds,
    /// then writes the frame log and quits.
    pub(crate) fn start_scroll_test(&mut self, cx: &mut Context<Self>) {
        let list = self.sessions[self.active].list.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(800))
                .await;
            list.set_follow_mode(FollowMode::Normal);
            list.scroll_to(ListOffset {
                item_ix: 0,
                offset_in_item: px(0.),
            });
            let tick = Duration::from_millis(8);
            for _ in 0..500 {
                cx.background_executor().timer(tick).await;
                list.scroll_by(px(48.));
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
            this.update(cx, |_, cx| {
                automation::write_frames("scroll");
                cx.quit();
            })
            .ok();
        })
        .detach();
    }
}
