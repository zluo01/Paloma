using CommunityToolkit.WinUI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Paloma.Models;
using Paloma.Views.Overlay.Footer;
using Xunit;

namespace Paloma.UI.Tests;

[Collection(UiCollectionDefinition.Name)]
public sealed class FooterViewTests(UiFixture ui)
{
    [Theory]
    [InlineData("Sessions", "Ctrl+H")]
    [InlineData("Stop", "Ctrl+C")]
    public Task GivenFooterWhenShownShouldShowTheShortcutInTheButtonToolTip(string button, string toolTip) =>
        ui.RunAsync(async () =>
        {
            var footer = await ui.ShowAsync(new FooterView());

            var target = footer.FindDescendants().OfType<Button>().Single(b => AutomationProperties.GetName(b) == button);

            Assert.Equal(toolTip, ToolTipService.GetToolTip(target));
        });

    [Theory]
    [InlineData(OverlayMode.Search, true, new[] { "Submit: Enter", "Show actions: Ctrl Enter" })]
    [InlineData(OverlayMode.Search, false, new string[0])]
    [InlineData(OverlayMode.Chat, false, new[] { "Scroll by page: PgUp PgDn" })]
    [InlineData(OverlayMode.Sessions, false, new[] { "Open session: Enter", "Delete session: Del" })]
    public Task GivenModeWhenShownShouldShowItsHintsWithTheirKeys(OverlayMode mode, bool hasResults, string[] expected) =>
        ui.RunAsync(async () =>
        {
            var footer = await ui.ShowAsync(new FooterView());
            footer.Mode = mode;
            footer.HasResults = hasResults;
            footer.UpdateLayout();
            await ui.IdleAsync();
            var hint = (Style)footer.Resources["FooterHint"];
            var keyCap = (Style)Application.Current.Resources["KeyCap"];

            var hints = footer.FindDescendants().OfType<TextBlock>()
                .Where(text => text.Style == hint && IsShown(text, footer))
                .Select(text =>
                {
                    var keys = ((FrameworkElement)text.Parent).FindDescendants().OfType<ContentControl>()
                        .Where(key => key.Style == keyCap);
                    return $"{text.Text}: {string.Join(" ", keys.Select(key => key.Content))}";
                });

            Assert.Equal(expected, hints);
        });

    private static bool IsShown(FrameworkElement element, FrameworkElement root)
    {
        for (var current = element; current != root; current = (FrameworkElement)VisualTreeHelper.GetParent(current))
        {
            if (current.Visibility != Visibility.Visible)
            {
                return false;
            }
        }

        return true;
    }
}
