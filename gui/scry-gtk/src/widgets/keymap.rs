//! Binding table shared by Shortcuts settings and overlay dispatch. It owns
//! binding identity, display grouping, dispatch context, and accepted
//! accelerators; contextual guards stay in dispatch code.

use gtk4::gdk::{Key, ModifierType};

pub(in crate::widgets) type Accel = (Key, ModifierType);

/// How an accelerator is matched against a key event.
#[derive(Clone, Copy)]
pub(in crate::widgets) enum Match {
    /// Match on the keyval alone; modifiers are ignored.
    KeyOnly,
    /// Match the keyval and require the accel's modifiers to be present.
    Contains,
}

#[derive(Clone, Copy)]
pub(in crate::widgets) struct Chord {
    pub(in crate::widgets) accel: Accel,
    pub(in crate::widgets) policy: Match,
}

/// Display grouping on the Shortcuts page.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::widgets) enum Group {
    Search,
    ActionPanel,
    Chat,
    Sessions,
}

/// Dispatch context, mirroring `keys.rs` precedence. A binding's context can
/// differ from its display group (e.g. `OpenSessions` shows under Search but is
/// matched globally, before the mode split).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::widgets) enum Context {
    Panel,
    Sessions,
    Global,
    Local,
    Chat,
}

/// Stable identity of one display row, the join key between the page and dispatch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::widgets) enum BindingId {
    SearchMove,
    SearchOpen,
    SearchShowActions,
    OpenSessions,
    SearchClose,
    PanelMove,
    PanelRun,
    PanelClose,
    ChatSend,
    ChatInterrupt,
    ChatMovePrompt,
    ChatExit,
    SessionMove,
    SessionOpen,
    SessionDelete,
    SessionClose,
}

pub(in crate::widgets) struct Binding {
    pub(in crate::widgets) id: BindingId,
    pub(in crate::widgets) group: Group,
    pub(in crate::widgets) context: Context,
    pub(in crate::widgets) label: &'static str,
    /// Accelerators rendered on the page.
    pub(in crate::widgets) shown: &'static [Chord],
    /// Accelerators accepted by dispatch but never rendered (keypad aliases,
    /// `Shift+Left`).
    pub(in crate::widgets) hidden: &'static [Chord],
}

const fn key(k: Key) -> Chord {
    Chord {
        accel: (k, ModifierType::empty()),
        policy: Match::KeyOnly,
    }
}

const fn ctrl(k: Key) -> Chord {
    Chord {
        accel: (k, ModifierType::CONTROL_MASK),
        policy: Match::Contains,
    }
}

const fn shift(k: Key) -> Chord {
    Chord {
        accel: (k, ModifierType::SHIFT_MASK),
        policy: Match::Contains,
    }
}

/// Every documented binding, ordered by display group and row.
const ALL: &[Binding] = &[
    Binding {
        id: BindingId::SearchMove,
        group: Group::Search,
        context: Context::Local,
        label: "Move selection",
        shown: &[key(Key::Up), key(Key::Down)],
        hidden: &[],
    },
    Binding {
        id: BindingId::SearchOpen,
        group: Group::Search,
        context: Context::Local,
        label: "Open result or start chat",
        shown: &[key(Key::Return)],
        hidden: &[key(Key::KP_Enter)],
    },
    Binding {
        id: BindingId::SearchShowActions,
        group: Group::Search,
        context: Context::Local,
        label: "Show actions",
        shown: &[ctrl(Key::k)],
        hidden: &[],
    },
    Binding {
        id: BindingId::OpenSessions,
        group: Group::Search,
        context: Context::Global,
        label: "Open sessions",
        shown: &[shift(Key::Right)],
        hidden: &[],
    },
    Binding {
        id: BindingId::SearchClose,
        group: Group::Search,
        context: Context::Local,
        label: "Close overlay",
        shown: &[key(Key::Escape)],
        hidden: &[],
    },
    Binding {
        id: BindingId::PanelMove,
        group: Group::ActionPanel,
        context: Context::Panel,
        label: "Move between actions",
        shown: &[key(Key::Up), key(Key::Down)],
        hidden: &[],
    },
    Binding {
        id: BindingId::PanelRun,
        group: Group::ActionPanel,
        context: Context::Panel,
        label: "Run action",
        shown: &[key(Key::Return)],
        hidden: &[key(Key::KP_Enter)],
    },
    Binding {
        id: BindingId::PanelClose,
        group: Group::ActionPanel,
        context: Context::Panel,
        label: "Close",
        shown: &[key(Key::Escape)],
        hidden: &[],
    },
    Binding {
        id: BindingId::ChatSend,
        group: Group::Chat,
        context: Context::Chat,
        label: "Send message",
        shown: &[key(Key::Return)],
        hidden: &[key(Key::KP_Enter)],
    },
    Binding {
        id: BindingId::ChatInterrupt,
        group: Group::Chat,
        context: Context::Chat,
        label: "Interrupt response",
        shown: &[ctrl(Key::c)],
        hidden: &[],
    },
    Binding {
        id: BindingId::ChatMovePrompt,
        group: Group::Chat,
        context: Context::Chat,
        label: "Move between prompts",
        shown: &[key(Key::Up), key(Key::Down)],
        hidden: &[],
    },
    Binding {
        id: BindingId::ChatExit,
        group: Group::Chat,
        context: Context::Chat,
        label: "Exit chat",
        shown: &[key(Key::Escape)],
        hidden: &[],
    },
    Binding {
        id: BindingId::SessionMove,
        group: Group::Sessions,
        context: Context::Sessions,
        label: "Move between sessions",
        shown: &[key(Key::Up), key(Key::Down)],
        hidden: &[],
    },
    Binding {
        id: BindingId::SessionOpen,
        group: Group::Sessions,
        context: Context::Sessions,
        label: "Open session",
        shown: &[key(Key::Return)],
        hidden: &[key(Key::KP_Enter)],
    },
    Binding {
        id: BindingId::SessionDelete,
        group: Group::Sessions,
        context: Context::Sessions,
        label: "Delete session",
        shown: &[key(Key::Delete)],
        hidden: &[key(Key::KP_Delete)],
    },
    Binding {
        id: BindingId::SessionClose,
        group: Group::Sessions,
        context: Context::Sessions,
        label: "Close",
        shown: &[key(Key::Escape)],
        hidden: &[shift(Key::Left)],
    },
];

fn chord_matches(c: Chord, k: Key, mods: ModifierType) -> bool {
    c.accel.0 == k
        && match c.policy {
            Match::KeyOnly => true,
            Match::Contains => mods.contains(c.accel.1),
        }
}

/// Bindings in a display group, in page order.
pub(in crate::widgets) fn group_bindings(group: Group) -> impl Iterator<Item = &'static Binding> {
    ALL.iter().filter(move |b| b.group == group)
}

/// The binding a key event resolves to within a dispatch context, scanning both
/// shown and hidden accelerators. `None` means the key is unbound there.
pub(in crate::widgets) fn match_binding(
    ctx: Context,
    k: Key,
    mods: ModifierType,
) -> Option<BindingId> {
    ALL.iter()
        .filter(|b| b.context == ctx)
        .find(|b| {
            b.shown
                .iter()
                .chain(b.hidden)
                .any(|c| chord_matches(*c, k, mods))
        })
        .map(|b| b.id)
}

#[cfg(test)]
mod tests {
    use gtk4::gdk::{Key, ModifierType};

    use super::*;

    #[test]
    fn keyonly_matches_its_key() {
        assert!(chord_matches(key(Key::Up), Key::Up, ModifierType::empty()));
    }

    #[test]
    fn keyonly_ignores_modifiers() {
        assert!(chord_matches(
            key(Key::Up),
            Key::Up,
            ModifierType::SHIFT_MASK
        ));
    }

    #[test]
    fn keyonly_rejects_a_different_key() {
        assert!(!chord_matches(
            key(Key::Up),
            Key::Down,
            ModifierType::empty()
        ));
    }

    #[test]
    fn contains_matches_with_its_modifier() {
        assert!(chord_matches(
            ctrl(Key::k),
            Key::k,
            ModifierType::CONTROL_MASK
        ));
    }

    #[test]
    fn contains_rejects_without_its_modifier() {
        assert!(!chord_matches(ctrl(Key::k), Key::k, ModifierType::empty()));
    }

    #[test]
    fn every_declared_chord_resolves_to_its_own_binding() {
        for b in ALL {
            for c in b.shown.iter().chain(b.hidden) {
                let mods = match c.policy {
                    Match::KeyOnly => ModifierType::empty(),
                    Match::Contains => c.accel.1,
                };
                assert_eq!(
                    match_binding(b.context, c.accel.0, mods),
                    Some(b.id),
                    "{:?} chord did not resolve to its own binding",
                    b.id
                );
            }
        }
    }

    #[test]
    fn no_two_bindings_share_a_keyval_within_a_context() {
        // Under KeyOnly/Contains semantics two chords on the same keyval can
        // always be triggered by one event (press the union of their modifiers),
        // so a shared keyval within a context is an ambiguous overlap.
        for ctx in [
            Context::Panel,
            Context::Sessions,
            Context::Global,
            Context::Local,
            Context::Chat,
        ] {
            let mut seen: Vec<(Key, BindingId)> = Vec::new();
            for b in ALL.iter().filter(|b| b.context == ctx) {
                for c in b.shown.iter().chain(b.hidden) {
                    if let Some((_, other)) = seen.iter().find(|(k, _)| *k == c.accel.0) {
                        panic!("{:?} and {:?} share a key in the same context", other, b.id);
                    }
                    seen.push((c.accel.0, b.id));
                }
            }
        }
    }

    #[test]
    fn keyonly_binding_matches_with_extra_modifier() {
        assert_eq!(
            match_binding(Context::Local, Key::Up, ModifierType::CONTROL_MASK),
            Some(BindingId::SearchMove)
        );
    }

    #[test]
    fn contains_binding_needs_its_modifier() {
        assert_eq!(
            match_binding(Context::Local, Key::k, ModifierType::empty()),
            None
        );
    }

    #[test]
    fn unbound_key_resolves_to_none() {
        assert_eq!(
            match_binding(Context::Local, Key::F1, ModifierType::empty()),
            None
        );
    }

    #[test]
    fn kp_enter_is_a_hidden_alias_of_open() {
        assert_eq!(
            match_binding(Context::Local, Key::KP_Enter, ModifierType::empty()),
            Some(BindingId::SearchOpen)
        );
    }

    #[test]
    fn kp_delete_is_a_hidden_alias_of_delete() {
        assert_eq!(
            match_binding(Context::Sessions, Key::KP_Delete, ModifierType::empty()),
            Some(BindingId::SessionDelete)
        );
    }

    #[test]
    fn shift_left_is_a_hidden_alias_of_close_sessions() {
        assert_eq!(
            match_binding(Context::Sessions, Key::Left, ModifierType::SHIFT_MASK),
            Some(BindingId::SessionClose)
        );
    }

    #[test]
    fn unbound_key_in_panel_resolves_to_none() {
        assert_eq!(
            match_binding(Context::Panel, Key::F1, ModifierType::empty()),
            None
        );
    }

    #[test]
    fn plain_left_in_sessions_resolves_to_none() {
        assert_eq!(
            match_binding(Context::Sessions, Key::Left, ModifierType::empty()),
            None
        );
    }
}
