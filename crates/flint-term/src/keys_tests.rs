use pretty_assertions::assert_eq;

use super::*;

fn key(name: &str, mods: Mods, app: bool) -> Vec<u8> {
    encode(name, mods, app).expect("bytes")
}

const NONE: Mods = Mods {
    shift: false,
    ctrl: false,
    alt: false,
};
const CTRL: Mods = Mods {
    shift: false,
    ctrl: true,
    alt: false,
};
const ALT: Mods = Mods {
    shift: false,
    ctrl: false,
    alt: true,
};
const SHIFT: Mods = Mods {
    shift: true,
    ctrl: false,
    alt: false,
};

#[test]
fn arrows_follow_application_cursor_mode() {
    assert_eq!(key("up", NONE, false), b"\x1b[A");
    assert_eq!(key("up", NONE, true), b"\x1bOA");
    assert_eq!(key("left", CTRL, true), b"\x1b[1;5D");
    assert_eq!(key("right", ALT, false), b"\x1b[1;3C");
    assert_eq!(key("home", NONE, false), b"\x1b[H");
    assert_eq!(key("end", SHIFT, false), b"\x1b[1;2F");
}

#[test]
fn control_and_alt_combinations() {
    assert_eq!(key("c", CTRL, false), vec![0x03]);
    assert_eq!(key("d", CTRL, false), vec![0x04]);
    assert_eq!(key("[", CTRL, false), vec![0x1b]);
    assert_eq!(key("space", CTRL, false), vec![0]);
    assert_eq!(key("b", ALT, false), b"\x1bb");
    assert_eq!(key("enter", NONE, false), b"\r");
    assert_eq!(key("backspace", NONE, false), vec![0x7f]);
    assert_eq!(key("backspace", ALT, false), vec![0x1b, 0x7f]);
    assert_eq!(key("tab", SHIFT, false), b"\x1b[Z");
    assert_eq!(key("é", NONE, false), "é".as_bytes());
}

#[test]
fn function_and_editing_keys() {
    assert_eq!(key("f1", NONE, false), b"\x1bOP");
    assert_eq!(key("f4", CTRL, false), b"\x1b[1;5S");
    assert_eq!(key("f5", NONE, false), b"\x1b[15~");
    assert_eq!(key("f12", SHIFT, false), b"\x1b[24;2~");
    assert_eq!(key("delete", NONE, false), b"\x1b[3~");
    assert_eq!(key("pageup", NONE, false), b"\x1b[5~");
    assert_eq!(encode("shift", NONE, false), None);
}

#[test]
fn paste_is_bracketed_and_cannot_escape() {
    assert_eq!(paste("a\nb", false), b"a\rb");
    assert_eq!(paste("x\x1b[201~y", true), b"\x1b[200~xy\x1b[201~");
}

#[test]
fn nested_paste_marker_cannot_create_a_close_before_appended_command() {
    let bytes = paste("é\n\x1b[20\x1b[201~1~\necho injected\n", true);
    assert!(bytes.starts_with(b"\x1b[200~"));
    assert!(bytes.ends_with(b"\x1b[201~"));
    let payload = &bytes[6..bytes.len() - 6];
    assert!(
        !payload.contains(&0x1b),
        "no escape may survive inside a paste"
    );
    assert!(std::str::from_utf8(payload).unwrap().starts_with("é\r"));
    assert!(
        std::str::from_utf8(payload)
            .unwrap()
            .ends_with("\recho injected\r")
    );
    assert_eq!(
        paste("é\n界\r\n", true),
        "\x1b[200~é\r界\r\x1b[201~".as_bytes()
    );
}
