use super::*;
use serde_json::json;

fn check(task: &mut TaskRecord, command: &str, success: bool, revision: u64) {
    task.observe_command(
        command,
        command,
        CommandTiming {
            started: revision,
            finished: revision,
            order: task.checks.iter().map(|check| check.id).max().unwrap_or(0) + 1,
        },
        &ToolOutcome {
            output: if success { "ok" } else { "failed" }.into(),
            exit_code: Some(if success { 0 } else { 1 }),
            success,
            diff: None,
        },
        true,
    );
}

#[test]
fn optional_review_actions_are_proposals_not_acceptance_or_real_blocker_resolution() {
    let mut task = TaskRecord::new(1, "Analyze the gaps.".into(), 0);
    task.note_scope_denial("edit_file", &json!({"path":"ANALYSIS.md"}));
    let false_blocker = "The protected task is read-only; edit_file cannot update ANALYSIS.md.";
    let report = |blockers: Vec<&str>| {
        json!({
            "summary":"The code has an accessibility gap.", "deliverables":[], "blockers":blockers,
            "proposed_actions":["Fix the accessibility gap and run tests."]
        })
    };
    assert!(task.report(&report(vec![false_blocker])).is_err());
    assert!(task.report.is_none() && task.blockers.is_empty());
    task.report(&report(vec![])).unwrap();
    assert_eq!(
        task.proposed_actions,
        ["Fix the accessibility gap and run tests."]
    );
    assert_eq!(
        task.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::NeedsReview
    );
    assert!(!task.is_accepted() && !task.verified_for_queue());
    task.add_blocker("Review additional source first.", "user review");
    task.report(&report(vec![])).unwrap();
    assert_eq!(task.blockers.len(), 1);
    task.report(&report(vec![
        false_blocker,
        "The required source is inaccessible.",
    ]))
    .unwrap();
    assert_eq!(
        task.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::Blocked
    );
    let resolution = json!({
        "summary":"Ready.", "deliverables":[], "blockers":[],
        "resolved_blockers":["Review additional source first."]
    });
    assert!(task.report(&resolution).is_err());
}

#[test]
fn scope_wording_alone_cannot_hide_requested_actions_or_actual_blockers() {
    let mut task = TaskRecord::new(1, "Analyze the gaps.".into(), 0);
    let text = "The protected task is read-only; edit_file cannot update ANALYSIS.md.";
    assert!(!task.optional_scope_blocker(text));
    task.note_scope_denial("edit_file", &json!({"path":"ANALYSIS.md"}));
    assert!(task.optional_scope_blocker(text));
    assert!(!task.optional_scope_blocker("The source is inaccessible."));
    let mut explicit = TaskRecord::new(1, "Analyze ANALYSIS.md. Do not change files.".into(), 0);
    explicit.note_scope_denial("edit_file", &json!({"path":"ANALYSIS.md"}));
    assert!(!explicit.optional_scope_blocker(text));
    let mut tests = TaskRecord::new(1, "Analyze test failures without changing files.".into(), 0);
    tests.note_scope_denial("run_command", &json!({"command":"bun test"}));
    assert!(!tests.optional_scope_blocker("read-only mode prevents run_command: bun test"));
    task.contract.revision += 1;
    assert!(
        !task.optional_scope_blocker(text),
        "old scope denials cannot justify new reports"
    );
}

#[test]
fn old_task_records_load_without_retroactive_scope_feedback() {
    let task = TaskRecord::new(1, "Review the code.".into(), 0);
    let mut value = serde_json::to_value(&task).unwrap();
    value.as_object_mut().unwrap().remove("scope_denials");
    value.as_object_mut().unwrap().remove("proposed_actions");
    value.as_object_mut().unwrap().remove("continuation_stop");
    let restored: TaskRecord = serde_json::from_value(value).unwrap();
    restored.validate().unwrap();
    assert!(restored.scope_denials.is_empty() && restored.proposed_actions.is_empty());
    assert!(!restored.is_accepted());
    assert!(restored.continuation_stop.is_none());
}
#[test]
fn verified_queue_readiness_is_current_native_evidence_not_acceptance() {
    let mut task = TaskRecord::new(1, "Fix the parser".into(), 0);
    task.observed_edits = 1;
    check(&mut task, "cargo test", true, 0);
    task.assess(TurnEndReason::Completed, true);
    assert!(task.verified_for_queue());
    assert!(!task.is_accepted());
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::NeedsReview
    );
    for source in [TaskSource::External, TaskSource::Legacy] {
        let mut external = task.clone();
        external.source = source;
        assert!(!external.verified_for_queue());
    }
    let mut stale = task.clone();
    stale.set_work_revision(1);
    assert!(!stale.verified_for_queue());
    for reason in [
        TurnEndReason::Interrupted,
        TurnEndReason::ContinuationLimit,
        TurnEndReason::Failed("fixture".into()),
    ] {
        let mut incomplete = task.clone();
        incomplete.assess(reason, true);
        assert!(!incomplete.verified_for_queue());
    }
    task.add_blocker("Missing dependency", "agent report");
    task.assess(TurnEndReason::Completed, true);
    assert!(!task.verified_for_queue());
}

#[test]
fn successful_child_reports_need_review_but_are_not_failed_dependencies() {
    let mut parent = TaskRecord::new(1, "Implement the parser".into(), 0);
    let mut child = TaskRecord::new(2, "Implement parsing".into(), 0);
    check(&mut child, "cargo test", true, 0);
    let assessment = child.assess(TurnEndReason::Completed, true);
    parent.absorb_child(&child);
    parent.note_child("child-1", &assessment);
    assert_eq!(
        parent.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::NeedsReview
    );
    assert!(parent.verified_for_queue());
    assert!(!parent.is_accepted());
    child.add_blocker("Missing input", "agent report");
    parent.note_child("child-1", &child.assess(TurnEndReason::Completed, true));
    assert_eq!(
        parent.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::Partial
    );
    assert!(!parent.verified_for_queue());
}

#[test]
fn exploratory_failures_do_not_poison_a_verified_result_or_erase_required_failures() {
    let mut task = TaskRecord::new(1, "Fix the parser".into(), 0);
    task.observe_command(
        "inspection",
        "git grep optional_name",
        CommandTiming {
            started: 0,
            finished: 0,
            order: 1,
        },
        &ToolOutcome::error("No matches"),
        false,
    );
    check(&mut task, "cargo test", true, 0);
    assert_eq!(
        task.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::NeedsReview
    );
    assert!(task.verified_for_queue());
    check(&mut task, "cargo check", false, 0);
    assert_eq!(
        task.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::Partial
    );
}

#[test]
fn corrected_python_invocations_need_later_current_checks_and_preserve_required_failures() {
    let bad = r#"python3 -c "assert True\nassert True""#;
    let mut task = TaskRecord::new(1, "Fix the helpers".into(), 0);
    task.observed_edits = 1;
    check(&mut task, bad, false, 0);
    task.checks[0].detail =
        "  File \"<string>\", line 1\nSyntaxError: unexpected character after line continuation character"
            .into();
    assert_eq!(
        task.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::Partial
    );
    check(&mut task, "python3 -m unittest -q", true, 0);
    assert_eq!(
        task.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::NeedsReview
    );
    assert_eq!(task.checks[0].state(1, 0), EvidenceState::Failed);
    assert!(!task.is_accepted());
    let mut guard = crate::harness::guard::TurnGuard::new("Fix the helpers");
    guard.set_task_record(&task);
    assert!(guard.before_finish().is_none());
    assert!(task.verified_for_queue());
    for field in ["request", "objective", "instruction"] {
        let mut user_required = task.clone();
        match field {
            "request" => user_required.contract.original_request.push_str(bad),
            "objective" => user_required.contract.objective.push_str(bad),
            _ => user_required.contract.instructions.push(bad.into()),
        }
        assert_eq!(
            user_required.assess(TurnEndReason::Completed, true).outcome,
            TaskOutcome::Partial
        );
        assert!(!user_required.verified_for_queue());
    }
    let mut stale = task.clone();
    stale.set_work_revision(1);
    assert_eq!(
        stale.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::Partial
    );
    task.contract.requirements.push(Requirement {
        id: 2,
        spec: RequirementSpec::Command(bad.into()),
    });
    assert_eq!(
        task.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::Partial
    );
    assert!(!task.verified_for_queue());
    guard.set_task_record(&task);
    assert!(guard.before_finish().is_some());
}

#[test]
fn live_failing_task_snapshot_recovers_without_erasing_its_failed_attempt() {
    let mut task: TaskRecord = serde_json::from_str(include_str!(
        "../tests/fixtures/recovered-python-check.json"
    ))
    .unwrap();
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::Partial
    );
    assert!(task.has_current_verification());
    assert_eq!(
        task.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::NeedsReview
    );
    assert!(task.checks.iter().any(|check| !check.success));
    assert!(!task.is_accepted());
    let mut guard = crate::harness::guard::TurnGuard::new(&task.contract.original_request);
    guard.set_task_record(&task);
    assert!(guard.before_finish().is_none());
}

#[test]
fn long_inspection_history_preserves_current_checks_without_false_overflow() {
    let mut task = TaskRecord::new(1, "Fix the parser".into(), 0);
    for i in 0..100 {
        task.observe_command(
            &format!("inspection-{i}"),
            &format!("ls folder{i}"),
            CommandTiming {
                started: 0,
                finished: 0,
                order: i + 1,
            },
            &ToolOutcome::ok("listed"),
            false,
        );
    }
    check(&mut task, "cargo test", true, 0);
    assert!(!task.evidence_overflow);
    assert!(task.has_current_verification());
    assert_eq!(task.checks.len(), MAX_CHECKS);
    assert_eq!(
        task.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::NeedsReview
    );
    let mut parent = TaskRecord::new(2, "Use the checked child result".into(), 0);
    parent.absorb_child(&task);
    assert!(!parent.evidence_overflow);
    assert!(parent.has_current_verification());
}

#[test]
fn resolving_own_blockers_requires_explicit_checked_evidence_and_preserves_user_review() {
    let mut task = TaskRecord::new(1, "Fix the parser".into(), 0);
    task.add_blocker("Missing dependency", "agent report");
    task.add_blocker("The user requested more work.", "user review");
    let report = json!({
        "summary": "Dependency installed and checked.",
        "deliverables": [],
        "blockers": [],
        "resolved_blockers": ["Missing dependency"],
    });
    assert!(task.report(&report).is_err());
    assert_eq!(task.blockers.len(), 2);
    check(&mut task, "cargo test", true, 0);
    let original = task.contract.clone();
    task.report(&report).unwrap();
    assert_eq!(task.contract, original);
    assert_eq!(task.blockers.len(), 1);
    assert_eq!(task.blockers[0].source, "user review");
    assert!(!task.is_accepted());
    let protected = json!({
        "summary": "Done", "deliverables": [], "blockers": [],
        "resolved_blockers": ["The user requested more work."],
    });
    assert!(task.report(&protected).is_err());
    assert_eq!(task.blockers.len(), 1);
    assert_eq!(
        task.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::Blocked
    );
}

#[test]
fn informational_blocker_resolution_requires_new_current_inspection_and_exact_text() {
    let mut task = TaskRecord::new(1, "What did I work on recently?".into(), 0);
    let args = json!({"path":"activity.md"});
    task.observe_inspection("read_file", ToolKind::Read, &args);
    task.add_blocker("Activity source not yet found.", "agent report");
    let resolution = json!({
        "summary":"Read the activity source and found recent work.",
        "deliverables":[], "blockers":[],
        "resolved_blockers":["Activity source not yet found."],
    });
    assert!(
        task.report(&resolution).is_err(),
        "old inspection is not recovery evidence"
    );
    task.observe_inspection("read_file", ToolKind::Read, &args);
    task.set_work_revision(1);
    assert!(
        task.report(&resolution).is_err(),
        "stale inspection is not recovery evidence"
    );
    task.observe_inspection("read_file", ToolKind::Read, &args);
    let contract = task.contract.clone();
    task.report(&resolution).unwrap();
    assert!(task.blockers.is_empty());
    assert_eq!(task.contract, contract);
    assert!(!task.has_current_verification() && !task.is_accepted());
    task.validate().unwrap();
}

#[test]
fn inspection_cannot_clear_user_blockers_or_replace_work_verification() {
    for request in [
        "Fix the parser.",
        "Analyze only.\nRequired check: cargo test",
        "Analyze only.\nRequired file: report.md",
    ] {
        let mut task = TaskRecord::new(1, request.into(), 0);
        task.add_blocker("Missing input", "agent report");
        task.observe_inspection("list_dir", ToolKind::Read, &json!({"path":"."}));
        assert!(
            task.report(&json!({
                "summary":"Inspected", "deliverables":[], "blockers":[],
                "resolved_blockers":["Missing input"],
            }))
            .is_err(),
            "{request}"
        );
        assert_eq!(task.blockers.len(), 1);
    }
    let mut task = TaskRecord::new(1, "Analyze the code.".into(), 0);
    task.add_blocker("User requested more work", "user review");
    task.observe_inspection("read_file", ToolKind::Read, &json!({"path":"source.rs"}));
    assert!(
        task.report(&json!({
            "summary":"Inspected", "deliverables":[], "blockers":[],
            "resolved_blockers":["User requested more work"],
        }))
        .is_err()
    );
}

#[test]
fn informational_inspection_cannot_certify_observed_edits() {
    let mut task = TaskRecord::new(1, "What changed?".into(), 0);
    task.add_blocker("Missing input", "agent report");
    task.observed_edits = 1;
    task.observe_inspection("list_dir", ToolKind::Read, &json!({"path":"."}));
    assert!(
        task.report(&json!({
            "summary":"Inspected", "deliverables":[], "blockers":[],
            "resolved_blockers":["Missing input"],
        }))
        .is_err()
    );
    assert!(!task.has_current_verification());
}

#[test]
fn only_known_inspection_calls_count_as_blocker_resolution_evidence() {
    let mut task = TaskRecord::new(1, "What changed?".into(), 0);
    for (name, kind, args) in [
        ("report_task", ToolKind::Other, json!({"summary":"Done"})),
        (
            "run_command",
            ToolKind::Command,
            json!({"command":"cargo test"}),
        ),
        (
            "run_command",
            ToolKind::Command,
            json!({"command":"echo ok > out.txt"}),
        ),
        (
            "run_command",
            ToolKind::Command,
            json!({"command":"python3 -c 'print(1)'"}),
        ),
        ("mcp__server__read", ToolKind::Read, json!({})),
    ] {
        task.observe_inspection(name, kind, &args);
        assert!(task.last_inspection.is_none(), "{name}: {args}");
    }
    task.observe_inspection(
        "run_command",
        ToolKind::Command,
        &json!({"command":"git log -3"}),
    );
    assert_eq!(task.last_inspection.as_ref().unwrap().id, 1);
}

#[test]
fn blocker_inspection_evidence_roundtrips_and_legacy_records_remain_valid() {
    let mut task = TaskRecord::new(1, "What changed?".into(), 0);
    task.observe_inspection(
        "read_file",
        ToolKind::Read,
        &json!({"path":"x".repeat(2_000)}),
    );
    task.add_blocker("Need another source", "agent report");
    let mut saved = serde_json::to_value(&task).unwrap();
    let mut restored: TaskRecord = serde_json::from_value(saved.clone()).unwrap();
    assert_eq!(restored, task);
    restored.validate().unwrap();
    restored.observe_inspection("list_dir", ToolKind::Read, &json!({"path":"."}));
    assert_eq!(restored.last_inspection.as_ref().unwrap().id, 2);
    assert_eq!(restored.blockers[0].inspection_id, 1);
    restored.contract.revision += 1;
    assert!(
        restored
            .report(&json!({
                "summary":"Inspected", "deliverables":[], "blockers":[],
                "resolved_blockers":["Need another source"],
            }))
            .is_err(),
        "old-scope inspection cannot resolve a new-scope blocker"
    );
    saved.as_object_mut().unwrap().remove("last_inspection");
    saved["blockers"][0]
        .as_object_mut()
        .unwrap()
        .remove("inspection_id");
    let legacy: TaskRecord = serde_json::from_value(saved).unwrap();
    legacy.validate().unwrap();
    assert!(legacy.last_inspection.is_none());
    assert_eq!(legacy.blockers[0].inspection_id, 0);
}

fn edit(task: &TaskRecord) -> TaskEdit {
    TaskEdit {
        task_id: task.contract.id,
        expected_revision: task.contract.revision,
        objective: task.contract.objective.clone(),
        mode: task.contract.mode,
        requirements: task
            .contract
            .requirements
            .iter()
            .map(|r| r.spec.clone())
            .collect(),
        restrictions: task.contract.restrictions.clone(),
        clear_reported_blockers: false,
    }
}

#[test]
fn model_report_and_empty_plan_cannot_remove_the_raw_goal_or_accept_it() {
    let mut task = TaskRecord::new(1, "Decompile method bodies, not only metadata.".into(), 0);
    let original = task.contract.clone();
    task.report(&json!({"summary":"Done", "blockers":[], "deliverables":["metadata.json"]}))
        .unwrap();
    task.plan.clear();
    let assessment = task.assess(TurnEndReason::Completed, true);
    assert_eq!(task.contract, original);
    assert_eq!(assessment.outcome, TaskOutcome::NeedsReview);
    assert!(!assessment.outcome.allows_queue());
}

#[test]
fn exact_failed_check_survives_unrelated_success_and_an_edit() {
    let mut task = TaskRecord::new(1, "Fix the parser.\nRequired check: cargo test".into(), 0);
    check(&mut task, "cargo test", false, 0);
    check(&mut task, "cargo --version", true, 0);
    task.set_work_revision(1);
    assert_eq!(
        task.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::Partial
    );
    assert_eq!(task.checks[0].state(1, 1), EvidenceState::Failed);
    check(&mut task, "cargo test", true, 1);
    assert_eq!(task.checks[0].state(1, 1), EvidenceState::Passed);
}

#[test]
fn a_later_edit_or_concurrent_child_edit_invalidates_passing_evidence() {
    let mut task = TaskRecord::new(1, "Fix it".into(), 4);
    check(&mut task, "cargo test", true, 4);
    task.set_work_revision(5);
    assert_eq!(task.checks[0].state(1, 5), EvidenceState::Stale);
    task.observe_command(
        "check",
        "cargo check",
        CommandTiming {
            started: 4,
            finished: 5,
            order: 2,
        },
        &ToolOutcome {
            output: "ok".into(),
            exit_code: Some(0),
            success: true,
            diff: None,
        },
        true,
    );
    assert_eq!(task.checks[1].state(1, 5), EvidenceState::Stale);
}

#[test]
fn metadata_validation_cannot_cover_a_missing_required_file() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("metadata.json"), "{}").unwrap();
    let mut task = TaskRecord::new(
        1,
        "Decompile Gemini.\nRequired file: method-bodies.c".into(),
        0,
    );
    check(&mut task, "python3 validate_metadata.py", true, 0);
    task.refresh_files(root.path());
    let assessment = task.assess(TurnEndReason::Completed, true);
    assert_eq!(assessment.outcome, TaskOutcome::Partial);
    assert_eq!(assessment.requirements[1].state, EvidenceState::Missing);
}

#[test]
fn present_artifact_is_observed_not_semantically_certified() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("bodies.c"), "metadata only").unwrap();
    let mut task = TaskRecord::new(1, "Decompile Gemini.\nRequired file: bodies.c".into(), 0);
    task.refresh_files(root.path());
    let assessment = task.assess(TurnEndReason::Completed, true);
    assert_eq!(assessment.outcome, TaskOutcome::NeedsReview);
    assert_eq!(assessment.requirements[1].state, EvidenceState::Observed);
}

#[test]
fn external_file_changes_are_detected_and_paths_cannot_escape() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let mut task = TaskRecord::new(1, "Produce the file.\nRequired file: out.txt".into(), 0);
    std::fs::write(root.path().join("out.txt"), "first").unwrap();
    assert!(!task.refresh_files(root.path()));
    std::fs::write(root.path().join("out.txt"), "second").unwrap();
    assert!(task.refresh_files(root.path()));
    task.note_file(&outside.path().join("secret").display().to_string(), true);
    task.refresh_files(root.path());
    assert!(matches!(task.files[1].state, FileState::Unknown(_)));
}

#[test]
fn blockers_do_not_disappear_when_a_model_omits_them() {
    let mut task = TaskRecord::new(1, "Recover method bodies".into(), 0);
    task.report(
        &json!({"summary":"Missing input", "blockers":["Binary unavailable"], "deliverables":[]}),
    )
    .unwrap();
    task.report(&json!({"summary":"Done", "blockers":[], "deliverables":[]}))
        .unwrap();
    assert_eq!(
        task.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::Blocked
    );
    assert_eq!(task.blockers.len(), 1);
}

#[test]
fn acceptance_is_explicit_and_stale_reviews_are_rejected() {
    let mut task = TaskRecord::new(1, "Fix it".into(), 0);
    task.assess(TurnEndReason::Completed, true);
    let review = TaskReview {
        turn_id: 1,
        task_id: 1,
        contract_revision: 1,
        work_revision: 0,
        decision: ReviewDecision::Accept,
    };
    task.review(&review).unwrap();
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::Accepted
    );
    task.set_work_revision(1);
    assert!(task.review(&review).is_err());
    assert!(!task.is_accepted());
}

#[test]
fn user_scope_revision_keeps_original_request_and_stable_requirement_ids() {
    let mut task = TaskRecord::new(1, "Original goal".into(), 0);
    let mut revision = edit(&task);
    revision.objective = "Narrower goal".into();
    revision
        .requirements
        .push(RequirementSpec::File("out.txt".into()));
    task.edit(revision.clone()).unwrap();
    assert_eq!(task.contract.original_request, "Original goal");
    assert_eq!(task.contract.requirements[0].id, 1);
    assert_eq!(task.contract.revision, 2);
    assert!(task.edit(revision).is_err());
}

#[test]
fn pending_read_only_policy_survives_scope_resets_until_every_instruction_is_consumed() {
    let clock = WorkClock::default();
    clock.queue_restriction("Research only.");
    clock.queue_restriction("Do not change files.");
    clock.set_read_only(false);
    assert!(clock.read_only());
    clock.consume_restriction("Research only.");
    clock.set_read_only(false);
    assert!(clock.read_only());
    clock.consume_restriction("Do not change files.");
    assert!(clock.read_only());
    clock.set_read_only(false);
    assert!(
        !clock.read_only(),
        "a later explicit work scope can enable writes after consumption"
    );
    clock.consume_restriction("Research only.");
    clock.set_read_only(false);
    assert!(
        !clock.read_only(),
        "direct native inputs must not underflow pending restrictions"
    );
}

#[test]
fn a_check_name_cannot_hide_unknown_effects_in_compound_shell_scripts() {
    for command in [
        "touch result.c; cargo test",
        "cargo test && rm result.c",
        "printf test | sh",
        "cargo check\npython3 mutate.py",
        "cargo test $(touch result.c)",
    ] {
        assert!(
            may_mutate(
                "run_command",
                ToolKind::Command,
                &json!({"command":command})
            ),
            "{command}"
        );
    }
    assert!(!may_mutate(
        "run_command",
        ToolKind::Command,
        &json!({"command":"cargo test"})
    ));
}

#[test]
fn question_can_be_answered_but_work_never_self_accepts() {
    let mut question = TaskRecord::new(1, "How does the parser work?".into(), 0);
    assert_eq!(
        question.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::Answered
    );
    let mut work = TaskRecord::new(1, "Fix the parser".into(), 0);
    assert_eq!(
        work.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::Partial
    );
    work.plan = vec![PlanStep {
        step: "Tests".into(),
        status: crate::tools::StepStatus::Pending,
    }];
    assert_eq!(
        work.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::Partial
    );
}

#[test]
fn mode_inference_honors_read_only_negation_and_ambiguous_requests() {
    for request in [
        "Research the implementation. Do not change files.",
        "Review only, don't implement it.",
        "Research-only: find the bug.",
        "analyze this for improvements and gaps to fix",
        "Please analyze potential fixes and implementation gaps.",
        "Can you review this for bugs to fix?",
        "Explain how to implement the feature.",
        "Find improvements and list the recommended changes.",
        "Review this and make recommendations.",
        "Analyze this. Then make suggestions.",
        "Review this and please make recommendations.",
    ] {
        assert_eq!(TaskMode::infer(request), TaskMode::ReadOnly);
    }
    assert_eq!(TaskMode::infer("Fix the bug"), TaskMode::Work);
    for request in [
        "Review this and fix the bugs",
        "Analyze the failures, then implement the fixes",
        "Analyze the code. Fix the bug.",
        "Review this; please apply the changes",
        "Review the component and make it responsive",
        "Review this and please fix the bugs",
        "Analyze this. Then please implement the fixes.",
        "Review this and could you please apply the fixes?",
    ] {
        assert_eq!(TaskMode::infer(request), TaskMode::Work, "{request}");
    }
    assert_eq!(
        TaskMode::infer("some ambiguous context"),
        TaskMode::Unspecified
    );
}

#[test]
fn read_only_policy_refuses_shell_escapes_and_mcp_even_under_auto_approval() {
    for command in [
        "echo x > out",
        "cat a; rm b",
        "python3 x.py",
        "git -c alias.x='!rm a' x",
        "rg --pre=script x",
        "find . -delete",
        "git -ccore.fsmonitor=script status",
        "git diff --ext-diff",
        "git log --open-files-in-pager=script",
        "git --config-env=core.fsmonitor=SCRIPT status",
    ] {
        assert!(
            !read_only_call_allowed(
                "run_command",
                ToolKind::Command,
                &json!({"command":command})
            ),
            "{command}"
        );
    }
    for command in ["ls -la", "rg pattern src", "git status --short"] {
        assert!(
            read_only_call_allowed(
                "run_command",
                ToolKind::Command,
                &json!({"command":command})
            ),
            "{command}"
        );
    }
    assert!(!read_only_call_allowed(
        "edit_file",
        ToolKind::Edit,
        &json!({})
    ));
    assert!(!read_only_call_allowed(
        "mcp__server__tool",
        ToolKind::Other,
        &json!({})
    ));
}

#[test]
fn decoded_readonly_arguments_allow_quoted_paths_but_not_executable_options() {
    for command in [
        "cat 'two words.txt'",
        "rg 'two words' src",
        "'git' status --short",
    ] {
        assert!(
            read_only_call_allowed(
                "run_command",
                ToolKind::Command,
                &json!({"command":command})
            ),
            "{command}"
        );
    }
    for command in [
        "git diff --'output'=result",
        "git log '--open-files-in-pager=script'",
        "rg '--pre=script' pattern",
        "file '--compile' -m rules",
        "git diff 'unterminated",
    ] {
        assert!(
            !read_only_call_allowed(
                "run_command",
                ToolKind::Command,
                &json!({"command":command})
            ),
            "{command}"
        );
    }
}

#[test]
fn read_only_git_option_values_cannot_pose_as_the_subcommand() {
    for command in [
        "git -C status reset --hard",
        "git -C status commit --allow-empty -m x",
        "git --git-dir diff checkout -- .",
        "git --work-tree log clean -fdx",
        "git --namespace show push",
        "git --attr-source ls-files stash drop",
        "git -C",
        "git --no-pager",
        "git -p log",
        "git --paginate log",
        "git --unknown-global status",
        "git -- status",
    ] {
        assert!(
            !read_only_call_allowed(
                "run_command",
                ToolKind::Command,
                &json!({"command":command})
            ),
            "{command}"
        );
        assert!(
            may_mutate(
                "run_command",
                ToolKind::Command,
                &json!({"command":command})
            ),
            "{command}"
        );
    }
    for command in [
        "git -C sub status --short",
        "git -C 'two words' log --oneline",
        "git --git-dir=.git --work-tree=. diff",
        "git --no-pager -C sub show HEAD",
        "git -P --no-optional-locks ls-files",
    ] {
        assert!(
            read_only_call_allowed(
                "run_command",
                ToolKind::Command,
                &json!({"command":command})
            ),
            "{command}"
        );
    }
}

#[cfg(windows)]
#[test]
fn readonly_windows_shell_refuses_expansion_and_escape_syntax() {
    for command in [
        "git status %INJECT%",
        "git status !INJECT!",
        "git status ^& echo changed",
        "git status\recho changed",
    ] {
        assert!(
            !read_only_call_allowed(
                "run_command",
                ToolKind::Command,
                &json!({"command":command})
            ),
            "{command}"
        );
    }
}

#[test]
fn steering_requirements_keep_stable_ids_and_readonly_policy_at_the_cap() {
    let mut task = TaskRecord::new(1, "Research only.\nRequired file: original".into(), 0);
    let original = task.contract.requirements.clone();
    task.add_instruction(
        "Implement changes.\nRequired file: original\nRequired check: test -s original".into(),
    );
    assert_eq!(task.contract.mode, TaskMode::ReadOnly);
    assert_eq!(
        &task.contract.requirements[..original.len()],
        original.as_slice()
    );
    assert_eq!(task.contract.requirements.len(), 3);
    let next = task.contract.requirements[2].id;
    task.add_instruction(
        (0..MAX_REQUIREMENTS)
            .map(|i| format!("Required file: extra-{i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    assert_eq!(task.contract.requirements.len(), MAX_REQUIREMENTS);
    assert_eq!(task.contract.requirements[2].id, next);
    assert!(task.evidence_overflow);
    assert_eq!(
        task.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::Partial
    );
    task.validate().unwrap();
}

#[test]
fn labelled_requirement_overflow_or_empty_content_remains_explicitly_unknown() {
    let text = format!(
        "Recover actual methods.\n{}",
        (0..40)
            .map(|n| format!("Required file: output-{n}.c"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let mut task = TaskRecord::new(1, text, 0);
    assert_eq!(task.contract.requirements.len(), MAX_REQUIREMENTS);
    assert!(task.evidence_overflow);
    assert!(
        task.assess(TurnEndReason::Completed, true)
            .issues
            .iter()
            .any(|issue| issue.contains("observation limit"))
    );
    assert!(TaskRecord::new(1, "Required file:".into(), 0).evidence_overflow);
}

#[test]
fn formerly_unknown_file_evidence_invalidates_explicit_acceptance_when_it_changes() {
    let root = tempfile::tempdir().unwrap();
    let mut task = TaskRecord::new(1, "Deliver the result.\nRequired file: result".into(), 0);
    std::fs::create_dir(root.path().join("result")).unwrap();
    assert!(!task.refresh_files(root.path()));
    assert!(matches!(task.files[0].state, FileState::Unknown(_)));
    task.assess(TurnEndReason::Completed, true);
    task.review(&TaskReview {
        turn_id: 1,
        task_id: 1,
        contract_revision: 1,
        work_revision: 0,
        decision: ReviewDecision::Accept,
    })
    .unwrap();
    std::fs::remove_dir(root.path().join("result")).unwrap();
    std::fs::write(root.path().join("result"), "now readable").unwrap();
    assert!(task.refresh_files(root.path()));
    assert!(!task.is_accepted());
}

#[test]
fn evidence_caps_fail_closed_instead_of_eviction_of_failed_checks() {
    let mut task = TaskRecord::new(1, "Fix it".into(), 0);
    check(&mut task, "required failing check", false, 0);
    for n in 0..100 {
        check(&mut task, &format!("check {n}"), true, 0);
    }
    assert!(task.evidence_overflow);
    assert_eq!(task.checks.len(), MAX_CHECKS);
    assert!(!task.checks[0].success);
    assert_eq!(
        task.assess(TurnEndReason::Completed, true).outcome,
        TaskOutcome::Partial
    );
}

#[test]
fn task_record_roundtrips_and_rejects_inconsistent_acceptance() {
    let mut task = TaskRecord::new(1, "Fix it".into(), 0);
    task.assess(TurnEndReason::Completed, true);
    let restored: TaskRecord =
        serde_json::from_str(&serde_json::to_string(&task).unwrap()).unwrap();
    assert_eq!(restored, task);
    restored.validate().unwrap();
    task.assessment.as_mut().unwrap().outcome = TaskOutcome::Accepted;
    assert!(task.validate().is_err());
}

#[test]
fn saved_task_validation_rejects_missing_criteria_false_acceptance_and_future_checks() {
    let mut task = TaskRecord::new(1, "Fix it.\nRequired check: cargo test".into(), 4);
    check(&mut task, "cargo test", true, 4);
    task.assess(TurnEndReason::Completed, true);
    task.validate().unwrap();
    let mut damaged = task.clone();
    damaged.assessment.as_mut().unwrap().requirements.pop();
    assert!(damaged.validate().is_err());
    let mut damaged = task.clone();
    damaged.acceptance = Some(Acceptance {
        contract_revision: 1,
        work_revision: 4,
    });
    assert!(damaged.validate().is_err());
    let mut damaged = task.clone();
    damaged.checks[0].work_revision = 5;
    assert!(damaged.validate().is_err());
    let mut damaged = task;
    damaged.checks.push(damaged.checks[0].clone());
    assert!(damaged.validate().is_err());
}

#[test]
fn image_only_and_malformed_command_records_remain_valid_without_invented_scope() {
    let mut task = TaskRecord::new(1, String::new(), 0);
    assert!(task.contract.original_request.is_empty());
    assert_eq!(task.contract.mode, TaskMode::Unspecified);
    check(&mut task, "   ", false, 0);
    assert!(task.checks.is_empty());
    task.assess(TurnEndReason::Completed, true);
    task.validate().unwrap();
}

#[test]
fn verification_words_in_mutating_command_arguments_do_not_hide_effects() {
    for command in [
        "rm tests/regression.rs",
        "touch test-output",
        "git checkout test-branch",
    ] {
        assert!(
            may_mutate(
                "run_command",
                ToolKind::Command,
                &json!({"command":command})
            ),
            "{command}"
        );
    }
    for command in [
        "cargo test",
        "python3 validate_metadata.py",
        "test -s result.c",
    ] {
        assert!(
            !may_mutate(
                "run_command",
                ToolKind::Command,
                &json!({"command":command})
            ),
            "{command}"
        );
    }
}

#[test]
fn read_only_file_inspection_refuses_magic_database_compilation() {
    for command in [
        "file -C -m rules",
        "file --compile -m rules",
        "file -iC -m rules",
    ] {
        assert!(
            !read_only_call_allowed(
                "run_command",
                ToolKind::Command,
                &json!({"command":command})
            ),
            "{command}"
        );
    }
    assert!(read_only_call_allowed(
        "run_command",
        ToolKind::Command,
        &json!({"command":"file result.c"})
    ));
}

#[cfg(unix)]
#[test]
fn fingerprinting_a_fifo_without_a_writer_finishes_as_unknown() {
    use std::os::unix::ffi::OsStrExt as _;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("result");
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: c_path is a valid terminated string; this creates only a
    // private fixture FIFO and never reads or modifies user data.
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    let started = std::time::Instant::now();
    assert!(matches!(
        file_state(root.path(), "result"),
        FileState::Unknown(_)
    ));
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}

#[cfg(unix)]
#[test]
fn native_history_readers_reject_a_fifo_without_blocking_or_moving_it() {
    use std::os::unix::{ffi::OsStrExt as _, fs::FileTypeExt as _};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("history.json");
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: this terminated path is inside the private temporary fixture.
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    let started = std::time::Instant::now();
    assert!(crate::saved_task(root.path()).is_err());
    assert!(crate::persist::load(root.path()).is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    assert!(
        std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_fifo()
    );
    assert!(!root.path().join("history.json.corrupt").exists());
}

/// Moves a file's timestamp into the past so the scan cache may reuse it.
fn settle(path: &Path) -> std::time::SystemTime {
    let settled = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(settled)
        .unwrap();
    settled
}

#[test]
fn fingerprint_cache_reuses_settled_files_but_follows_metadata_moves() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("result");
    std::fs::write(&path, "aaaa").unwrap();
    let settled = settle(&path);
    let mut cache = FileScanCache::default();
    let first = file_state_cached(root.path(), "result", &mut cache);
    let FileState::Present { .. } = first else {
        panic!("expected a present file, got {first:?}");
    };
    // Reuse is keyed on metadata: a same-length rewrite that keeps the
    // timestamp is served from the cache without re-reading the bytes.
    std::fs::write(&path, "bbbb").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(settled)
        .unwrap();
    assert_eq!(file_state_cached(root.path(), "result", &mut cache), first);
    assert_eq!(cache.entries.len(), 1);
    // A scan without the cache reads the bytes, so the reuse above is a cache
    // decision and never a change in what the evidence means.
    assert_ne!(file_state(root.path(), "result"), first);
    // Any metadata move re-reads immediately, even inside the grace window.
    std::fs::write(&path, "cccc-and-longer").unwrap();
    let moved = file_state_cached(root.path(), "result", &mut cache);
    assert!(matches!(moved, FileState::Present { bytes: 15, .. }));
    assert_ne!(moved, first);
    // A recently written file is hashed even when its metadata matches a
    // cached entry, because one timestamp tick can hide a same-size rewrite.
    let recent = root.path().join("recent");
    std::fs::write(&recent, "1234").unwrap();
    let mut warm = FileScanCache::default();
    let hashed = file_state_cached(root.path(), "recent", &mut warm);
    std::fs::write(&recent, "5678").unwrap();
    assert_ne!(file_state_cached(root.path(), "recent", &mut warm), hashed);
}

#[test]
fn fingerprint_cache_forgets_missing_and_oversized_files() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("result");
    std::fs::write(&path, "aaaa").unwrap();
    settle(&path);
    let mut cache = FileScanCache::default();
    assert!(matches!(
        file_state_cached(root.path(), "result", &mut cache),
        FileState::Present { .. }
    ));
    assert_eq!(cache.entries.len(), 1);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        file_state_cached(root.path(), "result", &mut cache),
        FileState::Missing
    );
    assert!(
        cache.entries.is_empty(),
        "a missing file holds no fingerprint"
    );
    // Oversized files are rejected from metadata alone and never cached.
    let big = root.path().join("big");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(MAX_FILE_BYTES + 1)
        .unwrap();
    assert!(matches!(
        file_state_cached(root.path(), "big", &mut cache),
        FileState::Unknown(_)
    ));
    assert!(cache.entries.is_empty());
}

#[test]
fn cached_refresh_matches_a_fresh_refresh_and_still_flags_drift() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("result"), "before").unwrap();
    settle(&root.path().join("result"));
    let mut task = TaskRecord::new(1, "Produce result.\nRequired file: result".into(), 0);
    let mut cache = FileScanCache::default();
    assert!(!task.refresh_files_cached(root.path(), &mut cache));
    let mut fresh = task.clone();
    assert!(!fresh.refresh_files(root.path()));
    assert_eq!(fresh.files, task.files);
    // A second cached scan of unchanged files reports no drift.
    assert!(!task.refresh_files_cached(root.path(), &mut cache));
    // A real change is still detected with a warm cache.
    std::fs::write(root.path().join("result"), "after-and-longer").unwrap();
    assert!(task.refresh_files_cached(root.path(), &mut cache));
    fresh.refresh_files(root.path());
    assert_eq!(fresh.files, task.files);
    assert_eq!(task.files[0].state, fresh.files[0].state);
}

/// `Verified` is the strongest outcome the engine awards automatically, so a
/// child holding it must not reach the parent as failed work. The predicate is
/// shared with `run_child` and the finish-progress counter, which previously
/// listed three of the four usable outcomes and silently dropped `Verified`.
#[test]
fn verified_child_reports_are_usable_work() {
    for outcome in [
        TaskOutcome::Answered,
        TaskOutcome::NeedsReview,
        TaskOutcome::Accepted,
        TaskOutcome::Verified,
    ] {
        assert!(
            outcome.is_usable_child_report(),
            "{} should count as usable child work",
            outcome.label()
        );
    }
    for outcome in [
        TaskOutcome::Partial,
        TaskOutcome::Blocked,
        TaskOutcome::Unknown,
    ] {
        assert!(
            !outcome.is_usable_child_report(),
            "{} must not count as usable child work",
            outcome.label()
        );
    }

    let mut task = TaskRecord::new(1, "Fix the rounding bug.".into(), 0);
    task.note_child(
        "child-1",
        &TaskAssessment {
            task_id: 2,
            contract_revision: 0,
            work_revision: 1,
            outcome: TaskOutcome::Verified,
            reason: TurnEndReason::Completed,
            requirements: Vec::new(),
            issues: Vec::new(),
        },
    );
    let assessment = task.assess(TurnEndReason::Completed, false);
    assert!(
        !assessment
            .issues
            .iter()
            .any(|issue| issue.starts_with("Child ")),
        "a Verified child was reported as a failed dependency: {:?}",
        assessment.issues
    );
}
