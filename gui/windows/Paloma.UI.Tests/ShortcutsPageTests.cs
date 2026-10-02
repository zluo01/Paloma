using CommunityToolkit.WinUI;
using CommunityToolkit.WinUI.Controls;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Paloma.Views.Settings.Shortcuts;
using Xunit;

namespace Paloma.UI.Tests;

[Collection(UiCollectionDefinition.Name)]
public sealed class ShortcutsPageTests(UiFixture ui)
{
    [Fact]
    public Task GivenPageWhenShownShouldListEveryShortcutWithItsKeys() => ui.RunAsync(async () =>
    {
        var page = await ui.ShowAsync(new ShortcutsPage());
        var sectionHeader = (Style)Application.Current.Resources["SettingsSectionHeader"];
        var keyCap = (Style)Application.Current.Resources["KeyCap"];

        var lines = new List<string>();
        foreach (var element in page.FindDescendants())
        {
            switch (element)
            {
                case TextBlock text when text.Style == sectionHeader:
                    lines.Add(text.Text);
                    break;
                case SettingsCard card:
                    var keys = card.FindDescendants().OfType<ContentControl>().Where(key => key.Style == keyCap);
                    lines.Add($"{card.Header}: {string.Join(" ", keys.Select(key => key.Content))}");
                    break;
            }
        }

        Assert.Equal(
        [
            "Global",
            "Open sessions: Ctrl H",
            "New line: Shift Enter",
            "Search",
            "Move selection: ↑ ↓",
            "Submit: Enter",
            "Show actions: Ctrl Enter",
            "Close overlay: Esc",
            "Chat",
            "Send message: Enter",
            "Interrupt response: Ctrl C",
            "Move between pending decisions: ↑ ↓",
            "Scroll by page: PgUp PgDn",
            "Scroll to top / bottom: Ctrl Home Ctrl End",
            "Exit chat: Esc",
            "Sessions",
            "Move between sessions: ↑ ↓",
            "Open session: Enter",
            "Delete session (Enter confirms): Del",
            "Close: Esc",
        ], lines);
    });
}
