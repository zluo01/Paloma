//! Sessions popup in its own layer window, positioned right of the bar.
//! Rows are `AdwActionRow`s in a single-selection `ListBox`; the popup
//! window never takes keyboard (`KeyboardMode::None`), so list focus
//! can't conflict with the bar's entry — the key handler forwards
//! arrows/Enter as selection and activation. The active session's row
//! is disabled so it can't be re-restored on top of itself.

use std::rc::Rc;

use gtk4::{
    Align, ApplicationWindow, Button, ListBoxRow, ScrolledWindow, glib, subclass::prelude::*,
};
use libadwaita::{ActionRow, prelude::*};
use uuid::Uuid;

use super::{OVERLAY_CONTENT_HEIGHT_PX, SESSIONS_WIDTH_PX};

/// Sessions panel styling: header, transparent list, empty state.
pub(super) const CSS: &str = include_str!("style.css");

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use gtk4::{
        ApplicationWindow, Box as GtkBox, CompositeTemplate, Label, ListBox, glib, prelude::*,
        subclass::prelude::*,
    };
    use libadwaita::ActionRow;
    use uuid::Uuid;

    #[derive(CompositeTemplate, Default)]
    #[template(file = "sessions.ui")]
    pub struct SessionsView {
        #[template_child]
        pub list: TemplateChild<ListBox>,
        pub window: OnceCell<ApplicationWindow>,
        pub rows: RefCell<Vec<(Uuid, ActionRow)>>,
        /// Session currently shown on the chat surface; its row is disabled.
        pub active: Cell<Option<Uuid>>,
        /// Panel height (golden section of the monitor), for centering on the bar.
        pub height: Cell<i32>,
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
            // ListBox shows this whenever it has no rows.
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
    /// `window` is a prepared (hidden) layer window; this fills it.
    pub(super) fn new(window: ApplicationWindow) -> Self {
        let view: Self = glib::Object::new();
        view.set_width_request(SESSIONS_WIDTH_PX);
        view.set_height_request(OVERLAY_CONTENT_HEIGHT_PX);
        view.imp().height.set(OVERLAY_CONTENT_HEIGHT_PX);
        window.set_child(Some(&view));
        let _ = view.imp().window.set(window);
        view
    }

    /// The layer window this panel fills; positioned by the overlay.
    pub(super) fn window(&self) -> &ApplicationWindow {
        self.imp().window.get().expect("window set in new")
    }

    /// Fix the panel height (the golden section of the monitor) so it can be
    /// vertically centered on the bar.
    pub(super) fn set_height(&self, px: i32) {
        self.set_height_request(px);
        self.imp().height.set(px);
    }

    pub(super) fn height(&self) -> i32 {
        self.imp().height.get()
    }

    pub(super) fn is_open(&self) -> bool {
        self.window().is_visible()
    }

    pub(super) fn open(&self) {
        if self.window().is_visible() {
            return;
        }
        self.window().present();
        // No initial selection: the first arrow steps off the active session
        // (see `navigate`), not from the top of the list.
        self.scroll_to_active();
    }

    pub(super) fn close(&self) {
        if !self.window().is_visible() {
            return;
        }
        self.window().set_visible(false);
        self.imp().list.select_row(None::<&ListBoxRow>);
    }

    /// Rebuild the list, one row per `(id, provider, title)`. Resets the
    /// keyboard selection; if the popup is open, the first selectable row
    /// is re-selected so arrow navigation keeps working.
    pub(super) fn set_sessions(
        &self,
        on_session_click: Rc<dyn Fn(Uuid)>,
        on_delete: Rc<dyn Fn(Uuid)>,
        sessions: &[(Uuid, String, String)],
    ) {
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

            let view = self.clone();
            let on_click = on_session_click.clone();
            row.connect_activated(move |_| {
                if view.imp().active.get() == Some(id_copy) {
                    return;
                }
                view.close();
                on_click(id_copy);
            });

            let delete = Button::builder()
                .icon_name("user-trash-symbolic")
                .tooltip_text("Delete session")
                .valign(Align::Center)
                .css_classes(["flat", "circular"])
                .build();
            let on_delete = on_delete.clone();
            delete.connect_clicked(move |_| on_delete(id_copy));
            row.add_suffix(&delete);

            list.append(&row);
            new_rows.push((*id, row));
        }
        *self.imp().rows.borrow_mut() = new_rows;
        self.sync_row_states();

        if self.is_open() {
            self.select_first_enabled();
        }
    }

    pub(super) fn set_active(&self, session_id: Option<Uuid>) {
        self.imp().active.set(session_id);
        self.sync_row_states();
    }

    /// Move the keyboard selection by `delta`, skipping the active row.
    pub(super) fn navigate(&self, delta: i32) {
        let rows = self.imp().rows.borrow();
        if rows.is_empty() {
            return;
        }
        let list = &self.imp().list;
        let active = self.imp().active.get();
        // With nothing selected yet (just opened), step off the active session:
        // Down -> active + 1, Up -> active - 1.
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

    /// Activate the keyboard-selected row: close the popup and return the
    /// row's session id to the caller.
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
        self.close();
        Some(id)
    }

    /// The keyboard-selected session id (without closing the popup); used by
    /// the Delete key. The active row can't be selected, so it's never returned.
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

    /// Center the active session's row when the popup opens, so the user sees
    /// where they currently are.
    fn scroll_to_active(&self) {
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

    /// Disable the active session's row and drop the selection if it
    /// landed on it.
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

/// Scroll `row` to the vertical center of its scroller, deferring until the
/// row is allocated (the popup lays out a frame or two after it's shown).
fn center_when_allocated(row: &ActionRow) {
    row.add_tick_callback(|row, _clock| {
        // The row's bounds in the list (the scrollable content); `None`/zero
        // until the popup has laid out.
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

/// Scroll `row` just enough to bring it fully into view (for arrow nav).
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
