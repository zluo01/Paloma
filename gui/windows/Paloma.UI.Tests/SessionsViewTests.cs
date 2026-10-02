using CommunityToolkit.WinUI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Paloma.ViewModels.Overlay;
using Paloma.Views.Overlay.Sessions;
using Paloma.Views.Overlay.Shared;
using Xunit;
using SessionListItem = PalomaCore.SessionListItem;

namespace Paloma.UI.Tests;

[Collection(UiCollectionDefinition.Name)]
public sealed class SessionsViewTests(UiFixture ui)
{
    [Theory]
    [InlineData("line 1\rline 2\rline 3")]
    [InlineData("line 1\vline 2\vline 3")]
    [InlineData("line 1\nline 2\nline 3")]
    public Task GivenMultiLineTitleWhenShownShouldKeepTheRowAsTallAsASingleLineOne(string title) =>
        ui.RunAsync(async () =>
        {
            var items = await ShowAsync("single line title", title);

            Assert.Equal(RowHeight(items, 0), RowHeight(items, 1));
        });

    [Theory]
    [InlineData("single line title", "single line title")]
    [InlineData("line 1\rline 2", "line 1")]
    [InlineData("line 1\vline 2", "line 1")]
    [InlineData("line 1\nline 2", "line 1")]
    [InlineData("line 1\r\nline 2", "line 1")]
    [InlineData("  line 1  \nline 2", "line 1")]
    public Task GivenTitleWhenShownShouldShowOnlyItsFirstLine(string title, string shown) =>
        ui.RunAsync(async () =>
        {
            var items = await ShowAsync(title);

            var label = Row(items, 0).FindDescendants().OfType<TextBlock>()
                .First(text => text.TextTrimming == TextTrimming.CharacterEllipsis);
            Assert.Equal(shown, label.Text);
        });

    [Fact]
    public Task GivenMultiLineTitleWhenShownShouldKeepTheFullTitleInTheTooltip() => ui.RunAsync(async () =>
    {
        var items = await ShowAsync("line 1\rline 2");

        Assert.Equal("line 1\rline 2", ToolTipService.GetToolTip(Row(items, 0).FindDescendantOrSelf<RowItem>()));
    });

    private async Task<ItemsControl> ShowAsync(params string[] titles)
    {
        var sessions = await ui.ShowAsync(new SessionsView { Width = 680 });
        foreach (var title in titles)
        {
            sessions.ViewModel.Rows.Add(new SessionRow(new SessionListItem(Guid.NewGuid().ToString(), title, 0)));
        }

        sessions.UpdateLayout();
        await ui.IdleAsync();
        return sessions.FindDescendant<ItemsControl>()!;
    }

    private static FrameworkElement Row(ItemsControl items, int index)
    {
        return (FrameworkElement)items.ContainerFromIndex(index);
    }

    private static double RowHeight(ItemsControl items, int index)
    {
        return Row(items, index).ActualHeight;
    }
}