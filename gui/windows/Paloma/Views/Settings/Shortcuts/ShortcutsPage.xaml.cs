using Microsoft.UI.Xaml.Navigation;

namespace Paloma.Views.Settings.Shortcuts;

internal sealed record ShortcutEntry(string Keys, string Description);

public sealed partial class ShortcutsPage
{
    internal IReadOnlyList<ShortcutEntry> Search { get; }

    internal IReadOnlyList<ShortcutEntry> Chat { get; }

    internal IReadOnlyList<ShortcutEntry> Sessions { get; }

    public ShortcutsPage()
    {
        Search =
        [
            new ShortcutEntry("↑ ↓", "Move selection"),
            new ShortcutEntry("Enter", "Submit"),
            new ShortcutEntry("Ctrl+Enter", "Show actions"),
            new ShortcutEntry("Shift+↓", "Open sessions"),
            new ShortcutEntry("Esc", "Close overlay"),
        ];
        Chat =
        [
            new ShortcutEntry("Enter", "Send message"),
            new ShortcutEntry("Ctrl+C", "Interrupt response"),
            new ShortcutEntry("↑ ↓", "Move between pending decisions"),
            new ShortcutEntry("PgUp PgDn", "Scroll by page"),
            new ShortcutEntry("Ctrl+Home Ctrl+End", "Scroll to top / bottom"),
            new ShortcutEntry("Shift+↓", "Open sessions"),
            new ShortcutEntry("Esc", "Exit chat"),
        ];
        Sessions =
        [
            new ShortcutEntry("↑ ↓", "Move between sessions"),
            new ShortcutEntry("Enter", "Open session"),
            new ShortcutEntry("Del", "Delete session (Enter confirms)"),
            new ShortcutEntry("Esc", "Close"),
        ];
        NavigationCacheMode = NavigationCacheMode.Required;
        InitializeComponent();
    }
}