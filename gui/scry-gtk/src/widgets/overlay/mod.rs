//! Overlay assembly and internal API.
//!
//! Three layer-shell windows move as one: the focused search bar, the
//! results/chat content window below it, and the sessions popup beside it. A
//! single shared `position` stores the bar's top-left corner; the other windows
//! derive their margins from it.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk4::{
    Align, ApplicationWindow, Box as GtkBox, EventControllerKey, Orientation, PolicyType,
    PropagationPhase, ScrolledWindow, Viewport, prelude::*,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use libadwaita::Application;
use uuid::Uuid;

mod action_panel;
mod bar;
mod chat;
mod connectors;
mod controller;
mod keys;
mod results;
mod selection;
mod sessions;
mod window;

use action_panel::ActionPanel;
use bar::Bar;
use chat::ChatView;
pub(crate) use controller::OverlayController;
use results::ResultsView;
use scry_core::{Action, Connector, HealthLevel, Item, PermissionState, UserDecision};
use selection::{Selection, SelectionRef};
use sessions::SessionsView;

const SEARCH_BAR_HEIGHT_PX: i32 = 94;
const OVERLAY_WIDTH_PX: i32 = 640;
const OVERLAY_CONTENT_HEIGHT_PX: i32 = 420;
const SESSIONS_WIDTH_PX: i32 = 260;
const PANEL_GAP_PX: i32 = 8;

const SELECTED_CLASS: &str = "selected";
const CHAT_ACTION_LABEL: &str = "Chat about it";

const CSS: &str = include_str!("style.css");

/// Overlay stylesheet fragments loaded into the global GTK provider.
pub(crate) const CSS_PARTS: &[&str] = &[CSS, bar::CSS, results::CSS, chat::CSS, sessions::CSS];

/// Invokes a capability action by its static handler id.
type InvokeFn = Rc<dyn Fn(&'static str, Action)>;
/// Applies a resolved permission decision to the originating tool-call row.
type DecisionOutcomeFn = Box<dyn FnOnce(PermissionState)>;
/// Resolves a user-picked permission decision, then calls back on GTK.
type OnDecideFn = Rc<dyn Fn(UserDecision, DecisionOutcomeFn)>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Local,
    Chat,
}

/// Cloneable handle to the overlay windows, views, and shared state.
#[derive(Clone)]
struct Overlay {
    bar_window: ApplicationWindow,
    content_window: ApplicationWindow,
    bar: Bar,
    scroller: ScrolledWindow,
    results: ResultsView,
    chat: ChatView,
    sessions: SessionsView,
    sessions_window: ApplicationWindow,
    selection: SelectionRef,
    /// Retained Ctrl+K panel handle. Visibility, not `Option::is_some`, is the
    /// open-state source of truth because a dismissed popover can remain here
    /// until the next panel replaces it.
    last_action_panel: Rc<RefCell<Option<ActionPanel>>>,
    mode: Rc<Cell<Mode>>,
    /// Chat auto-scroll stays pinned until the user scrolls away.
    stuck_to_bottom: Rc<Cell<bool>>,
    /// Bar top-left in monitor pixels; `None` until first show.
    position: Rc<Cell<Option<(i32, i32)>>>,
    /// Monitor that `position` is relative to.
    monitor: Rc<RefCell<Option<gtk4::gdk::Monitor>>>,
}

/// Build the overlay windows. Kept alive (hidden) for the life of the
/// process so summoning is instant.
fn build(app: &Application) -> Overlay {
    let bar_window = layer_window(app, "scry-bar", OVERLAY_WIDTH_PX, KeyboardMode::Exclusive);
    bar_window.set_title(Some("Scry"));
    let bar = Bar::new(OVERLAY_WIDTH_PX);
    bar_window.set_child(Some(&bar));

    let results = ResultsView::new();
    let chat = ChatView::new(OVERLAY_WIDTH_PX);

    let content_box = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .valign(Align::Start)
        .build();
    content_box.append(results.widget());
    content_box.append(chat.widget());

    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        // Hug the content height, capped so long output scrolls.
        .propagate_natural_height(true)
        .max_content_height(OVERLAY_CONTENT_HEIGHT_PX)
        .width_request(OVERLAY_WIDTH_PX)
        // Avoid competing with TextView drag selection in chat output.
        .kinetic_scrolling(false)
        .build();
    // The scroller is the visible surface; clip the scrolled cards to
    // its rounded corners so they stay rounded mid-scroll.
    scroller.add_css_class("scry-surface");
    scroller.add_css_class("scry-scroller");
    scroller.set_overflow(gtk4::Overflow::Hidden);
    scroller.set_child(Some(&content_box));

    let content_window = layer_window(app, "scry-content", OVERLAY_WIDTH_PX, KeyboardMode::None);
    content_window.set_child(Some(&scroller));

    let sessions_window = layer_window(app, "scry-sessions", SESSIONS_WIDTH_PX, KeyboardMode::None);
    let sessions = SessionsView::new();
    sessions_window.set_child(Some(&sessions));

    let overlay = Overlay {
        bar_window,
        content_window,
        bar,
        scroller,
        results,
        chat,
        sessions,
        sessions_window,
        selection: Rc::new(RefCell::new(Selection::default())),
        last_action_panel: Rc::new(RefCell::new(None)),
        mode: Rc::new(Cell::new(Mode::Local)),
        stuck_to_bottom: Rc::new(Cell::new(true)),
        position: Rc::new(Cell::new(None)),
        monitor: Rc::new(RefCell::new(None)),
    };

    overlay.install_scroll_stickiness();
    overlay.install_bar_drag();
    overlay.install_monitor_watcher();
    overlay
}

/// A transparent, undecorated layer-shell window anchored top-left;
/// positioned via margins by [`Overlay::layout`].
fn layer_window(
    app: &Application,
    namespace: &str,
    width: i32,
    keyboard: KeyboardMode,
) -> ApplicationWindow {
    let window = ApplicationWindow::builder()
        .application(app)
        .default_width(width)
        .decorated(false)
        .resizable(false)
        .build();
    window.add_css_class("scry-window");

    window.init_layer_shell();
    window.set_namespace(Some(namespace));
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(keyboard);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Left, true);
    window
}

impl Overlay {
    fn is_visible(&self) -> bool {
        self.bar_window.is_visible()
    }

    fn show(&self) -> bool {
        let was_hidden = !self.bar_window.is_visible();
        if was_hidden {
            match self.position.get() {
                Some(_) => self.layout(),
                None => {
                    // Provisional guess for the first frame; the monitor
                    // watcher finalizes once the compositor has placed
                    // the surface on its real output.
                    let (x, y) = window::centered_position(&self.bar_window);
                    self.layout_at(x, y);
                },
            }
            self.bar_window.present();
        }
        self.bar.focus_entry();
        was_hidden
    }

    fn hide(&self) -> bool {
        self.bar.clear_input();
        self.clear_results();
        let backgrounded = self.mode.get() == Mode::Chat;
        if backgrounded {
            self.mode.set(Mode::Local);
            self.chat.hide();
            self.chat.reset();
        }
        self.close_sessions();
        self.hide_content();
        self.bar_window.set_visible(false);
        backgrounded
    }

    fn set_input(&self, text: &str) {
        self.bar.set_input(text);
    }

    fn clear_input(&self) {
        self.bar.clear_input();
    }

    fn input_text(&self) -> String {
        self.bar.input_text()
    }

    fn is_chat_mode(&self) -> bool {
        self.mode.get() == Mode::Chat
    }

    /// Whether the entry should receive GTK's native Ctrl+C copy.
    fn entry_has_selection(&self) -> bool {
        self.bar.has_selection()
    }

    /// Copy the chat transcript's text selection to the clipboard, if any.
    fn copy_chat_selection(&self) -> bool {
        self.chat.copy_selection()
    }

    fn connect_search_changed(&self, cb: impl Fn(String) + 'static) {
        self.bar.connect_search_changed(cb);
    }

    fn connect_key_pressed(
        &self,
        cb: impl Fn(gtk4::gdk::Key, gtk4::gdk::ModifierType) -> gtk4::glib::Propagation + 'static,
    ) {
        let controller = EventControllerKey::new();
        controller.set_propagation_phase(PropagationPhase::Capture);
        controller.connect_key_pressed(move |_, key, _, state| cb(key, state));
        self.bar_window.add_controller(controller);
    }

    fn connect_settings_clicked(&self, cb: impl Fn() + 'static) {
        self.bar.connect_settings_clicked(cb);
    }

    fn connect_sessions_clicked(&self, cb: impl Fn() + 'static) {
        self.bar.connect_sessions_clicked(cb);
    }

    fn connect_model_selected(&self, cb: impl Fn(bar::ModelChoice) + 'static) {
        self.bar.connect_model_selected(cb);
    }

    fn set_health(&self, models: HealthLevel, plugins: HealthLevel) {
        self.bar.set_health(models, plugins);
    }

    fn set_model_options(&self, connectors: &[Connector]) {
        self.bar.set_model_options(connectors);
    }

    // --- local results -------------------------------------------------

    fn clear_results(&self) {
        self.close_action_panel();
        self.results.clear(&self.selection);
    }

    /// Hide the results surface. Local mode only, so it never blanks chat
    /// content sharing the same window.
    fn hide_results(&self) {
        if self.mode.get() == Mode::Local {
            self.hide_content();
        }
    }

    fn append_section(
        &self,
        handler_id: &'static str,
        handler_name: &str,
        items: Vec<Item>,
        on_invoke: InvokeFn,
    ) {
        // Action-less items are skipped, so a non-empty `items` can still render
        // nothing; only reveal the surface when a row actually landed.
        if self.results.append_section(
            &self.selection,
            handler_id,
            handler_name,
            items,
            on_invoke,
            &self.panel_closer(),
        ) {
            self.show_content();
        }
    }

    fn append_chat_action(&self, invoke: Rc<dyn Fn()>) {
        if self.mode.get() != Mode::Local {
            return;
        }
        self.results
            .append_chat_action(&self.selection, invoke, &self.panel_closer());
        self.show_content();
    }

    // --- sessions panel ------------------------------------------------

    /// Replace the sessions panel content with one row per session.
    fn set_sessions(&self, sessions: &[(Uuid, String, String)]) {
        self.sessions
            .set_sessions(sessions, self.is_sessions_open());
    }

    /// Remove one session row in place (no scroll reset / rebuild).
    fn remove_session(&self, id: Uuid) {
        self.sessions.remove_session(id);
    }

    fn set_on_session_activated(&self, f: impl Fn(Uuid) + 'static) {
        self.sessions.set_on_session_activated(f);
    }

    fn set_on_session_deleted(&self, f: impl Fn(Uuid) + 'static) {
        self.sessions.set_on_session_deleted(f);
    }

    fn set_active_session(&self, session_id: Option<Uuid>) {
        self.sessions.set_active(session_id);
    }

    fn is_sessions_open(&self) -> bool {
        self.sessions_window.is_visible()
    }

    fn open_sessions(&self) {
        if self.sessions_window.is_visible() {
            return;
        }
        self.sessions_window.present();
        // No initial selection: the first arrow steps off the active session.
        self.sessions.scroll_to_active();
    }

    fn close_sessions(&self) {
        if !self.sessions_window.is_visible() {
            return;
        }
        self.sessions_window.set_visible(false);
        self.sessions.clear_selection();
    }

    fn toggle_sessions(&self) {
        if self.is_sessions_open() {
            self.close_sessions();
        } else {
            self.open_sessions();
        }
    }

    // --- chat ----------------------------------------------------------

    fn active_chat(&self) -> Option<&ChatView> {
        (self.mode.get() == Mode::Chat).then_some(&self.chat)
    }

    fn append_chat_text(&self, text: &str) {
        if let Some(chat) = self.active_chat() {
            chat.append_text(text);
        }
    }

    fn append_chat_reasoning(&self, text: &str) {
        if let Some(chat) = self.active_chat() {
            chat.append_reasoning(text);
        }
    }

    fn add_chat_tool_call(
        &self,
        name: &str,
        arguments: &str,
        description: Option<&str>,
        decisions: &[UserDecision],
        on_decide: OnDecideFn,
    ) {
        if let Some(chat) = self.active_chat() {
            chat.add_tool_call(name, arguments, description, decisions, on_decide);
        }
    }

    fn finish_chat_turn(&self) {
        if let Some(chat) = self.active_chat() {
            chat.finish_turn();
        }
    }

    fn fail_chat_turn(&self, message: &str) {
        if let Some(chat) = self.active_chat() {
            chat.fail_turn(message);
        }
    }

    fn cancel_chat_turn(&self) {
        if let Some(chat) = self.active_chat() {
            chat.cancel_turn();
        }
    }

    fn start_chat_turn(&self, text: &str) {
        if let Some(chat) = self.active_chat() {
            chat.start_turn(text);
        }
    }

    /// Enter chat mode on a blank surface so a restored session can be
    /// repainted from its replayed events.
    fn enter_chat_for_restore(&self) {
        self.enter_chat_mode();
    }

    // --- internals (shared with `keys`) ---------------------------------

    fn enter_chat_mode(&self) {
        self.mode.set(Mode::Chat);
        self.clear_results();
        self.chat.reset();
        self.chat.show();
        self.show_content();
        // Scroll reset first: the value drop reads as "scrolled up" to
        // the stick tracker, so the stick flag is set afterwards.
        self.scroller.vadjustment().set_value(0.0);
        self.stuck_to_bottom.set(true);
    }

    fn exit_chat_mode(&self) {
        self.mode.set(Mode::Local);
        self.chat.hide();
        self.chat.reset();
        self.hide_content();
        self.bar.clear_input();
    }

    fn activate_selection(&self) {
        let primary = self.selection.borrow().activate();
        if let Some(primary) = primary {
            primary();
        }
    }

    // --- action panel (Ctrl+K) ------------------------------------------

    /// Visibility is the source of truth: a panel dismissed by clicking one of
    /// its own actions can briefly remain in the slot, closed, until reopened.
    fn is_action_panel_open(&self) -> bool {
        self.last_action_panel
            .borrow()
            .as_ref()
            .is_some_and(ActionPanel::is_open)
    }

    /// Open the action panel for the selected row, if it has more than one
    /// action. No-op otherwise.
    fn open_action_panel(&self) {
        if self.is_action_panel_open() {
            return;
        }
        let Some((anchor, actions)) = self.selection.borrow().selected_actions() else {
            return;
        };
        let bar = self.bar.clone();
        let panel = ActionPanel::new(&anchor, actions, move || bar.focus_entry());
        *self.last_action_panel.borrow_mut() = Some(panel);
    }

    fn close_action_panel(&self) {
        if let Some(panel) = take_action_panel(&self.last_action_panel) {
            panel.close();
        }
    }

    /// A callback that closes the panel, handed to result rows so a pointer
    /// click on another row dismisses an open panel (clicks bypass `keys`).
    fn panel_closer(&self) -> Rc<dyn Fn()> {
        let slot = self.last_action_panel.clone();
        Rc::new(move || {
            if let Some(panel) = take_action_panel(&slot) {
                panel.close();
            }
        })
    }

    fn navigate_action_panel(&self, delta: i32) {
        if let Some(panel) = self.last_action_panel.borrow().as_ref() {
            panel.navigate(delta);
        }
    }

    fn activate_action_panel(&self) {
        if let Some(panel) = take_action_panel(&self.last_action_panel) {
            panel.activate();
        }
    }

    fn scroll_selection_into_view(&self) {
        let selection = self.selection.borrow();
        let Some(widget) = selection.selected_widget() else {
            return;
        };
        // The first/last rows snap the card fully to the top/bottom so its
        // padding (and the chat divider) isn't clipped; the rows sit below the
        // card padding, so a minimal scroll-to would leave that padding cut off.
        // Middle rows use the minimal scroll.
        let adj = self.scroller.vadjustment();
        match selection.selected_index() {
            Some(0) => adj.set_value(0.0),
            Some(i) if i + 1 == selection.len() => {
                adj.set_value((adj.upper() - adj.page_size()).max(0.0));
            },
            _ => scroll_into_view(&widget),
        }
    }

    /// Arrow keys while chatting walk the pending permission prompts
    /// (all tool calls' options form one sequence). Returns whether a
    /// prompt consumed the key.
    fn navigate_decisions(&self, delta: i32) -> bool {
        if !self.chat.navigate_decisions(delta) {
            return false;
        }
        if let Some(button) = self.chat.selected_decision() {
            scroll_into_view(&button);
        }
        true
    }

    fn activate_selected_decision(&self) -> bool {
        self.chat.activate_selected_decision()
    }

    // --- content window --------------------------------------------------

    fn show_content(&self) {
        if !self.content_window.is_visible() {
            self.content_window.present();
        }
    }

    fn hide_content(&self) {
        self.content_window.set_visible(false);
        // Mode is back to Local before content hides, so this reset does not
        // affect chat stickiness.
        self.scroller.vadjustment().set_value(0.0);
    }
}

/// Take the panel out before closing/activating so synchronous `closed` handlers
/// cannot re-borrow the same `RefCell`.
fn take_action_panel(slot: &RefCell<Option<ActionPanel>>) -> Option<ActionPanel> {
    slot.borrow_mut().take()
}

/// Scroll `widget` into its enclosing viewport. Needed because the
/// keyboard highlight is a CSS class, not GTK focus.
fn scroll_into_view(widget: &impl IsA<gtk4::Widget>) {
    if let Some(viewport) = widget
        .ancestor(Viewport::static_type())
        .and_downcast::<Viewport>()
    {
        viewport.scroll_to(widget, None);
    }
}
