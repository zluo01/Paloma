using Windows.Graphics;
using Microsoft.UI.Windowing;
using Paloma.Views.Settings;
using Xunit;

namespace Paloma.UI.Tests;

[Collection(UiCollectionDefinition.Name)]
public sealed partial class SettingsWindowTests(UiFixture ui)
{
    private static readonly PointInt32 OffScreen = new(-30000, -30000);

    [Fact]
    public Task GivenPageWhenLoadedShouldLoadOnce() => ui.RunAsync(async () =>
    {
        var page = await ui.ShowAsync(new CountingPage());
        await SettleAsync();

        Assert.Equal(1, page.Loads);
    });

    [Fact]
    public Task GivenShownPageWhenTheWindowIsHiddenAndShownAgainShouldReloadOnce() => ui.RunAsync(async () =>
    {
        var (page, window) = await ShownAsync();
        try
        {
            var shown = page.Loads;
            window.Hide();
            await SettleAsync();
            var hidden = page.Loads;

            window.Show(false);
            await SettleAsync();

            Assert.Equal((shown, shown + 1), (hidden, page.Loads));
        }
        finally
        {
            window.Hide();
        }
    });

    [Fact]
    public Task GivenShownPageWhenTheWindowIsResizedShouldNotReload() => ui.RunAsync(async () =>
    {
        var (page, window) = await ShownAsync();
        try
        {
            var shown = page.Loads;

            window.ResizeClient(new SizeInt32(600, 500));
            await SettleAsync();

            Assert.Equal(shown, page.Loads);
        }
        finally
        {
            window.Hide();
        }
    });

    [Fact]
    public Task GivenLoadStillRunningWhenTheWindowIsResizedShouldNotLoadAgain() => ui.RunAsync(async () =>
    {
        var (page, window) = await ShownAsync();
        var loading = new TaskCompletionSource();
        try
        {
            window.Hide();
            await SettleAsync();
            page.Pending = loading.Task;
            var hidden = page.Loads;

            window.Show(false);
            await SettleAsync();
            window.ResizeClient(new SizeInt32(window.ClientSize.Width + 40, window.ClientSize.Height + 40));
            await SettleAsync();

            Assert.Equal(hidden + 1, page.Loads);
        }
        finally
        {
            loading.SetResult();
            window.Hide();
        }
    });

    [Fact]
    public Task GivenWindowHiddenWhileLoadingWhenShownAgainShouldReloadOnce() => ui.RunAsync(async () =>
    {
        var (page, window) = await ShownAsync();
        var loading = new TaskCompletionSource();
        try
        {
            window.Hide();
            await SettleAsync();
            page.Pending = loading.Task;
            window.Show(false);
            await SettleAsync();
            window.Hide();
            await SettleAsync();
            loading.SetResult();
            page.Pending = Task.CompletedTask;
            await SettleAsync();
            var hidden = page.Loads;

            window.Show(false);
            await SettleAsync();

            Assert.Equal(hidden + 1, page.Loads);
        }
        finally
        {
            loading.TrySetResult();
            window.Hide();
        }
    });

    private async Task<(CountingPage Page, AppWindow Window)> ShownAsync()
    {
        var page = await ui.ShowAsync(new CountingPage());
        var window = AppWindow.GetFromWindowId(page.XamlRoot.ContentIslandEnvironment.AppWindowId);
        window.Move(OffScreen);
        window.Show(false);
        await SettleAsync();
        return (page, window);
    }

    private async Task SettleAsync()
    {
        for (var i = 0; i < 5; i++)
        {
            await Task.Delay(60);
            await ui.IdleAsync();
        }
    }

    private sealed partial class CountingPage : RefreshOnVisiblePage
    {
        public int Loads { get; private set; }

        public Task Pending { get; set; } = Task.CompletedTask;

        protected override Task LoadAsync()
        {
            Loads++;
            return Pending;
        }
    }
}