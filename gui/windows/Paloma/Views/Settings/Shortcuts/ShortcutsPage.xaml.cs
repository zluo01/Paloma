using Microsoft.UI.Xaml.Navigation;

namespace Paloma.Views.Settings.Shortcuts;

internal sealed record ShortcutEntry(string Description, IReadOnlyList<string> Keys);

public sealed partial class ShortcutsPage
{
    internal IReadOnlyList<ShortcutEntry> Search { get; }

    internal IReadOnlyList<ShortcutEntry> Chat { get; }

    internal IReadOnlyList<ShortcutEntry> Sessions { get; }

    public ShortcutsPage()
    {
        Search =
        [
            new("Move selection", ["↑", "↓"]),
            new("Submit", ["Enter"]),
            new("Show actions", ["Ctrl", "Enter"]),
            new("Open sessions", ["Shift", "↓"]),
            new("New line", ["Shift", "Enter"]),
            new("Close overlay", ["Esc"]),
        ];
        Chat =
        [
            new("Send message", ["Enter"]),
            new("Interrupt response", ["Ctrl", "C"]),
            new("Move between pending decisions", ["↑", "↓"]),
            new("Scroll by page", ["PgUp", "PgDn"]),
            new("Scroll to top / bottom", ["Ctrl", "Home", "Ctrl", "End"]),
            new("Open sessions", ["Shift", "↓"]),
            new("Exit chat", ["Esc"]),
        ];
        Sessions =
        [
            new("Move between sessions", ["↑", "↓"]),
            new("Open session", ["Enter"]),
            new("Delete session (Enter confirms)", ["Del"]),
            new("Close", ["Esc"]),
        ];
        NavigationCacheMode = NavigationCacheMode.Required;
        InitializeComponent();
    }
}
