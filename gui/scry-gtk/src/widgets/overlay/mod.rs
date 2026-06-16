//! Overlay assembly and public API.
//!
//! Three layer-shell windows that move as one: the search bar (owns
//! keyboard focus), the content window (results/chat) directly below
//! it, and the sessions popup to its right. All are positioned from a
//! single shared `position` — the bar's top-left corner — which starts
//! centered and follows the user's drag afterwards.
//!
//! Widgets and behaviors live in the submodules: `bar` (search bar),
//! `chat` (turn rendering), `keys` (keyboard dispatch), `results`
//! (capability rows), `selection` (keyboard selection state),
//! `sessions` (dev popup), `window` (monitor/centering math).

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

mod bar;
mod chat;
mod connectors;
mod controller;
mod keys;
mod results;
mod selection;
mod sessions;
mod window;

use chat::ChatView;
pub(crate) use controller::OverlayController;
use results::ResultsView;
use scry_core::{Action, Connector, HealthLevel, Item, PermissionState, UserDecision};
use selection::{Activation, Selection, SelectionRef};
use sessions::SessionsView;

const SEARCH_BAR_HEIGHT_PX: i32 = 94;
const OVERLAY_WIDTH_PX: i32 = 640;
const OVERLAY_CONTENT_HEIGHT_PX: i32 = 420;
const SESSIONS_WIDTH_PX: i32 = 260;
/// Gap between the bar and its satellite windows.
const PANEL_GAP_PX: i32 = 8;

const CHEVRON_COLLAPSED: &str = "pan-end-symbolic";
const CHEVRON_EXPANDED: &str = "pan-down-symbolic";
const SELECTED_CLASS: &str = "selected";
const CHAT_ACTION_LABEL: &str = "Chat about it";

/// Shell styling: transparent windows and the shared `.scry-surface`
/// card chrome.
const CSS: &str = include_str!("style.css");

/// CSS fragments contributed by the overlay and its submodules.
/// Aggregated into the global stylesheet by `crate::style::load`.
pub(crate) const CSS_PARTS: &[&str] = &[CSS, bar::CSS, results::CSS, chat::CSS, sessions::CSS];

/// Runs a capability action: `(handler_id, action)`.
type InvokeFn = Rc<dyn Fn(String, Action)>;
/// Applies the resolved [`PermissionState`] back to the tool-call row
/// that raised the prompt. Runs on the GTK main thread.
type DecisionOutcomeFn = Box<dyn FnOnce(PermissionState)>;
/// Fired when the user picks a permission decision under a tool call;
/// the handler resolves it and hands the outcome to the callback.
type OnDecideFn = Rc<dyn Fn(UserDecision, DecisionOutcomeFn)>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Local,
    Chat,
}

/// Handle to the overlay windows and views. Cheap to clone — every
/// field is a widget handle or `Rc`-shared state.
#[derive(Clone)]
struct Overlay {
    bar_window: ApplicationWindow,
    content_window: ApplicationWindow,
    bar: bar::Bar,
    scroller: ScrolledWindow,
    results: ResultsView,
    chat: ChatView,
    sessions: SessionsView,
    selection: SelectionRef,
    mode: Rc<Cell<Mode>>,
    /// Chat auto-scroll: pinned to the bottom until the user scrolls away.
    stuck_to_bottom: Rc<Cell<bool>>,
    /// The bar's top-left corner in monitor pixels; `None` until the
    /// first show computes the centered default.
    position: Rc<Cell<Option<(i32, i32)>>>,
    /// The monitor `position` is relative to. When the compositor maps
    /// the bar on a different monitor (e.g. focus moved to another
    /// screen), the position is recomputed for it.
    monitor: Rc<RefCell<Option<gtk4::gdk::Monitor>>>,
}

/// Build the overlay windows. Kept alive (hidden) for the life of the
/// process so summoning is instant.
fn build(app: &Application) -> Overlay {
    let bar_window = layer_window(app, "scry-bar", OVERLAY_WIDTH_PX, KeyboardMode::Exclusive);
    bar_window.set_title(Some("Scry"));
    let bar = bar::Bar::new(OVERLAY_WIDTH_PX);
    bar_window.set_child(Some(&bar));

    let results = ResultsView::new();
    let chat = ChatView::new(OVERLAY_WIDTH_PX);

    let content_box = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .valign(Align::Start)
        .build();
    content_box.append(&results);
    content_box.append(&chat);

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

    let sessions = SessionsView::new(layer_window(
        app,
        "scry-sessions",
        SESSIONS_WIDTH_PX,
        KeyboardMode::None,
    ));

    let overlay = Overlay {
        bar_window,
        content_window,
        bar,
        scroller,
        results,
        chat,
        sessions,
        selection: Rc::new(RefCell::new(Selection::default())),
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
        self.results.clear(&self.selection);
        let backgrounded = self.mode.get() == Mode::Chat;
        if backgrounded {
            self.mode.set(Mode::Local);
            self.chat.hide();
            self.chat.reset();
        }
        self.sessions.close();
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

    /// Whether the input entry has a text selection — used to let Ctrl+C copy
    /// instead of cancelling.
    fn entry_has_selection(&self) -> bool {
        self.bar.has_selection()
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

    /// Repaint the bar's status dots from aggregate controller health.
    fn set_health(&self, models: HealthLevel, plugins: HealthLevel) {
        self.bar.set_health(models, plugins);
    }

    /// Repopulate the bar's model picker (cascading provider → model menu and
    /// the effort menu) from the current connectors.
    fn set_model_options(&self, connectors: &[Connector]) {
        self.bar.set_model_options(connectors);
    }

    // --- local results -------------------------------------------------

    fn clear_results(&self) {
        self.results.clear(&self.selection);
        if self.mode.get() == Mode::Local {
            self.hide_content();
        }
    }

    fn append_section(
        &self,
        handler_id: &str,
        handler_name: &str,
        items: Vec<Item>,
        on_invoke: InvokeFn,
    ) {
        if items.is_empty() {
            return;
        }
        self.results
            .append_section(&self.selection, handler_id, handler_name, items, on_invoke);
        self.show_content();
    }

    /// Append the "Chat about it" row under the current results.
    fn append_chat_action(&self, invoke: Rc<dyn Fn()>) {
        if self.mode.get() != Mode::Local {
            return;
        }
        self.results.append_chat_action(&self.selection, invoke);
        self.show_content();
    }

    // --- sessions panel ------------------------------------------------

    /// Replace the sessions panel content with one row per
    /// `(id, provider, title)`. Called sparingly (start, session
    /// create/cleanup) — not per broadcast event.
    fn set_sessions(
        &self,
        sessions: &[(Uuid, String, String)],
        on_selected: Rc<dyn Fn(Uuid)>,
        on_delete: Rc<dyn Fn(Uuid)>,
    ) {
        self.sessions.set_sessions(on_selected, on_delete, sessions);
    }

    fn set_active_session(&self, session_id: Option<Uuid>) {
        self.sessions.set_active(session_id);
    }

    fn toggle_sessions(&self) {
        if self.sessions.is_open() {
            self.sessions.close();
        } else {
            self.sessions.open();
        }
    }

    // --- chat ----------------------------------------------------------

    /// The chat view while it is the active surface. Broadcasts can arrive
    /// while hidden or after a session switch, so callers no-op otherwise.
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
        self.results.clear(&self.selection);
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
        let activation = self.selection.borrow().activation();
        match activation {
            Some(Activation::Invoke(f)) => f(),
            Some(Activation::Expand(row)) => {
                self.selection.borrow_mut().toggle_row(row);
                self.scroll_selection_into_view();
            },
            None => {},
        }
    }

    fn scroll_selection_into_view(&self) {
        if let Some(widget) = self.selection.borrow().selected_widget() {
            scroll_into_view(&widget);
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
        // Mode is back to Local by the time content hides, so this
        // reset never reaches the chat stick-tracking.
        self.scroller.vadjustment().set_value(0.0);
    }
}

/// Scroll `widget` into its enclosing viewport. Needed because the
/// keyboard highlight is a CSS class, not GTK focus, so the viewport's
/// scroll-to-focus machinery never triggers on its own.
fn scroll_into_view(widget: &impl IsA<gtk4::Widget>) {
    if let Some(viewport) = widget
        .ancestor(Viewport::static_type())
        .and_downcast::<Viewport>()
    {
        viewport.scroll_to(widget, None);
    }
}
