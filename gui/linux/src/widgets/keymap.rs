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
    /// Match on the keyval and exact modifiers.
    Exact,
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
    Chat,
    Sessions,
}

/// Dispatch context; `keys.rs` checks `Global` first, then the active mode's
/// context. A binding's context can differ from its display group (e.g.
/// `OpenSessions` shows under Search but is matched globally, before the mode
/// split).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::widgets) enum Context {
    Sessions,
    Global,
    Search,
    Chat,
}

/// Stable identity of one display row, the join key between the page and dispatch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::widgets) enum BindingId {
    SearchMove,
    SearchSubmit,
    SearchShowActions,
    OpenSessions,
    SearchClose,
    ChatSend,
    ChatInterrupt,
    ChatMovePrompt,
    ChatScrollPage,
    ChatScrollEdge,
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
    /// Accelerators accepted by dispatch but never rendered (keypad aliases).
    pub(in crate::widgets) hidden: &'static [Chord],
}

const fn key(k: Key) -> Chord {
    Chord {
        accel: (k, ModifierType::empty()),
        policy: Match::KeyOnly,
    }
}

const fn plain(k: Key) -> Chord {
    Chord {
        accel: (k, ModifierType::empty()),
        policy: Match::Exact,
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
        context: Context::Search,
        label: "Move selection",
        shown: &[plain(Key::Up), plain(Key::Down)],
        hidden: &[plain(Key::KP_Up), plain(Key::KP_Down)],
    },
    Binding {
        id: BindingId::SearchSubmit,
        group: Group::Search,
        context: Context::Search,
        label: "Submit",
        shown: &[plain(Key::Return)],
        hidden: &[plain(Key::KP_Enter)],
    },
    Binding {
        id: BindingId::SearchShowActions,
        group: Group::Search,
        context: Context::Search,
        label: "Show actions",
        shown: &[ctrl(Key::Return)],
        hidden: &[ctrl(Key::KP_Enter)],
    },
    Binding {
        id: BindingId::OpenSessions,
        group: Group::Search,
        context: Context::Global,
        label: "Open sessions",
        shown: &[shift(Key::Down)],
        hidden: &[],
    },
    Binding {
        id: BindingId::SearchClose,
        group: Group::Search,
        context: Context::Search,
        label: "Close overlay",
        shown: &[key(Key::Escape)],
        hidden: &[],
    },
    Binding {
        id: BindingId::ChatSend,
        group: Group::Chat,
        context: Context::Chat,
        label: "Send message",
        shown: &[plain(Key::Return)],
        hidden: &[plain(Key::KP_Enter)],
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
        shown: &[plain(Key::Up), plain(Key::Down)],
        hidden: &[plain(Key::KP_Up), plain(Key::KP_Down)],
    },
    Binding {
        id: BindingId::ChatScrollPage,
        group: Group::Chat,
        context: Context::Chat,
        label: "Scroll by page",
        shown: &[plain(Key::Page_Up), plain(Key::Page_Down)],
        hidden: &[plain(Key::KP_Page_Up), plain(Key::KP_Page_Down)],
    },
    Binding {
        id: BindingId::ChatScrollEdge,
        group: Group::Chat,
        context: Context::Chat,
        label: "Scroll to top / bottom",
        shown: &[ctrl(Key::Home), ctrl(Key::End)],
        hidden: &[ctrl(Key::KP_Home), ctrl(Key::KP_End)],
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
        shown: &[plain(Key::Up), plain(Key::Down)],
        hidden: &[plain(Key::KP_Up), plain(Key::KP_Down)],
    },
    Binding {
        id: BindingId::SessionOpen,
        group: Group::Sessions,
        context: Context::Sessions,
        label: "Open session",
        shown: &[plain(Key::Return)],
        hidden: &[plain(Key::KP_Enter)],
    },
    Binding {
        id: BindingId::SessionDelete,
        group: Group::Sessions,
        context: Context::Sessions,
        label: "Delete session",
        shown: &[plain(Key::Delete)],
        hidden: &[plain(Key::KP_Delete)],
    },
    Binding {
        id: BindingId::SessionClose,
        group: Group::Sessions,
        context: Context::Sessions,
        label: "Close",
        shown: &[key(Key::Escape)],
        hidden: &[],
    },
];

fn chord_matches(c: Chord, k: Key, mods: ModifierType) -> bool {
    c.accel.0 == k
        && match c.policy {
            Match::KeyOnly => true,
            Match::Exact => mods == c.accel.1,
            Match::Contains => mods.contains(c.accel.1),
        }
}

/// Bindings in a display group, in page order.
pub(in crate::widgets) fn group_bindings(group: Group) -> impl Iterator<Item = &'static Binding> {
    ALL.iter().filter(move |b| b.group == group)
}

pub(in crate::widgets) fn binding(id: BindingId) -> &'static Binding {
    ALL.iter()
        .find(|b| b.id == id)
        .expect("every BindingId has a row in ALL")
}

/// Modifiers that distinguish chords; lock and pointer-button state must not.
const ACCEL_MODS: ModifierType = ModifierType::SHIFT_MASK
    .union(ModifierType::CONTROL_MASK)
    .union(ModifierType::ALT_MASK)
    .union(ModifierType::SUPER_MASK)
    .union(ModifierType::HYPER_MASK)
    .union(ModifierType::META_MASK);

/// The binding a key event resolves to within a dispatch context, scanning both
/// shown and hidden accelerators. `None` means the key is unbound there.
pub(in crate::widgets) fn match_binding(
    ctx: Context,
    k: Key,
    mods: ModifierType,
) -> Option<BindingId> {
    let mods = mods.intersection(ACCEL_MODS);
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
                    Match::Exact => c.accel.1,
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

    fn chords_overlap(a: Chord, b: Chord) -> bool {
        if a.accel.0 != b.accel.0 {
            return false;
        }

        match (a.policy, b.policy) {
            (Match::KeyOnly, _) | (_, Match::KeyOnly) => true,
            (Match::Exact, Match::Exact) => a.accel.1 == b.accel.1,
            (Match::Exact, Match::Contains) => a.accel.1.contains(b.accel.1),
            (Match::Contains, Match::Exact) => b.accel.1.contains(a.accel.1),
            (Match::Contains, Match::Contains) => true,
        }
    }

    #[test]
    fn no_two_bindings_overlap_within_a_context() {
        for ctx in [
            Context::Sessions,
            Context::Global,
            Context::Search,
            Context::Chat,
        ] {
            let bindings: Vec<_> = ALL.iter().filter(|b| b.context == ctx).collect();
            for (i, a) in bindings.iter().enumerate() {
                for b in bindings.iter().skip(i + 1) {
                    for ac in a.shown.iter().chain(a.hidden) {
                        for bc in b.shown.iter().chain(b.hidden) {
                            assert!(
                                !chords_overlap(*ac, *bc),
                                "{:?} and {:?} have overlapping chords in the same context",
                                a.id,
                                b.id
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn modified_arrows_resolve_to_none() {
        assert_eq!(
            match_binding(Context::Search, Key::Up, ModifierType::CONTROL_MASK),
            None
        );
        assert_eq!(
            match_binding(Context::Chat, Key::Up, ModifierType::SHIFT_MASK),
            None
        );
        assert_eq!(
            match_binding(Context::Sessions, Key::Down, ModifierType::SHIFT_MASK),
            None
        );
    }

    #[test]
    fn lock_state_does_not_affect_matching() {
        assert_eq!(
            match_binding(Context::Chat, Key::Return, ModifierType::LOCK_MASK),
            Some(BindingId::ChatSend)
        );
        assert_eq!(
            match_binding(Context::Search, Key::Down, ModifierType::LOCK_MASK),
            Some(BindingId::SearchMove)
        );
    }

    #[test]
    fn global_chords_do_not_shadow_context_bindings() {
        for g in ALL.iter().filter(|b| b.context == Context::Global) {
            for c in g.shown.iter().chain(g.hidden) {
                for ctx in [Context::Sessions, Context::Search, Context::Chat] {
                    assert_eq!(
                        match_binding(ctx, c.accel.0, c.accel.1),
                        None,
                        "global {:?} chord shadows a {ctx:?} binding",
                        g.id
                    );
                }
            }
        }
    }

    #[test]
    fn submit_rejects_extra_modifier() {
        assert_eq!(
            match_binding(Context::Search, Key::Return, ModifierType::SHIFT_MASK),
            None
        );
        assert_eq!(
            match_binding(Context::Chat, Key::Return, ModifierType::SHIFT_MASK),
            None
        );
        assert_eq!(
            match_binding(Context::Sessions, Key::Return, ModifierType::CONTROL_MASK),
            None
        );
    }

    #[test]
    fn contains_binding_needs_its_modifier() {
        assert_eq!(
            match_binding(Context::Search, Key::k, ModifierType::empty()),
            None
        );
    }

    #[test]
    fn unbound_key_resolves_to_none() {
        assert_eq!(
            match_binding(Context::Search, Key::F1, ModifierType::empty()),
            None
        );
    }

    #[test]
    fn kp_enter_is_a_hidden_alias_of_open() {
        assert_eq!(
            match_binding(Context::Search, Key::KP_Enter, ModifierType::empty()),
            Some(BindingId::SearchSubmit)
        );
    }

    #[test]
    fn modified_delete_resolves_to_none() {
        assert_eq!(
            match_binding(Context::Sessions, Key::Delete, ModifierType::CONTROL_MASK),
            None
        );
        assert_eq!(
            match_binding(Context::Sessions, Key::Delete, ModifierType::SHIFT_MASK),
            None
        );
        assert_eq!(
            match_binding(Context::Sessions, Key::KP_Delete, ModifierType::SHIFT_MASK),
            None
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
    fn kp_arrows_are_hidden_aliases_of_move() {
        assert_eq!(
            match_binding(Context::Search, Key::KP_Up, ModifierType::empty()),
            Some(BindingId::SearchMove)
        );
        assert_eq!(
            match_binding(Context::Chat, Key::KP_Down, ModifierType::empty()),
            Some(BindingId::ChatMovePrompt)
        );
        assert_eq!(
            match_binding(Context::Sessions, Key::KP_Down, ModifierType::empty()),
            Some(BindingId::SessionMove)
        );
    }

    #[test]
    fn left_in_sessions_resolves_to_none() {
        assert_eq!(
            match_binding(Context::Sessions, Key::Left, ModifierType::SHIFT_MASK),
            None
        );
        assert_eq!(
            match_binding(Context::Sessions, Key::Left, ModifierType::empty()),
            None
        );
    }
}
