use std::{
    cell::RefCell,
    rc::Rc,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use futures::channel::mpsc;
use gtk4::{
    Align, Box as GtkBox, Button, Label, ListBox, ListBoxRow, Orientation, PolicyType,
    ScrolledWindow, SelectionMode, prelude::*,
};
use libadwaita::{ActionRow, prelude::*};
use log::error;
use scry_core::{AppContext, SessionListItem};
use uuid::Uuid;

use super::{OVERLAY_CONTENT_HEIGHT_PX, SESSIONS_WIDTH_PX};
use crate::{helper::Clear, runtime, widgets::overlay::model::Msg};

pub(super) const CSS: &str = include_str!("style.css");

struct SessionRow {
    session_id: Uuid,
    action_row: ActionRow,
}

pub(super) struct SessionsView {
    view: GtkBox,
    header: Label,
    list: ListBox,
    sessions: Rc<RefCell<Vec<SessionRow>>>,
    app_context: Arc<AppContext>,
    dispatcher: mpsc::UnboundedSender<Msg>,
}

impl SessionsView {
    pub(super) fn new(
        app_context: Arc<AppContext>,
        dispatcher: mpsc::UnboundedSender<Msg>,
    ) -> Self {
        let view = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .width_request(SESSIONS_WIDTH_PX)
            .height_request(OVERLAY_CONTENT_HEIGHT_PX)
            .css_classes(["scry-surface", "scry-sessions-card"])
            .build();

        let header = Label::builder()
            .label("Sessions")
            .xalign(0.0)
            .css_classes(["scry-sessions-header"])
            .build();

        let list = ListBox::builder()
            .selection_mode(SelectionMode::Single)
            .css_classes(["scry-sessions-list"])
            .build();

        let empty = Label::builder()
            .label("No sessions yet")
            .halign(Align::Center)
            .valign(Align::Center)
            .css_classes(["scry-sessions-empty"])
            .build();
        empty.add_css_class("scry-sessions-empty");
        list.set_placeholder(Some(&empty));

        let scroller = ScrolledWindow::builder()
            .hscrollbar_policy(PolicyType::Never)
            .vscrollbar_policy(PolicyType::Automatic)
            .vexpand(true)
            .css_classes(["scry-scroller"])
            .child(&list)
            .build();

        view.append(&header);
        view.append(&scroller);

        Self {
            view,
            header,
            list,
            sessions: Rc::new(RefCell::new(vec![])),
            app_context,
            dispatcher,
        }
    }

    pub(super) fn widget(&self) -> &GtkBox {
        &self.view
    }

    pub(in crate::widgets::overlay) fn height(&self) -> i32 {
        self.view.height_request()
    }

    pub(in crate::widgets::overlay) fn set_height(&self, height: i32) {
        self.view.set_height_request(height);
    }

    pub(super) fn refresh(&self, session_id: Option<Uuid>) {
        let app_context = self.app_context.clone();
        let list = self.list.clone();
        let header = self.header.clone();
        let session_rows = self.sessions.clone();
        let dispatcher = self.dispatcher.clone();
        runtime::spawn_with(
            {
                let app_context = app_context.clone();
                async move { app_context.available_sessions().await }
            },
            move |result| match result {
                Ok(sessions) => {
                    set_sessions(
                        &list,
                        &header,
                        &session_rows,
                        app_context,
                        dispatcher,
                        sessions,
                        session_id,
                    );
                },
                Err(err) => error!("failed to refresh sessions: {err}"),
            },
        );
    }

    pub(super) fn activate_selected(&self) {
        if let Some(selected) = self.list.selected_row()
            && selected.is_sensitive()
        {
            selected.activate();
        }
    }

    pub(super) fn delete_selected(&self) {
        let Some(selected) = self.list.selected_row().filter(|row| row.is_sensitive()) else {
            return;
        };
        let index = selected.index();
        if index < 0 {
            return;
        }

        let Some((session_id, action_row)) = self
            .sessions
            .borrow()
            .get(index as usize)
            .map(|session| (session.session_id, session.action_row.clone()))
        else {
            return;
        };

        remove_session(
            &self.list,
            &self.header,
            &self.sessions,
            &action_row,
            session_id,
            self.app_context.clone(),
        );
    }

    pub(super) fn clear_selection(&self) {
        self.list.select_row(None::<&ListBoxRow>);
    }

    pub(super) fn navigate(&self, delta: i32) {
        let sessions = self.sessions.borrow();
        if sessions.is_empty() {
            return;
        }

        // If no row is selected, anchor navigation at the active row so Up and
        // Down move to its neighboring sessions.
        let current = self
            .list
            .selected_row()
            .map(|row| row.index() as usize)
            .or_else(|| {
                sessions
                    .iter()
                    .position(|session| !session.action_row.is_sensitive())
            });

        let Some(next) = next_enabled_index(sessions.len(), current, delta, |i| {
            !sessions[i].action_row.is_sensitive()
        }) else {
            return;
        };

        if current == Some(next) {
            return;
        }

        if let Some(action_row) = sessions.get(next).map(|session| &session.action_row) {
            self.list.select_row(Some(action_row));
            bring_into_view(action_row);
        }
    }
}

fn set_sessions(
    list: &ListBox,
    header: &Label,
    session_rows: &Rc<RefCell<Vec<SessionRow>>>,
    app_context: Arc<AppContext>,
    dispatcher: mpsc::UnboundedSender<Msg>,
    sessions: Vec<SessionListItem>,
    current_session_id: Option<Uuid>,
) {
    list.clear();
    session_rows.borrow_mut().clear();

    let mut current_row = None;
    for session in sessions {
        let row = ActionRow::builder()
            .title(&session.title)
            .subtitle(relative_time(session.last_update))
            .title_lines(1)
            .subtitle_lines(1)
            .activatable(true)
            .tooltip_text(&session.title)
            .build();

        let session_id = session.session_id;
        if current_session_id == Some(session_id) {
            current_row = Some(row.clone());
            row.set_sensitive(false);
        }
        let action_dispatcher = dispatcher.clone();
        row.connect_activated(move |_| {
            let _ = action_dispatcher.unbounded_send(Msg::SessionRestoreRequested { session_id });
        });

        let delete = Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Delete session")
            .valign(Align::Center)
            .css_classes(["flat", "circular"])
            .build();

        let list_for_delete = list.clone();
        let header_for_delete = header.clone();
        let session_rows_for_delete = session_rows.clone();
        let action_row = row.clone();
        let app_context = app_context.clone();
        delete.connect_clicked(move |_| {
            remove_session(
                &list_for_delete,
                &header_for_delete,
                &session_rows_for_delete,
                &action_row,
                session_id,
                app_context.clone(),
            );
        });
        row.add_suffix(&delete);

        list.append(&row);
        session_rows.borrow_mut().push(SessionRow {
            session_id,
            action_row: row,
        });
    }

    set_header(header, session_rows.borrow().len());

    if let Some(row) = current_row {
        center_when_allocated(&row);
        list.select_row(Some(&row))
    }
}

fn remove_session(
    list: &ListBox,
    header: &Label,
    session_rows: &Rc<RefCell<Vec<SessionRow>>>,
    action_row: &ActionRow,
    session_id: Uuid,
    app_context: Arc<AppContext>,
) {
    let list = list.clone();
    let header = header.clone();
    let session_rows = session_rows.clone();
    let action_row = action_row.clone();

    runtime::spawn_with(
        async move { app_context.remove_session(session_id).await },
        move |result| match result {
            Ok(()) => {
                list.remove(&action_row);
                session_rows
                    .borrow_mut()
                    .retain(|session| session.session_id != session_id);
                set_header(&header, session_rows.borrow().len());
            },
            Err(err) => error!("failed to remove session: {err}"),
        },
    );
}

/// Header text: plain "Sessions" when empty, "Sessions (n)" otherwise.
fn set_header(header: &Label, count: usize) {
    if count == 0 {
        header.set_label("Sessions");
    } else {
        header.set_label(&format!("Sessions ({count})"));
    }
}

fn relative_time(epoch_secs: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0);

    relative_time_at(now, epoch_secs)
}

fn relative_time_at(now: i64, epoch_secs: i64) -> String {
    const MIN: i64 = 60;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;
    const MONTH: i64 = 30 * DAY;
    const YEAR: i64 = 365 * DAY;

    let delta = now.saturating_sub(epoch_secs).max(0);

    match delta {
        0..MIN => "just now".to_string(),
        MIN..HOUR => format!("{}m ago", delta / MIN),
        HOUR..DAY => format!("{}h ago", delta / HOUR),
        DAY..MONTH => format!("{}d ago", delta / DAY),
        MONTH..YEAR => format!("{}mo ago", delta / MONTH),
        _ => format!("{}y ago", delta / YEAR),
    }
}

fn center_when_allocated(row: &ActionRow) {
    let Some(scroller) = row
        .ancestor(ScrolledWindow::static_type())
        .and_downcast::<ScrolledWindow>()
    else {
        return;
    };
    let vadj = scroller.vadjustment();

    // The adjustment reports a real extent only once the list has been
    // allocated; `changed` fires at exactly that point. Scroll once, then
    // disconnect so we don't fight the user's later scrolling.
    let handler = Rc::new(RefCell::new(None));
    let id = vadj.connect_changed({
        let row = row.clone();
        let handler = handler.clone();
        move |vadj| {
            if vadj.upper() <= 0.0 {
                return;
            }
            if let Some(id) = handler.borrow_mut().take() {
                vadj.disconnect(id);
            }
            if vadj.upper() <= vadj.page_size() {
                return;
            }
            let Some(bounds) = row.parent().and_then(|list| row.compute_bounds(&list)) else {
                return;
            };
            let center = (bounds.y() + bounds.height() / 2.0) as f64;
            let max = (vadj.upper() - vadj.page_size()).max(0.0);
            vadj.set_value((center - vadj.page_size() / 2.0).clamp(0.0, max));
        }
    });
    *handler.borrow_mut() = Some(id);
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
    use super::{next_enabled_index, relative_time_at};

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

    #[test]
    fn relative_time_handles_recent_and_future_timestamps() {
        assert_eq!(relative_time_at(1_000, 1_000), "just now");
        assert_eq!(relative_time_at(1_000, 941), "just now");
        assert_eq!(relative_time_at(1_000, 1_001), "just now");
    }

    #[test]
    fn relative_time_formats_minutes_hours_and_days() {
        const MIN: i64 = 60;
        const HOUR: i64 = 60 * MIN;
        const DAY: i64 = 24 * HOUR;

        let now = 1_000_000;

        assert_eq!(relative_time_at(now, now - MIN), "1m ago");
        assert_eq!(relative_time_at(now, now - 59 * MIN), "59m ago");
        assert_eq!(relative_time_at(now, now - HOUR), "1h ago");
        assert_eq!(relative_time_at(now, now - 23 * HOUR), "23h ago");
        assert_eq!(relative_time_at(now, now - DAY), "1d ago");
        assert_eq!(relative_time_at(now, now - 29 * DAY), "29d ago");
    }

    #[test]
    fn relative_time_formats_months_and_years() {
        const DAY: i64 = 24 * 60 * 60;
        const MONTH: i64 = 30 * DAY;
        const YEAR: i64 = 365 * DAY;

        let now = 100_000_000;

        assert_eq!(relative_time_at(now, now - MONTH), "1mo ago");
        assert_eq!(relative_time_at(now, now - 11 * MONTH), "11mo ago");
        assert_eq!(relative_time_at(now, now - YEAR), "1y ago");
        assert_eq!(relative_time_at(now, now - 2 * YEAR), "2y ago");
    }
}
