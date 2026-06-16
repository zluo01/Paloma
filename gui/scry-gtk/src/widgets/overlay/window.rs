//! Overlay window placement: where the three layer windows sit, how a
//! drag moves them, which monitor they follow, and chat auto-scroll. The
//! windows are anchored top-left and positioned via pixel margins.

use std::{cell::Cell, rc::Rc};

use gtk4::{ApplicationWindow, GestureDrag, prelude::*};
use gtk4_layer_shell::{Edge, LayerShell};

use super::{Mode, OVERLAY_WIDTH_PX, Overlay, PANEL_GAP_PX, SEARCH_BAR_HEIGHT_PX};

impl Overlay {
    /// Reposition all three windows from the current bar position.
    pub(super) fn layout(&self) {
        if let Some((x, y)) = self.position.get() {
            self.layout_at(x, y);
        }
    }

    /// Bar at `(x, y)`, content directly below, sessions to the right and
    /// vertically centered on the bar.
    pub(super) fn layout_at(&self, x: i32, y: i32) {
        set_position(&self.bar_window, x, y);
        set_position(
            &self.content_window,
            x,
            y + SEARCH_BAR_HEIGHT_PX + PANEL_GAP_PX,
        );
        let sessions_y = (y + SEARCH_BAR_HEIGHT_PX / 2 - self.sessions.height() / 2).max(0);
        set_position(
            self.sessions.window(),
            x + OVERLAY_WIDTH_PX + PANEL_GAP_PX,
            sessions_y,
        );
    }

    /// Chat auto-scroll: stick to the bottom until the user scrolls up.
    pub(super) fn install_scroll_stickiness(&self) {
        const STICK_EPSILON_PX: f64 = 2.0;
        let vadj = self.scroller.vadjustment();

        let mode = self.mode.clone();
        let stuck = self.stuck_to_bottom.clone();
        let last_value = Rc::new(Cell::new(0.0_f64));
        vadj.connect_value_changed(move |adj| {
            let value = adj.value();
            let previous = last_value.replace(value);
            if mode.get() != Mode::Chat {
                return;
            }
            if value + adj.page_size() >= adj.upper() - STICK_EPSILON_PX {
                stuck.set(true);
            } else if value < previous {
                stuck.set(false);
            }
        });

        // Re-pin to the bottom once layout grows the content. The write is
        // deferred to an idle so it doesn't reenter the signal mid-allocation.
        let overlay = self.clone();
        vadj.connect_changed(move |adj| {
            if !overlay.is_stuck_below_bottom(adj) {
                return;
            }
            let overlay = overlay.clone();
            gtk4::glib::idle_add_local_once(move || {
                let vadj = overlay.scroller.vadjustment();
                if overlay.is_stuck_below_bottom(&vadj) {
                    vadj.set_value((vadj.upper() - vadj.page_size()).max(0.0));
                }
            });
        });
    }

    /// In chat mode, pinned, and not already at the bottom.
    fn is_stuck_below_bottom(&self, adj: &gtk4::Adjustment) -> bool {
        self.mode.get() == Mode::Chat
            && self.stuck_to_bottom.get()
            && adj.value() < (adj.upper() - adj.page_size()).max(0.0)
    }

    /// Follow the monitor the compositor maps the bar onto: a position from
    /// another output is meaningless here, so recenter and pin the
    /// satellites to it.
    pub(super) fn install_monitor_watcher(&self) {
        let overlay = self.clone();
        self.bar_window.connect_realize(move |window| {
            let Some(surface) = window.surface() else {
                return;
            };
            let overlay = overlay.clone();
            surface.connect_enter_monitor(move |_, monitor| {
                if overlay.monitor.borrow().as_ref() == Some(monitor) {
                    return;
                }
                *overlay.monitor.borrow_mut() = Some(monitor.clone());
                overlay.content_window.set_monitor(Some(monitor));
                overlay.sessions.window().set_monitor(Some(monitor));
                let panel = (monitor.geometry().height() as f64 * GOLDEN_SECTION_FROM_TOP) as i32;
                overlay.sessions.set_height(panel);
                overlay.position.set(Some(centered_on(monitor)));
                overlay.layout();
            });
        });
    }

    /// Dragging the bar moves all three windows, anchored at drag start.
    pub(super) fn install_bar_drag(&self) {
        let drag = GestureDrag::new();
        let drag_start = Rc::new(Cell::new((0, 0)));

        {
            let overlay = self.clone();
            let drag_start = drag_start.clone();
            drag.connect_drag_begin(move |_, _, _| {
                let position = overlay
                    .position
                    .get()
                    .unwrap_or_else(|| centered_position(&overlay.bar_window));
                drag_start.set(position);
            });
        }

        {
            let overlay = self.clone();
            drag.connect_drag_update(move |_, dx, dy| {
                let (start_x, start_y) = drag_start.get();
                let x = (start_x + dx.round() as i32).max(0);
                let y = (start_y + dy.round() as i32).max(0);
                overlay.position.set(Some((x, y)));
                overlay.layout();
            });
        }

        self.bar.add_controller(drag);
    }
}

fn set_position(window: &ApplicationWindow, x: i32, y: i32) {
    window.set_margin(Edge::Left, x);
    window.set_margin(Edge::Top, y);
}

/// Provisional centered position for the first frame, before the
/// compositor has placed the surface; guesses from the best monitor GDK
/// can name (falling back to 1920×1080). [`centered_on`] finalizes it.
pub(super) fn centered_position(window: &ApplicationWindow) -> (i32, i32) {
    let geometry =
        monitor_geometry(window).unwrap_or_else(|| gtk4::gdk::Rectangle::new(0, 0, 1920, 1080));
    centered_in(&geometry)
}

/// The bar centered on a known monitor.
pub(super) fn centered_on(monitor: &gtk4::gdk::Monitor) -> (i32, i32) {
    centered_in(&monitor.geometry())
}

/// The bar's center sits at the golden section: 61.8% up from the bottom.
const GOLDEN_SECTION_FROM_TOP: f64 = 1.0 - 0.618;

fn centered_in(geometry: &gtk4::gdk::Rectangle) -> (i32, i32) {
    centered_coords(geometry.width(), geometry.height())
}

fn centered_coords(width: i32, height: i32) -> (i32, i32) {
    let x = ((width - OVERLAY_WIDTH_PX) / 2).max(0);
    let y = ((height as f64 * GOLDEN_SECTION_FROM_TOP) as i32 - SEARCH_BAR_HEIGHT_PX / 2).max(0);
    (x, y)
}

/// The monitor backing the surface: explicit window monitor → surface
/// monitor → largest connected monitor (the fallback matters on first
/// show, before the compositor assigned one).
fn monitor_geometry(window: &ApplicationWindow) -> Option<gtk4::gdk::Rectangle> {
    window
        .monitor()
        .or_else(|| surface_monitor(window))
        .or_else(|| largest_monitor(&gtk4::prelude::WidgetExt::display(window)))
        .map(|monitor| monitor.geometry())
}

fn surface_monitor(window: &ApplicationWindow) -> Option<gtk4::gdk::Monitor> {
    let display = gtk4::prelude::WidgetExt::display(window);
    window
        .surface()
        .and_then(|surface| display.monitor_at_surface(&surface))
}

fn largest_monitor(display: &gtk4::gdk::Display) -> Option<gtk4::gdk::Monitor> {
    let monitors = display.monitors();
    let mut best: Option<(i32, gtk4::gdk::Monitor)> = None;

    for i in 0..monitors.n_items() {
        let Some(monitor) = monitors
            .item(i)
            .and_then(|item| item.downcast::<gtk4::gdk::Monitor>().ok())
        else {
            continue;
        };
        let geometry = monitor.geometry();
        let area = geometry.width() * geometry.height();
        if best.as_ref().is_none_or(|(best_area, _)| area > *best_area) {
            best = Some((area, monitor));
        }
    }

    best.map(|(_, monitor)| monitor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_horizontally() {
        let (x, _) = centered_coords(1920, 1080);
        assert_eq!(x, (1920 - OVERLAY_WIDTH_PX) / 2);
    }

    #[test]
    fn places_vertically_at_golden_section() {
        let (_, y) = centered_coords(1920, 1080);
        assert_eq!(
            y,
            (1080.0 * GOLDEN_SECTION_FROM_TOP) as i32 - SEARCH_BAR_HEIGHT_PX / 2
        );
    }

    #[test]
    fn clamps_to_zero_on_tiny_screens() {
        assert_eq!(centered_coords(100, 10), (0, 0));
    }
}
