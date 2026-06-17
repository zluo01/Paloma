//! Sessions popup for the overlay.
//!
//! The popup lives in a non-focusable layer window, so keyboard navigation is
//! driven by the bar's key handler instead of `ListBox` focus. The active
//! session row is disabled to prevent restoring a session onto itself.

use std::rc::Rc;

use gtk4::{Align, Button, ListBoxRow, ScrolledWindow, glib, prelude::*, subclass::prelude::*};
use libadwaita::{ActionRow, prelude::*};
use uuid::Uuid;

use super::{OVERLAY_CONTENT_HEIGHT_PX, SESSIONS_WIDTH_PX};

pub(super) const CSS: &str = include_str!("style.css");

mod imp {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    use gtk4::{
        Box as GtkBox, CompositeTemplate, Label, ListBox, glib, prelude::*, subclass::prelude::*,
    };
    use libadwaita::ActionRow;
    use uuid::Uuid;

    /// Typed callback for row activation/deletion.
    pub type SessionCallback = Rc<dyn Fn(Uuid)>;

    #[derive(CompositeTemplate, Default)]
    #[template(file = "sessions.ui")]
    pub struct SessionsView {
        #[template_child]
        pub list: TemplateChild<ListBox>,
        pub rows: RefCell<Vec<(Uuid, ActionRow)>>,
        pub active: Cell<Option<Uuid>>,
        pub height: Cell<i32>,
        /// Row-widget activation / delete callbacks. The keyboard path returns
        /// ids directly via `activate_selected` / `selected_session`, so it
        /// doesn't use these.
        pub on_activated: RefCell<Option<SessionCallback>>,
        pub on_deleted: RefCell<Option<SessionCallback>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SessionsView {
        const NAME: &'static str = "ScrySessions";
        type Type = super::SessionsView;
        type ParentType = GtkBox;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SessionsView {
        fn constructed(&self) {
            self.parent_constructed();
            let empty = Label::new(Some("No sessions yet"));
            empty.add_css_class("scry-sessions-empty");
            self.list.set_placeholder(Some(&empty));
        }
    }

    impl WidgetImpl for SessionsView {}
    impl BoxImpl for SessionsView {}
}

glib::wrapper! {
    pub struct SessionsView(ObjectSubclass<imp::SessionsView>)
        @extends gtk4::Box, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget, gtk4::Orientable;
}

impl SessionsView {
    pub(super) fn new() -> Self {
        let view: Self = glib::Object::new();
        view.set_width_request(SESSIONS_WIDTH_PX);
        view.set_height_request(OVERLAY_CONTENT_HEIGHT_PX);
        view.imp().height.set(OVERLAY_CONTENT_HEIGHT_PX);
        view
    }

    /// Mirror the layer-window height so the popup can be centered beside the bar.
    pub(super) fn set_height(&self, px: i32) {
        self.set_height_request(px);
        self.imp().height.set(px);
    }

    pub(super) fn height(&self) -> i32 {
        self.imp().height.get()
    }

    pub(super) fn clear_selection(&self) {
        self.imp().list.select_row(None::<&ListBoxRow>);
    }

    pub(super) fn set_on_session_activated(&self, f: impl Fn(Uuid) + 'static) {
        *self.imp().on_activated.borrow_mut() = Some(Rc::new(f));
    }

    pub(super) fn set_on_session_deleted(&self, f: impl Fn(Uuid) + 'static) {
        *self.imp().on_deleted.borrow_mut() = Some(Rc::new(f));
    }

    /// `select_first` is true while the popup is open, so keyboard navigation
    /// keeps an anchor after the list is rebuilt.
    pub(super) fn set_sessions(&self, sessions: &[(Uuid, String, String)], select_first: bool) {
        let list = &self.imp().list;
        while let Some(row) = list.first_child() {
            list.remove(&row);
        }
        self.imp().rows.borrow_mut().clear();

        let mut new_rows: Vec<(Uuid, ActionRow)> = Vec::with_capacity(sessions.len());
        for (id, provider, title) in sessions {
            let display_title: String = if title.is_empty() {
                id.to_string().chars().take(8).collect()
            } else {
                title.clone()
            };

            let row = ActionRow::builder()
                .title(&display_title)
                .subtitle(provider)
                .title_lines(1)
                .subtitle_lines(1)
                .activatable(true)
                .tooltip_text(&display_title)
                .build();

            let id_copy = *id;

            row.connect_activated(glib::clone!(
                #[weak(rename_to = view)]
                self,
                move |_| {
                    // The row should already be insensitive; keep this defensive
                    // for programmatic activation.
                    if view.imp().active.get() == Some(id_copy) {
                        return;
                    }
                    // Clone the callback out before invoking so arbitrary
                    // controller code never runs while the `RefCell` is borrowed.
                    let cb = view.imp().on_activated.borrow().clone();
                    if let Some(cb) = cb {
                        cb(id_copy);
                    }
                }
            ));

            let delete = Button::builder()
                .icon_name("user-trash-symbolic")
                .tooltip_text("Delete session")
                .valign(Align::Center)
                .css_classes(["flat", "circular"])
                .build();
            delete.connect_clicked(glib::clone!(
                #[weak(rename_to = view)]
                self,
                move |_| {
                    let cb = view.imp().on_deleted.borrow().clone();
                    if let Some(cb) = cb {
                        cb(id_copy);
                    }
                }
            ));
            row.add_suffix(&delete);

            list.append(&row);
            new_rows.push((*id, row));
        }
        *self.imp().rows.borrow_mut() = new_rows;
        self.sync_row_states();

        if select_first {
            self.select_first_enabled();
        }
    }

    /// Remove a single session's row in place, preserving scroll position and
    /// the rest of the list (unlike a full `set_sessions` rebuild).
    pub(super) fn remove_session(&self, id: Uuid) {
        let mut rows = self.imp().rows.borrow_mut();
        if let Some(pos) = rows.iter().position(|(rid, _)| *rid == id) {
            let (_, row) = rows.remove(pos);
            self.imp().list.remove(&row);
        }
    }

    pub(super) fn set_active(&self, session_id: Option<Uuid>) {
        self.imp().active.set(session_id);
        self.sync_row_states();
    }

    pub(super) fn navigate(&self, delta: i32) {
        let rows = self.imp().rows.borrow();
        if rows.is_empty() {
            return;
        }
        let list = &self.imp().list;
        let active = self.imp().active.get();
        // If no row is selected, anchor navigation at the active row so Up and
        // Down move to its neighboring sessions.
        let current = list
            .selected_row()
            .map(|row| row.index() as usize)
            .or_else(|| rows.iter().position(|(id, _)| Some(*id) == active));
        let Some(next) =
            next_enabled_index(rows.len(), current, delta, |i| Some(rows[i].0) == active)
        else {
            return;
        };
        if current == Some(next) {
            return;
        }
        if let Some((_, row)) = rows.get(next) {
            list.select_row(Some(row));
            bring_into_view(row);
        }
    }

    pub(super) fn activate_selected(&self) -> Option<Uuid> {
        let selected = self.imp().list.selected_row()?;
        let index = selected.index();
        if index < 0 {
            return None;
        }
        let id = match self.imp().rows.borrow().get(index as usize) {
            Some((id, _)) => *id,
            None => return None,
        };
        if self.imp().active.get() == Some(id) {
            return None;
        }
        Some(id)
    }

    pub(super) fn selected_session(&self) -> Option<Uuid> {
        let index = self.imp().list.selected_row()?.index();
        if index < 0 {
            return None;
        }
        self.imp()
            .rows
            .borrow()
            .get(index as usize)
            .map(|(id, _)| *id)
    }

    pub(super) fn scroll_to_active(&self) {
        let active = self.imp().active.get();
        let row = self
            .imp()
            .rows
            .borrow()
            .iter()
            .find(|(id, _)| Some(*id) == active)
            .map(|(_, row)| row.clone());
        if let Some(row) = row {
            center_when_allocated(&row);
        }
    }

    fn sync_row_states(&self) {
        let active = self.imp().active.get();
        let list = &self.imp().list;
        let rows = self.imp().rows.borrow();

        for (id, row) in rows.iter() {
            let is_active = active == Some(*id);
            row.set_sensitive(!is_active);
            if is_active
                && list.selected_row().map(|selected| selected.index()) == Some(row.index())
            {
                list.select_row(None::<&ListBoxRow>);
            }
        }
    }

    fn select_first_enabled(&self) {
        let active = self.imp().active.get();
        let rows = self.imp().rows.borrow();
        if let Some((_, row)) = rows.iter().find(|(id, _)| Some(*id) != active) {
            self.imp().list.select_row(Some(row));
        }
    }
}

/// Scroll `row` to the vertical center after the popup has laid out.
fn center_when_allocated(row: &ActionRow) {
    row.add_tick_callback(|row, _clock| {
        // Bounds are unavailable until the row has been allocated.
        let bounds = row.parent().and_then(|list| row.compute_bounds(&list));
        let Some(bounds) = bounds.filter(|b| b.height() > 0.0) else {
            return glib::ControlFlow::Continue;
        };
        if let Some(scroller) = row
            .ancestor(ScrolledWindow::static_type())
            .and_downcast::<ScrolledWindow>()
        {
            let vadj = scroller.vadjustment();
            let center = (bounds.y() + bounds.height() / 2.0) as f64;
            let max = (vadj.upper() - vadj.page_size()).max(0.0);
            vadj.set_value((center - vadj.page_size() / 2.0).clamp(0.0, max));
        }
        glib::ControlFlow::Break
    });
}

/// Scroll only enough to keep keyboard navigation visible.
fn bring_into_view(row: &ActionRow) {
    let Some(bounds) = row.parent().and_then(|list| row.compute_bounds(&list)) else {
        return;
    };
    let Some(scroller) = row
        .ancestor(ScrolledWindow::static_type())
        .and_downcast::<ScrolledWindow>()
    else {
        return;
    };
    let vadj = scroller.vadjustment();
    let top = bounds.y() as f64;
    let bottom = top + bounds.height() as f64;
    if top < vadj.value() {
        vadj.set_value(top);
    } else if bottom > vadj.value() + vadj.page_size() {
        vadj.set_value(bottom - vadj.page_size());
    }
}

/// Next selectable index in `delta` direction, clamped to the ends and
/// skipping rows where `is_active` holds.
fn next_enabled_index(
    len: usize,
    current: Option<usize>,
    delta: i32,
    is_active: impl Fn(usize) -> bool,
) -> Option<usize> {
    if len == 0 || (0..len).all(&is_active) {
        return None;
    }

    let max = len as i32 - 1;
    let mut next = match current {
        Some(i) => (i as i32 + delta).clamp(0, max),
        None if delta < 0 => max,
        None => 0,
    };

    while is_active(next as usize) {
        let candidate = (next + delta.signum()).clamp(0, max);
        if candidate == next {
            return current.filter(|i| !is_active(*i));
        }
        next = candidate;
    }

    Some(next as usize)
}

#[cfg(test)]
mod tests {
    use super::next_enabled_index;

    fn none(_: usize) -> bool {
        false
    }

    #[test]
    fn steps_forward() {
        assert_eq!(next_enabled_index(3, Some(0), 1, none), Some(1));
    }

    #[test]
    fn skips_active_row() {
        assert_eq!(next_enabled_index(3, Some(0), 1, |i| i == 1), Some(2));
    }

    #[test]
    fn clamps_at_end() {
        assert_eq!(next_enabled_index(3, Some(2), 1, none), Some(2));
    }

    #[test]
    fn none_when_all_active() {
        assert_eq!(next_enabled_index(2, Some(0), 1, |_| true), None);
    }

    #[test]
    fn from_unselected_picks_first_enabled() {
        assert_eq!(next_enabled_index(2, None, 1, |i| i == 0), Some(1));
    }
}
