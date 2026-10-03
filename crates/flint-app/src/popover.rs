//! Where a composer popover opens, and how tall it may grow.
//!
//! The popover is drawn with `deferred`, so the chat panel's
//! `overflow_hidden` cannot clip it: if it is taller than the room it has,
//! it spills over the header, a docked neighbour or the window edge. So its
//! height is not a share of the window but the measured room between the
//! composer and the chat panel's edge. Both rectangles are recorded while
//! painting (see [`probe`]) and used by the next render; when either one
//! moves, the probe asks for that next render.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::*;

/// Space between the composer and the popover.
pub const GAP: f32 = 8.;
/// Space kept between the popover and the chat panel's edge.
pub const MARGIN: f32 = 8.;
/// A side with at least this much room is used even when the other is larger.
pub const COMFORTABLE: f32 = 240.;
/// The popover never shrinks below this; a header and a row or two still fit.
pub const MIN_HEIGHT: f32 = 80.;

/// The rectangles a popover is placed against, from the last paint.
#[derive(Default)]
pub struct Room {
    /// The chat panel: the popover must stay inside it.
    pub area: Cell<Option<Bounds<Pixels>>>,
    /// The composer card the popover hangs from.
    pub anchor: Cell<Option<Bounds<Pixels>>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Opens below the composer instead of above it.
    pub below: bool,
    /// The tallest the popover may be, in pixels.
    pub max_height: f32,
}

/// Picks the side and height for a popover hanging from `anchor` inside
/// `area`. The preferred side wins when it has comfortable room; otherwise
/// the side with more room does.
pub fn place(area: Bounds<Pixels>, anchor: Bounds<Pixels>, prefer_below: bool) -> Placement {
    let above = f32::from(anchor.top() - area.top()) - GAP - MARGIN;
    let below = f32::from(area.bottom() - anchor.bottom()) - GAP - MARGIN;
    let (preferred, other) = if prefer_below {
        (below, above)
    } else {
        (above, below)
    };
    let keep = preferred >= COMFORTABLE || preferred >= other;
    let below_side = keep == prefer_below;
    let room = if keep { preferred } else { other };
    Placement {
        below: below_side,
        max_height: room.max(MIN_HEIGHT),
    }
}

impl Room {
    /// The placement for this frame. Before the first paint there is
    /// nothing measured yet, so it falls back to a share of the window.
    pub fn placement(&self, prefer_below: bool, window_height: f32) -> Placement {
        match (self.area.get(), self.anchor.get()) {
            (Some(area), Some(anchor)) => place(area, anchor, prefer_below),
            _ => Placement {
                below: prefer_below,
                max_height: (window_height * 0.32).max(MIN_HEIGHT),
            },
        }
    }

    /// The chat panel's height, once measured.
    pub fn area_height(&self) -> Option<f32> {
        self.area.get().map(|area| f32::from(area.size.height))
    }
}

/// An invisible element filling its (relatively positioned) parent that
/// records the parent's bounds into `slot` while painting, and asks for one
/// more frame when they changed so the next render can use them.
pub fn probe(
    room: &Rc<Room>,
    slot: fn(&Room) -> &Cell<Option<Bounds<Pixels>>>,
) -> impl IntoElement {
    let room = room.clone();
    canvas(
        move |bounds, window, _| {
            let cell = slot(&room);
            if cell.get() != Some(bounds) {
                cell.set(Some(bounds));
                window.refresh();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

#[cfg(test)]
#[path = "popover_tests.rs"]
mod tests;
