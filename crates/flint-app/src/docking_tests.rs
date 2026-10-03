use pretty_assertions::assert_eq;

use super::{Edge, Layout, Panel};

#[test]
fn every_panel_docks_on_every_edge_without_losing_or_duplicating_contents() {
    for panel in Panel::ALL {
        for target in Panel::ALL {
            for edge in Edge::ALL {
                let mut layout = Layout::default();
                let before = layout.clone();
                assert_eq!(layout.move_panel(panel, target, edge), panel != target);
                assert!(layout.valid());
                if panel == target {
                    assert_eq!(layout, before);
                    continue;
                }
                let order = layout.panels();
                let moving = order.iter().position(|p| *p == panel).unwrap();
                let anchor = order.iter().position(|p| *p == target).unwrap();
                assert_eq!(moving < anchor, matches!(edge, Edge::Left | Edge::Top));
            }
        }
    }
}

#[test]
fn repeated_docking_keeps_a_valid_tree_and_preserves_the_other_panels() {
    let mut layout = Layout::default();
    for _ in 0..20 {
        for (panel, target, edge) in [
            (Panel::Sidebar, Panel::Chat, Edge::Right),
            (Panel::Chat, Panel::Changes, Edge::Top),
            (Panel::Terminal, Panel::Chat, Edge::Left),
            (Panel::Changes, Panel::Sidebar, Edge::Bottom),
        ] {
            assert!(layout.move_panel(panel, target, edge));
            assert!(layout.valid());
        }
    }
}

#[test]
fn saved_layout_round_trips_with_split_sizes_and_hidden_panel_positions() {
    let dir = tempfile::tempdir().unwrap();
    let mut layout = Layout::default();
    layout.move_panel(Panel::Terminal, Panel::Chat, Edge::Left);
    assert!(
        layout
            .root
            .set_sizes("Horizontal(terminal,chat)", [325., 600.])
    );
    layout.save(dir.path()).unwrap();
    assert_eq!(Layout::load(dir.path(), 280.), layout);
}

#[test]
fn invalid_or_incomplete_layouts_fall_back_without_rewriting_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let fallback = Layout::initial(350.);
    assert_eq!(Layout::load(dir.path(), 350.), fallback);
    for invalid in [
        "{",
        r#"{"root":{"type":"panel","panel":"chat"}}"#,
        r#"{"root":{"type":"panel","panel":"unknown"}}"#,
    ] {
        std::fs::write(dir.path().join("layout.json"), invalid).unwrap();
        assert_eq!(Layout::load(dir.path(), 350.), fallback);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("layout.json")).unwrap(),
            invalid
        );
    }
    let mut duplicate = Layout::default();
    duplicate
        .root
        .insert(Panel::Sidebar, Panel::Chat, Edge::Left);
    duplicate.save(dir.path()).unwrap();
    assert_eq!(Layout::load(dir.path(), 350.), fallback);
    let mut invalid_size = Layout::default();
    let key = invalid_size.root.key();
    invalid_size.root.set_sizes(&key, [-1., 0.]);
    invalid_size.save(dir.path()).unwrap();
    assert_eq!(Layout::load(dir.path(), 350.), fallback);
}
