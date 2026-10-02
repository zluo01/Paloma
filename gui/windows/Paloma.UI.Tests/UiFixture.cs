using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Xunit;

namespace Paloma.UI.Tests;

[CollectionDefinition(Name)]
public sealed class UiCollectionDefinition : ICollectionFixture<UiFixture>
{
    public const string Name = "UI";
}

/// Runs one XAML app on its own UI thread for the whole test run.
public sealed class UiFixture : IDisposable
{
    private readonly DispatcherQueue _dispatcher;
    private Window? _window;

    public UiFixture()
    {
        var started = new TaskCompletionSource<DispatcherQueue>();
        var thread = new Thread(() => Application.Start(p =>
        {
            var dispatcher = DispatcherQueue.GetForCurrentThread();
            SynchronizationContext.SetSynchronizationContext(new DispatcherQueueSynchronizationContext(dispatcher));
            _ = new TestApp();
            started.SetResult(dispatcher);
        }))
        {
            IsBackground = true,
        };
        thread.SetApartmentState(ApartmentState.STA);
        thread.Start();
        _dispatcher = started.Task.GetAwaiter().GetResult();
    }

    /// Runs the test body on the UI thread.
    public Task RunAsync(Func<Task> body)
    {
        var done = new TaskCompletionSource();
        _dispatcher.TryEnqueue(async () =>
        {
            try
            {
                await body();
                done.SetResult();
            }
            catch (Exception e)
            {
                done.SetException(e);
            }
        });
        return done.Task;
    }

    /// Hosts the element in a window that is never shown and waits for it to load. Call on the UI thread.
    public async Task<T> ShowAsync<T>(T element) where T : FrameworkElement
    {
        var loaded = new TaskCompletionSource();
        element.Loaded += (_, _) => loaded.TrySetResult();
        _window ??= new Window();
        _window.Content = element;
        await loaded.Task.WaitAsync(TimeSpan.FromSeconds(5));
        return element;
    }

    /// Lets everything queued on the UI thread, down to low priority, run first. Call on the UI thread.
    public Task IdleAsync()
    {
        var idle = new TaskCompletionSource();
        _dispatcher.TryEnqueue(DispatcherQueuePriority.Low, idle.SetResult);
        return idle.Task;
    }

    public void Dispose()
    {
        _dispatcher.TryEnqueue(() =>
        {
            _window?.Close();
            Application.Current.Exit();
        });
    }
}
