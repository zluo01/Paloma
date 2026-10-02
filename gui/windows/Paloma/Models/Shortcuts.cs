using Windows.System;
using Windows.Win32.UI.Input.KeyboardAndMouse;

namespace Paloma.Models;

internal static class Shortcuts
{
    private static readonly KeyChord Up = new(VirtualKey.Up);
    private static readonly KeyChord Down = new(VirtualKey.Down);
    private static readonly KeyChord Enter = new(VirtualKey.Enter);
    private static readonly KeyChord Escape = new(VirtualKey.Escape, AnyModifiers: true);

    public static Shortcut OpenSessions { get; } = new("Open sessions", new KeyChord(VirtualKey.H, HOT_KEY_MODIFIERS.MOD_CONTROL));

    public static Shortcut NewLine { get; } = new("New line", new KeyChord(VirtualKey.Enter, HOT_KEY_MODIFIERS.MOD_SHIFT));

    public static Shortcut MoveSelection { get; } = new("Move selection", Up, Down);

    public static Shortcut Submit { get; } = new("Submit", Enter);

    public static Shortcut ShowActions { get; } = new("Show actions", new KeyChord(VirtualKey.Enter, HOT_KEY_MODIFIERS.MOD_CONTROL));

    public static Shortcut CloseOverlay { get; } = new("Close overlay", Escape);

    public static Shortcut SendMessage { get; } = new("Send message", Enter);

    public static Shortcut Interrupt { get; } = new("Interrupt response", new KeyChord(VirtualKey.C, HOT_KEY_MODIFIERS.MOD_CONTROL));

    public static Shortcut MoveBetweenDecisions { get; } = new("Move between pending decisions", Up, Down);

    public static Shortcut ScrollByPage { get; } = new("Scroll by page", new KeyChord(VirtualKey.PageUp), new KeyChord(VirtualKey.PageDown));

    public static Shortcut ScrollToEdge { get; } = new(
        "Scroll to top / bottom",
        new KeyChord(VirtualKey.Home, HOT_KEY_MODIFIERS.MOD_CONTROL),
        new KeyChord(VirtualKey.End, HOT_KEY_MODIFIERS.MOD_CONTROL));

    public static Shortcut ExitChat { get; } = new("Exit chat", Escape);

    public static Shortcut MoveBetweenSessions { get; } = new("Move between sessions", Up, Down);

    public static Shortcut OpenSession { get; } = new("Open session", Enter);

    public static Shortcut DeleteSession { get; } = new("Delete session (Enter confirms)", new KeyChord(VirtualKey.Delete));

    public static Shortcut CloseSessions { get; } = new("Close", Escape);

    public static IReadOnlyList<Shortcut> Global { get; } = [OpenSessions, NewLine];

    public static IReadOnlyList<Shortcut> Search { get; } = [MoveSelection, Submit, ShowActions, CloseOverlay];

    public static IReadOnlyList<Shortcut> Chat { get; } =
        [SendMessage, Interrupt, MoveBetweenDecisions, ScrollByPage, ScrollToEdge, ExitChat];

    public static IReadOnlyList<Shortcut> Sessions { get; } = [MoveBetweenSessions, OpenSession, DeleteSession, CloseSessions];
}
