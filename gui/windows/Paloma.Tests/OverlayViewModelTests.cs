using System.Collections.Concurrent;
using CommunityToolkit.Mvvm.Messaging;
using Paloma.Messages;
using Paloma.ViewModels.Overlay;
using Xunit;

namespace Paloma.Tests;

public sealed class OverlayViewModelTests
{
    [Fact]
    public void Report_ShowsTheMessage()
    {
        var (vm, messenger) = Banner(TimeSpan.FromSeconds(10));

        messenger.Send(new ErrorReportedMessage("core is down"));

        Assert.Equal("core is down", vm.ErrorMessage);
    }

    [Fact]
    public async Task Report_ClearsAfterTheBannerDuration()
    {
        var (vm, messenger) = Banner(TimeSpan.FromMilliseconds(50));

        messenger.Send(new ErrorReportedMessage("core is down"));

        Assert.Equal("core is down", vm.ErrorMessage);
        await TestWait.UntilAsync(() => vm.ErrorMessage.Length == 0);
    }

    [Fact]
    public async Task Report_WhileABannerShows_RestartsTheClock()
    {
        var (vm, messenger) = Banner(TimeSpan.FromMilliseconds(800));
        messenger.Send(new ErrorReportedMessage("first"));

        await Task.Delay(300);
        messenger.Send(new ErrorReportedMessage("second"));
        await TestWait.UntilAsync(() => vm.ErrorMessage == "second");

        // Past the first error's expiry: its cancelled timer must not have
        // wiped the newer message.
        await Task.Delay(600);
        Assert.Equal("second", vm.ErrorMessage);
        await TestWait.UntilAsync(() => vm.ErrorMessage.Length == 0);
    }

    [Fact]
    public void Report_AfterAnExpiredBanner_KeepsTheNewMessage()
    {
        var previousContext = SynchronizationContext.Current;
        var context = new PumpContext();
        SynchronizationContext.SetSynchronizationContext(context);
        try
        {
            var messenger = new WeakReferenceMessenger();
            var vm = new OverlayViewModel(messenger, TimeSpan.FromMilliseconds(20));
            messenger.Send(new ErrorReportedMessage("first"));
            Assert.Equal("first", vm.ErrorMessage);

            // Let the first banner's delay expire; its continuation parks in
            // the (not yet pumped) queue like on a busy dispatcher.
            for (var i = 0; i < 100 && context.Queue.IsEmpty; i++)
            {
                Thread.Sleep(10);
            }

            Assert.False(context.Queue.IsEmpty);
            messenger.Send(new ErrorReportedMessage("second"));
            Assert.Equal("second", vm.ErrorMessage);

            // The parked continuation of "first" runs now.
            context.PumpAll();

            Assert.Equal("second", vm.ErrorMessage);
        }
        finally
        {
            SynchronizationContext.SetSynchronizationContext(previousContext);
        }
    }

    private sealed class PumpContext : SynchronizationContext
    {
        public ConcurrentQueue<(SendOrPostCallback Callback, object? State)> Queue { get; } = new();

        public override void Post(SendOrPostCallback callback, object? state)
        {
            Queue.Enqueue((callback, state));
        }

        public void PumpAll()
        {
            while (Queue.TryDequeue(out var item))
            {
                item.Callback(item.State);
            }
        }
    }

    private static (OverlayViewModel Vm, IMessenger Messenger) Banner(TimeSpan duration)
    {
        // A private bus per test: the global one is shared across parallel
        // test classes.
        var messenger = new WeakReferenceMessenger();
        var vm = new OverlayViewModel(messenger, duration);
        return (vm, messenger);
    }

}
