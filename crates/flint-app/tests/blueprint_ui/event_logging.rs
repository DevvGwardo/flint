use super::*;

#[gpui_kit::test]
fn unsaved_event_logging_still_folds_the_ordered_engine_stream(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    assert!(ui.read(cx, |app, _| app.session().dir.is_none()));
    for event in [
        AgentEvent::TurnStarted { turn_id: 1 },
        AgentEvent::TextDelta("first ".into()),
        AgentEvent::TextDelta("second 界🙂".into()),
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    ] {
        engine.send(cx, event);
    }
    ui.read(cx, |app, _| {
        assert!(app.session().dir.is_none());
        assert!(!app.session().view.running);
        assert!(matches!(
            app.session().view.items.as_slice(),
            [Item::Assistant { text, streaming: false }, Item::TurnSummary { .. }]
            if text == "first second 界🙂"
        ));
    });
    ui.app.update(cx, |app, _| {
        app.sessions[0].flush_records().unwrap();
    });
}
