use gpui_kit::{Bounds, point, px, size};
use pretty_assertions::assert_eq;

use super::GAP;
use super::MARGIN;
use super::MIN_HEIGHT;
use super::Placement;
use super::Room;
use super::place;

/// A full-width rectangle from `top` to `bottom`.
fn span(top: f32, bottom: f32) -> Bounds<gpui_kit::Pixels> {
    Bounds::new(point(px(0.), px(top)), size(px(800.), px(bottom - top)))
}

#[test]
fn preferred_side_with_comfortable_room_wins() {
    // 400 above, 300 below: below is preferred and comfortable.
    let placed = place(span(0., 900.), span(416., 584.), true);
    assert_eq!(
        placed,
        Placement {
            below: true,
            max_height: 900. - 584. - GAP - MARGIN,
        }
    );
}

#[test]
fn cramped_preferred_side_gives_way_to_the_roomier_one() {
    // The welcome composer low in a short panel: 100 below, 500 above.
    let placed = place(span(0., 800.), span(516., 700.), true);
    assert_eq!(
        placed,
        Placement {
            below: false,
            max_height: 516. - GAP - MARGIN,
        }
    );
}

#[test]
fn height_is_the_room_not_a_share_of_the_window() {
    // A conversation composer at the bottom of a short docked panel.
    let placed = place(span(36., 360.), span(200., 340.), false);
    assert!(!placed.below);
    assert_eq!(placed.max_height, 200. - 36. - GAP - MARGIN);
}

#[test]
fn never_shrinks_below_the_minimum() {
    let placed = place(span(0., 200.), span(10., 190.), false);
    assert_eq!(placed.max_height, MIN_HEIGHT);
}

#[test]
fn falls_back_to_a_share_of_the_window_before_the_first_paint() {
    let room = Room::default();
    assert_eq!(
        room.placement(true, 1000.),
        Placement {
            below: true,
            max_height: 320.,
        }
    );
}
