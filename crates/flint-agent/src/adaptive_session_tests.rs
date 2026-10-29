use super::*;
use pretty_assertions::assert_eq;

fn saved_session() -> (tempfile::TempDir, Session) {
    let (root, workspace) = temp_workspace();
    let mut config = test_config("http://127.0.0.1:9/v1".into(), workspace.clone());
    config.session_dir = Some(root.path().join("history"));
    let (events, _) = async_channel::unbounded();
    let session = Session::new(
        config,
        events,
        Arc::new(Approvals::default()),
        Arc::new(Mutex::new(None)),
        tools::ToolContext::new(workspace, false),
        None,
        false,
    );
    (root, session)
}

fn unverified_task() -> TaskRecord {
    let mut task = TaskRecord::new(7, "Implement the requested change".into(), 4);
    task.observed_edits = 1;
    task.assessment = Some(TaskAssessment {
        task_id: task.contract.id,
        contract_revision: task.contract.revision,
        work_revision: task.work_revision,
        outcome: TaskOutcome::Partial,
        reason: TurnEndReason::Completed,
        requirements: task
            .contract
            .requirements
            .iter()
            .map(|requirement| crate::RequirementAssessment {
                requirement_id: requirement.id,
                state: EvidenceState::NeedsReview,
                detail: "Native assessment".into(),
                evidence_id: None,
            })
            .collect(),
        issues: vec!["No current verification was observed after the file changes.".into()],
    });
    task
}

#[tokio::test]
async fn adaptive_session_applies_fixed_hints_only_on_a_later_boundary_and_rolls_back() {
    let (_root, mut session) = saved_session();
    session.initialize_improvements(true);
    assert!(session.improvements.is_some());
    session.turn_id = 1;
    session.task = Some(unverified_task());
    let cancel = CancellationToken::new();
    session.prepare_improvement_boundary(&cancel);
    session.learn_completed_task(&TurnEndReason::Completed);
    assert!(session.improvement_context().is_empty());
    let state = session.improvements.as_ref().unwrap().inspect().unwrap();
    assert_eq!(state.candidates.len(), 1);
    assert_eq!(
        state.candidates[0].status,
        crate::improvement::CandidateStatus::Pending
    );
    session.turn_id = 2;
    session.task.as_mut().unwrap().assessment = None;
    session.prepare_improvement_boundary(&cancel);
    let fixed = crate::improvement::HintKind::DirectVerificationReminder.prompt_text();
    assert!(session.improvement_context().contains(fixed));
    session.update_task_context();
    assert!(session.history.iter().any(|message| matches!(
        message, Message::TaskContext(text) if text.contains(fixed)
    )));
    session.set_improvement_policy("rollback");
    assert!(session.improvement_context().is_empty());
    assert!(
        !session
            .improvements
            .as_ref()
            .unwrap()
            .inspect()
            .unwrap()
            .enabled
    );
    session.set_improvement_policy("enabled");
    session.turn_id = 3;
    session.prepare_improvement_boundary(&cancel);
    assert!(
        session.improvement_context().is_empty(),
        "Retired evidence cannot resurrect a hint."
    );
}

#[tokio::test]
async fn adaptive_hints_do_not_cross_scope_read_only_child_or_cancelled_boundaries() {
    let (_root, mut session) = saved_session();
    session.initialize_improvements(true);
    session.task = Some(unverified_task());
    session.turn_id = 1;
    let cancel = CancellationToken::new();
    session.prepare_improvement_boundary(&cancel);
    session.learn_completed_task(&TurnEndReason::Completed);
    session.turn_id = 2;
    session.prepare_improvement_boundary(&cancel);
    assert!(!session.improvement_context().is_empty());
    session.task.as_mut().unwrap().contract.revision += 1;
    assert!(session.improvement_context().is_empty());
    session.task.as_mut().unwrap().contract.revision -= 1;
    session.task.as_mut().unwrap().contract.mode = TaskMode::ReadOnly;
    assert!(session.improvement_context().is_empty());
    session.task.as_mut().unwrap().contract.mode = TaskMode::Work;
    session.child = true;
    assert!(session.improvement_context().is_empty());
    session.child = false;
    let before = session
        .improvements
        .as_ref()
        .unwrap()
        .inspect()
        .unwrap()
        .revision;
    cancel.cancel();
    session.turn_id = 3;
    session.prepare_improvement_boundary(&cancel);
    assert!(session.improvement_context().is_empty());
    assert_eq!(
        session
            .improvements
            .as_ref()
            .unwrap()
            .inspect()
            .unwrap()
            .revision,
        before
    );
}

#[tokio::test]
async fn adaptive_learning_is_capability_gated_and_interruption_is_not_training_evidence() {
    let (root, mut session) = saved_session();
    session.initialize_improvements(false);
    assert!(session.improvements.is_none());
    assert!(!root.path().join("history/improvement").exists());
    session.initialize_improvements(true);
    session.task = Some(unverified_task());
    session.turn_id = 1;
    session.prepare_improvement_boundary(&CancellationToken::new());
    session.learn_completed_task(&TurnEndReason::Interrupted);
    assert!(
        session
            .improvements
            .as_ref()
            .unwrap()
            .inspect()
            .unwrap()
            .candidates
            .is_empty()
    );
    session.set_improvement_policy("arbitrary model text");
    assert!(
        session
            .improvements
            .as_ref()
            .unwrap()
            .inspect()
            .unwrap()
            .enabled
    );
}
