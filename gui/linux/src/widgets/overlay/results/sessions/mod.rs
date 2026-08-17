use std::{
    cell::RefCell,
    collections::HashSet,
    rc::Rc,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use futures::channel::mpsc;
use gtk4::{
    Align, Box as GtkBox, Button, Image, Label, ListBox, ListBoxRow, Orientation, Revealer,
    RevealerTransitionType, SelectionMode, StateFlags, prelude::*,
};
use libadwaita::{ActionRow, prelude::*};
use log::error;
use paloma_core::{AppContext, SessionListItem};
use uuid::Uuid;

use super::step_index;
use crate::{
    helper::{Clear, scroll_selection_into_view},
    runtime,
    widgets::overlay::{
        OVERLAY_WIDTH_PX,
        model::{Msg, SessionMsg},
    },
};

const EMPTY_PLACEHOLDER: &str = "No sessions yet";
const NO_MATCH_PLACEHOLDER: &str = "No matching sessions";

struct SessionRow {
    session_id: Uuid,
    action_row: ActionRow,
}

pub(crate) struct SessionsView {
    view: GtkBox,
    list: ListBox,
    empty: Label,
    sessions: Rc<RefCell<Vec<SessionRow>>>,
    app_context: Arc<AppContext>,
    dispatcher: mpsc::UnboundedSender<Msg>,
}

impl SessionsView {
    pub(crate) fn new(
        app_context: Arc<AppContext>,
        dispatcher: mpsc::UnboundedSender<Msg>,
    ) -> Self {
        let view = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .halign(Align::Center)
            .width_request(OVERLAY_WIDTH_PX)
            .css_classes(["paloma-result-card"])
            .build();

        let list = ListBox::builder()
            .selection_mode(SelectionMode::Single)
            .css_classes(["paloma-sessions-list"])
            .build();

        let empty = Label::builder()
            .label(EMPTY_PLACEHOLDER)
            .justify(gtk4::Justification::Center)
            .css_classes(["paloma-sessions-empty-title"])
            .build();
        let empty_icon = Image::builder()
            .icon_name("document-open-recent-symbolic")
            .pixel_size(32)
            .css_classes(["paloma-sessions-empty-icon"])
            .build();
        let placeholder = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(10)
            .halign(Align::Center)
            .valign(Align::Center)
            .css_classes(["paloma-sessions-empty"])
            .build();
        placeholder.append(&empty_icon);
        placeholder.append(&empty);
        list.set_placeholder(Some(&placeholder));

        view.append(&list);

        Self {
            view,
            list,
            empty,
            sessions: Rc::new(RefCell::new(vec![])),
            app_context,
            dispatcher,
        }
    }

    pub(crate) fn widget(&self) -> &GtkBox {
        &self.view
    }

    pub(crate) fn has_selection(&self) -> bool {
        self.list.selected_row().is_some()
    }

    pub(crate) fn refresh(&self) {
        let app_context = self.app_context.clone();
        let list = self.list.clone();
        let empty = self.empty.clone();
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
                        &empty,
                        &session_rows,
                        app_context,
                        dispatcher,
                        sessions,
                    );
                },
                Err(err) => error!("failed to refresh sessions: {err}"),
            },
        );
    }

    pub(crate) fn activate_selected(&self) {
        // is_mapped rejects rows hidden by the filter, a stack page switch, or a stale selection
        if let Some(selected) = self.list.selected_row()
            && selected.is_mapped()
        {
            selected.activate();
        }
    }

    pub(crate) fn delete_selected(&self) {
        let Some(selected) = self.list.selected_row().filter(|row| row.is_mapped()) else {
            return;
        };
        let index = selected.index();
        if index < 0 {
            return;
        }

        let Some(session_id) = self
            .sessions
            .borrow()
            .get(index as usize)
            .map(|session| session.session_id)
        else {
            return;
        };

        remove_session(
            &self.list,
            &self.empty,
            &self.sessions,
            session_id,
            self.app_context.clone(),
        );
    }

    pub(crate) fn clear(&self) {
        self.list.select_row(None::<&ListBoxRow>);
        self.empty.set_label(EMPTY_PLACEHOLDER);
        for session in self.sessions.borrow().iter() {
            session.action_row.set_visible(true);
        }
    }

    pub(crate) fn navigate(&self, delta: i32) {
        let sessions = self.sessions.borrow();
        let visible_sessions: Vec<&SessionRow> = sessions
            .iter()
            .filter(|s| s.action_row.is_visible())
            .collect();
        if visible_sessions.is_empty() {
            return;
        }

        // the selected row may itself be filtered out
        let current = self.list.selected_row().and_then(|row| {
            visible_sessions
                .iter()
                .position(|session| session.action_row.index() == row.index())
        });

        let next = match current {
            Some(current) => step_index(current, delta, visible_sessions.len()),
            None => 0,
        };

        if current == Some(next) {
            return;
        }

        let action_row = &visible_sessions[next].action_row;
        self.list.select_row(Some(action_row));
        scroll_selection_into_view(action_row, next, visible_sessions.len());
    }

    pub(crate) fn filter(&self, query: String) {
        // an empty needle must also show sessions the content search cannot match
        if query.is_empty() {
            self.clear();
            return;
        }

        // always reset selection
        self.list.select_row(None::<&ListBoxRow>);

        let app_context = self.app_context.clone();
        let empty = self.empty.clone();
        let session_rows = self.sessions.clone();
        runtime::spawn_with(
            async move { app_context.search_sessions(query).await },
            move |result| match result {
                Ok(sessions) => {
                    let matches: HashSet<Uuid> = sessions.into_iter().collect();
                    let rows = session_rows.borrow();
                    for row in rows.iter() {
                        row.action_row
                            .set_visible(matches.contains(&row.session_id));
                    }
                    update_placeholder(&empty, &rows);
                },
                Err(err) => error!("failed to search sessions: {err}"),
            },
        );
    }
}

fn set_sessions(
    list: &ListBox,
    empty: &Label,
    session_rows: &Rc<RefCell<Vec<SessionRow>>>,
    app_context: Arc<AppContext>,
    dispatcher: mpsc::UnboundedSender<Msg>,
    sessions: Vec<SessionListItem>,
) {
    list.clear();

    let now = now_epoch();
    let mut rows = Vec::with_capacity(sessions.len());
    for session in sessions {
        let row = ActionRow::builder()
            .title(&session.title)
            // titles are user content, not Pango markup
            .use_markup(false)
            .title_lines(1)
            .activatable(true)
            .tooltip_text(&session.title)
            .build();

        let icon = Image::builder()
            .icon_name("document-open-recent-symbolic")
            .pixel_size(14)
            .valign(Align::Center)
            .css_classes(["paloma-session-icon"])
            .build();
        row.add_prefix(&icon);

        let session_id = session.session_id;
        let action_dispatcher = dispatcher.clone();
        row.connect_activated(move |_| {
            let _ = action_dispatcher
                .unbounded_send(Msg::Session(SessionMsg::RestoreRequested { session_id }));
        });

        let delete = Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Delete session")
            .valign(Align::Center)
            .css_classes(["flat", "circular"])
            .build();

        let list_for_delete = list.clone();
        let empty_for_delete = empty.clone();
        let session_rows_for_delete = session_rows.clone();
        let app_context = app_context.clone();
        delete.connect_clicked(move |_| {
            remove_session(
                &list_for_delete,
                &empty_for_delete,
                &session_rows_for_delete,
                session_id,
                app_context.clone(),
            );
        });

        let time = Label::builder()
            .label(relative_time_at(now, session.last_update))
            .valign(Align::Center)
            .css_classes(["paloma-session-time"])
            .build();
        row.add_suffix(&time);

        // zero width until hovered, so the time sits flush right otherwise
        let reveal = Revealer::builder()
            .child(&delete)
            .transition_type(RevealerTransitionType::SlideLeft)
            .transition_duration(150)
            .valign(Align::Center)
            .build();
        row.add_suffix(&reveal);

        row.connect_state_flags_changed(move |row, _| {
            reveal.set_reveal_child(row.state_flags().contains(StateFlags::PRELIGHT));
        });

        list.append(&row);
        rows.push(SessionRow {
            session_id,
            action_row: row,
        });
    }

    *session_rows.borrow_mut() = rows;
}

fn remove_session(
    list: &ListBox,
    empty: &Label,
    session_rows: &Rc<RefCell<Vec<SessionRow>>>,
    session_id: Uuid,
    app_context: Arc<AppContext>,
) {
    let list = list.clone();
    let empty = empty.clone();
    let session_rows = session_rows.clone();

    runtime::spawn_with(
        async move { app_context.remove_session(session_id).await },
        move |result| match result {
            Ok(()) => {
                let mut rows = session_rows.borrow_mut();
                // a refresh may have rebuilt the list mid-delete; resolve the row by id
                let Some(pos) = rows
                    .iter()
                    .position(|session| session.session_id == session_id)
                else {
                    return;
                };
                let removed = rows.remove(pos).action_row;
                // a selection is an explicit signal; a trash-click delete must not create one
                let was_selected =
                    list.selected_row().map(|row| row.index()) == Some(removed.index());
                list.remove(&removed);

                if was_selected {
                    let visible: Vec<&SessionRow> =
                        rows.iter().filter(|s| s.action_row.is_visible()).collect();
                    let before = rows[..pos]
                        .iter()
                        .filter(|s| s.action_row.is_visible())
                        .count();
                    if !visible.is_empty() {
                        // first visible row at/after the deleted position, else the last one
                        let nearest = before.min(visible.len() - 1);
                        list.select_row(Some(&visible[nearest].action_row));
                        scroll_selection_into_view(
                            &visible[nearest].action_row,
                            nearest,
                            visible.len(),
                        );
                    }
                }

                update_placeholder(&empty, &rows);
            },
            Err(err) => error!("failed to remove session: {err}"),
        },
    );
}

fn update_placeholder(empty: &Label, rows: &[SessionRow]) {
    let has_visible = rows.iter().any(|s| s.action_row.is_visible());
    empty.set_label(if has_visible || rows.is_empty() {
        EMPTY_PLACEHOLDER
    } else {
        NO_MATCH_PLACEHOLDER
    });
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
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
        MONTH..YEAR => format!("{}mo ago", (delta / MONTH).min(11)),
        _ => format!("{}y ago", delta / YEAR),
    }
}

#[cfg(test)]
mod tests {
    use super::relative_time_at;

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
        assert_eq!(relative_time_at(now, now - 364 * DAY), "11mo ago");
        assert_eq!(relative_time_at(now, now - YEAR), "1y ago");
        assert_eq!(relative_time_at(now, now - 2 * YEAR), "2y ago");
    }
}
