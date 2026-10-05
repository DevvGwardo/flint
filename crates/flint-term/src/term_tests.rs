use std::time::Duration;
use std::time::Instant;

use pretty_assertions::assert_eq;

use super::*;
use crate::palette::Rgb8;
use crate::snapshot::CursorShape;

#[path = "snapshot_reference_tests.rs"]
mod snapshot_reference;

fn size(cols: u16, rows: u16) -> Size {
    Size {
        cols,
        rows,
        cell_width: 8,
        cell_height: 16,
    }
}

#[test]
#[ignore = "manual release-mode performance probe"]
fn terminal_snapshot_perf_probe() {
    for case in ["blank", "ascii", "wide_combining", "alternating_style"] {
        let term = Terminal::detached(size(160, 48));
        let body = match case {
            "blank" => String::new(),
            "ascii" => "x".repeat(160),
            "wide_combining" => "e\u{301}界".repeat(53),
            _ => "\x1b[31mx\x1b[32mx".repeat(80),
        };
        if !body.is_empty() {
            for row in 1..=48 {
                term.feed(format!("\x1b[{row};1H{body}").as_bytes());
            }
        }
        let start = Instant::now();
        for _ in 0..100 {
            std::hint::black_box(std::hint::black_box(&term).snapshot());
        }
        eprintln!(
            "terminal_snapshot_{case}_100_runs_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
    }
}

#[test]
fn snapshots_match_frozen_builder_across_unicode_styles_and_view_changes() {
    let mut comparisons = 0;
    for cols in [10, 40] {
        for rows in [3, 8] {
            for body in [
                "",
                "plain text\r\nnext   ",
                "e\u{301}界🙂\x1b[31mx\x1b[0m\r\n界e\u{301}",
                "\x1b[1;3;4;9;31mstyled\x1b[0m\x1b[2m dim\x1b[7m inverse\x1b[8m hidden",
                "\x1b[48;2;1;2;3m  \x1b[0m\x1b[4m   \x1b[0m ",
                "\x1b[?1h\x1b[?2004h\x1b[?25l\x1b[2;4Hgap",
            ] {
                let mut term = Terminal::detached(size(cols, rows));
                term.feed(body.as_bytes());
                for phase in 0..4 {
                    match phase {
                        1 => {
                            term.start_selection(0, 0, false, 1);
                            term.update_selection(1, 4, true);
                        }
                        2 => {
                            term.feed("scroll 界e\u{301}\r\n".repeat(20).as_bytes());
                            term.scroll(3);
                        }
                        3 => term.resize(size(cols + 7, rows + 2)),
                        _ => {}
                    }
                    let expected = snapshot_reference::build(&term.term.lock(), &term.palette);
                    assert_eq!(
                        term.snapshot(),
                        expected,
                        "cols={cols}, rows={rows}, phase={phase}, body={body:?}"
                    );
                    comparisons += 1;
                }
            }
        }
    }
    assert_eq!(comparisons, 96);
}

#[test]
fn merged_terminal_runs_keep_combining_marks_styles_and_snapshot_ownership() {
    let term = Terminal::detached(size(40, 3));
    term.feed("ae\u{301}界🙂\x1b[31mred\u{301}\x1b[0mend".as_bytes());
    let snapshot = term.snapshot();
    assert_eq!(snapshot.lines[0].len(), 3);
    assert_eq!(
        (
            snapshot.lines[0][0].text.as_str(),
            snapshot.lines[0][0].width
        ),
        ("ae\u{301}界🙂", 6)
    );
    assert_eq!(
        (snapshot.lines[0][1].text.as_str(), snapshot.lines[0][1].col),
        ("red\u{301}", 6)
    );
    let original = snapshot.clone();
    let mut changed = snapshot;
    changed.lines[0][0].text.clear();
    assert_eq!(term.snapshot(), original);
}

#[test]
fn escape_sequences_land_in_the_grid_with_colours() {
    let term = Terminal::detached(size(20, 4));
    term.feed(
        b"plain \x1b[1;31mbold red\x1b[0m\r\n\x1b[38;5;208mx\x1b[48;2;1;2;3my\x1b[0m\x1b[7mz",
    );
    let snap = term.snapshot();
    assert_eq!(snap.text_lines(), vec!["plain bold red", "xyz", "", ""]);
    let red = &snap.lines[0][1];
    assert_eq!(
        (red.text.as_str(), red.bold, red.fg),
        ("bold red", true, Palette::default().ansi[9])
    );
    let orange = &snap.lines[1][0];
    assert_eq!(orange.fg, Palette::default().indexed(208));
    let truecolor = &snap.lines[1][1];
    assert_eq!(truecolor.bg, Some(Rgb8 { r: 1, g: 2, b: 3 }));
    // Inverse swaps foreground and background.
    let inverse = &snap.lines[1][2];
    assert_eq!(
        (inverse.fg, inverse.bg),
        (
            Palette::default().background,
            Some(Palette::default().foreground)
        )
    );
    assert_eq!(
        (snap.cursor.row, snap.cursor.col, snap.cursor.shape),
        (1, 3, CursorShape::Block)
    );
}

#[test]
fn bare_newlines_return_to_the_first_column() {
    let term = Terminal::detached(size(20, 4));
    term.feed(b"cli.rs\nclock.rs\r\ncolor.rs\n");
    let snap = term.snapshot();
    assert_eq!(
        snap.text_lines(),
        vec!["cli.rs", "clock.rs", "color.rs", ""]
    );
    assert_eq!((snap.cursor.row, snap.cursor.col), (3, 0));
}

#[test]
fn wide_characters_take_two_columns() {
    let term = Terminal::detached(size(10, 2));
    term.feed("a界b".as_bytes());
    let snap = term.snapshot();
    let run = &snap.lines[0][0];
    assert_eq!((run.text.as_str(), run.width), ("a界b", 4));
    assert_eq!(snap.cursor.col, 4);
}

#[test]
fn styled_wide_and_combining_runs_use_cell_endpoints() {
    let term = Terminal::detached(size(20, 3));
    term.feed("界\x1b[31mx\x1b[0m\n".as_bytes());
    term.feed("e\u{301}\x1b[32mx\x1b[0m\x1b[2Cy".as_bytes());
    assert_eq!(
        term.snapshot().text_lines(),
        vec!["界x", "e\u{301}x  y", ""]
    );
}

#[test]
fn scrollback_keeps_old_lines_and_scrolls_the_view() {
    let term = Terminal::detached(size(10, 3));
    for i in 0..20 {
        term.feed(format!("line {i}\r\n").as_bytes());
    }
    let snap = term.snapshot();
    assert_eq!(snap.text_lines(), vec!["line 18", "line 19", ""]);
    assert_eq!(snap.history, 18);
    term.scroll(5);
    let scrolled = term.snapshot();
    assert_eq!(
        (scrolled.display_offset, scrolled.text_lines()[0].as_str()),
        (5, "line 13")
    );
    assert_eq!(scrolled.cursor.shape, CursorShape::Hidden);
    term.scroll_to_bottom();
    assert_eq!(term.snapshot().display_offset, 0);
    assert!(term.buffer_text().starts_with("line 0\nline 1"));
}

#[test]
fn resize_reflows_and_keeps_text() {
    let mut term = Terminal::detached(size(10, 3));
    term.feed(b"0123456789abcd");
    assert_eq!(
        term.snapshot().text_lines()[..2],
        ["0123456789".to_string(), "abcd".to_string()]
    );
    term.resize(size(20, 3));
    let snap = term.snapshot();
    assert_eq!(
        (snap.cols, snap.text_lines()[0].as_str()),
        (20, "0123456789abcd")
    );
}

#[test]
fn selection_and_copy() {
    let term = Terminal::detached(size(20, 2));
    term.feed(b"hello world\r\nsecond");
    term.start_selection(0, 6, false, 1);
    term.update_selection(0, 10, true);
    assert_eq!(term.selection_text().as_deref(), Some("world"));
    assert_eq!(term.snapshot().selection.len(), 1);
    term.start_selection(1, 2, false, 2);
    assert_eq!(term.selection_text().as_deref(), Some("second"));
    term.clear_selection();
    assert_eq!(term.selection_text(), None);
}

#[test]
fn modes_reach_the_snapshot() {
    let term = Terminal::detached(size(10, 2));
    term.feed(b"\x1b[?1h\x1b[?2004h\x1b[?25l");
    let snap = term.snapshot();
    assert!(snap.app_cursor && snap.bracketed_paste);
    assert_eq!(snap.cursor.shape, CursorShape::Hidden);
    term.feed(b"\x1b[?25h\x1b[6 q");
    assert_eq!(term.snapshot().cursor.shape, CursorShape::Beam);
}

#[test]
fn wakeups_are_coalesced_until_acknowledged() {
    let term = Terminal::detached(size(10, 2));
    let events = term.events();
    for _ in 0..100 {
        term.feed(b"x");
    }
    assert_eq!(events.try_recv(), Ok(TermEvent::Wakeup));
    assert!(events.try_recv().is_err());
    term.acknowledge_wakeup();
    term.feed(b"y");
    assert_eq!(events.try_recv(), Ok(TermEvent::Wakeup));
}

#[test]
fn title_and_bell_floods_are_bounded_and_keep_the_latest_title_and_exit() {
    let term = Terminal::detached(size(10, 2));
    let events = term.events();
    // Include a control event: coalescing must never discard it.
    term.listener.send_event(Event::ChildExit({
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(3 << 8)
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(3)
        }
    }));
    for i in 0..10_000 {
        term.feed(format!("\x1b]2;title {i}\x07\x07").as_bytes());
    }
    assert!(events.len() <= 4);
    let mut drained = Vec::new();
    while let Ok(event) = events.try_recv() {
        drained.push(event);
    }
    assert!(drained.contains(&TermEvent::Title("title 9999".into())));
    assert!(drained.contains(&TermEvent::Exited(Some(3))));
    assert_eq!(
        drained
            .iter()
            .filter(|e| matches!(e, TermEvent::Bell))
            .count(),
        1
    );
}

#[test]
fn pty_runs_a_command_and_reports_its_exit_code() {
    let term = Terminal::spawn(SpawnConfig {
        cwd: std::env::temp_dir(),
        command: Some((
            "/bin/sh".into(),
            vec!["-c".into(), "printf hi; exit 3".into()],
        )),
        env: Vec::new(),
        size: size(20, 4),
    })
    .expect("spawn");
    let events = term.events();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut exit = None;
    while Instant::now() < deadline && exit.is_none() {
        match events.recv_blocking() {
            Ok(TermEvent::Exited(code)) => exit = Some(code),
            Ok(TermEvent::Wakeup) => term.acknowledge_wakeup(),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert_eq!(exit, Some(Some(3)));
    assert_eq!(term.snapshot().text_lines()[0], "hi");
}

#[test]
fn pty_echoes_typed_input() {
    let term = Terminal::spawn(SpawnConfig {
        cwd: std::env::temp_dir(),
        command: Some(("/bin/sh".into(), Vec::new())),
        env: vec![("PS1".into(), "$ ".into())],
        size: size(40, 6),
    })
    .expect("spawn");
    term.write(b"echo hi-$((1+2))\r".to_vec());
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if term.snapshot().text_lines().iter().any(|l| l == "hi-3") {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("no output: {:?}", term.snapshot().text_lines());
}
