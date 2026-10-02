using Windows.Foundation;
using Windows.Graphics;
using Windows.System;
using CommunityToolkit.WinUI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Paloma.Models;
using Paloma.Views.Overlay;
using Paloma.Views.Overlay.Footer;
using Paloma.Views.Overlay.Query;
using Xunit;

namespace Paloma.UI.Tests;

[Collection(UiCollectionDefinition.Name)]
public sealed class OverlayViewTests(UiFixture ui)
{
    [Fact]
    public Task GivenStreamingStartsWhenPublishingDragRegionsShouldKeepTheStopButtonClickable() =>
        ui.RunAsync(async () =>
        {
            var (overlay, footer, published) = await HostAsync();

            await SetStreamingAsync(overlay, footer, true);

            Assert.Contains(published(), region => Covers(region, Bounds(footer, "StopButton")));
        });

    [Theory]
    [InlineData("ModelButton")]
    [InlineData("SettingsButton")]
    [InlineData("SessionsButton")]
    public Task GivenStreamingEndsWhenPublishingDragRegionsShouldKeepTheFooterButtonClickable(string button) =>
        ui.RunAsync(async () =>
        {
            var (overlay, footer, published) = await HostAsync();
            await SetStreamingAsync(overlay, footer, true);

            await SetStreamingAsync(overlay, footer, false);

            Assert.Contains(published(), region => Covers(region, Bounds(footer, button)));
        });

    [Fact]
    public Task GivenTextWhenPressingEscapeShouldClearIt() => ui.RunAsync(async () =>
    {
        var (_, query, input) = await FocusedAsync();
        query.Text = "hello";

        await ui.PressAsync(input, VirtualKey.Escape);

        Assert.Equal(string.Empty, query.Text);
    });

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public Task GivenEmptyInputWhenPressingEscapeShouldRequestHide(bool shift) => ui.RunAsync(async () =>
    {
        var (overlay, _, input) = await FocusedAsync();
        var requested = 0;
        overlay.HideRequested += () => requested++;

        await ui.PressAsync(input, VirtualKey.Escape, shift ? [VirtualKey.Shift] : []);

        Assert.Equal(1, requested);
    });

    [Fact]
    public Task GivenSearchModeWhenPressingOpenSessionsShouldSwitchToSessions() => ui.RunAsync(async () =>
    {
        var (overlay, _, input) = await FocusedAsync();

        await ui.PressAsync(input, VirtualKey.H, VirtualKey.Control);

        Assert.Equal(OverlayMode.Sessions, overlay.Mode);
    });

    [Fact]
    public Task GivenSessionsModeWhenPressingOpenSessionsShouldReturnToSearch() => ui.RunAsync(async () =>
    {
        var (overlay, _, input) = await FocusedAsync();
        await ui.PressAsync(input, VirtualKey.H, VirtualKey.Control);

        await ui.PressAsync(input, VirtualKey.H, VirtualKey.Control);

        Assert.Equal(OverlayMode.Search, overlay.Mode);
    });

    [Fact]
    public Task GivenSearchModeWhenPressingTheOpenSessionsKeyAloneShouldTypeIt() => ui.RunAsync(async () =>
    {
        var (overlay, query, input) = await FocusedAsync();

        await ui.PressAsync(input, VirtualKey.H);

        Assert.Equal((OverlayMode.Search, "h"), (overlay.Mode, query.Text));
    });

    private async Task<(OverlayView Overlay, FooterView Footer, Func<RectInt32[]> Published)> HostAsync()
    {
        var overlay = await ui.ShowAsync(new OverlayView { Width = 680, Height = 540 });
        RectInt32[] passthrough = [];
        overlay.DragRegionsChanged += (_, regions) => passthrough = regions;
        return (overlay, overlay.FindDescendant<FooterView>()!, () => passthrough);
    }

    private async Task<(OverlayView Overlay, QueryView Query, RichEditBox Input)> FocusedAsync()
    {
        var overlay = await ui.ShowAsync(new OverlayView { Width = 680, Height = 540 });
        var query = overlay.FindDescendant<QueryView>()!;
        query.FocusInput();
        await ui.IdleAsync();
        return (overlay, query, query.FindDescendant<RichEditBox>()!);
    }

    private async Task SetStreamingAsync(OverlayView overlay, FooterView footer, bool streaming)
    {
        footer.Streaming = streaming;
        overlay.UpdateLayout();
        await ui.IdleAsync();
    }

    private static bool Covers(RectInt32 region, RectInt32 inner)
    {
        return region.X <= inner.X
               && region.Y <= inner.Y
               && region.X + region.Width >= inner.X + inner.Width
               && region.Y + region.Height >= inner.Y + inner.Height;
    }

    private static RectInt32 Bounds(FooterView footer, string name)
    {
        var element = (FrameworkElement)footer.FindName(name);
        var scale = element.XamlRoot.RasterizationScale;
        var bounds = element.TransformToVisual(null).TransformBounds(
            new Rect(0, 0, element.ActualWidth, element.ActualHeight));
        return new RectInt32(
            (int)Math.Round(bounds.X * scale),
            (int)Math.Round(bounds.Y * scale),
            (int)Math.Round(bounds.Width * scale),
            (int)Math.Round(bounds.Height * scale));
    }
}