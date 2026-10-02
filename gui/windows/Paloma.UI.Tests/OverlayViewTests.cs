using Windows.Foundation;
using Windows.Graphics;
using CommunityToolkit.WinUI;
using Microsoft.UI.Xaml;
using Paloma.Views.Overlay;
using Paloma.Views.Overlay.Footer;
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

    private async Task<(OverlayView Overlay, FooterView Footer, Func<RectInt32[]> Published)> HostAsync()
    {
        var overlay = await ui.ShowAsync(new OverlayView { Width = 680, Height = 540 });
        RectInt32[] passthrough = [];
        overlay.DragRegionsChanged += (_, regions) => passthrough = regions;
        return (overlay, overlay.FindDescendant<FooterView>()!, () => passthrough);
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