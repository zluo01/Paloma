using System.Runtime.InteropServices;
using Windows.System;
using Microsoft.UI;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Xunit;
using DispatcherQueue = Microsoft.UI.Dispatching.DispatcherQueue;
using DispatcherQueuePriority = Microsoft.UI.Dispatching.DispatcherQueuePriority;

namespace Paloma.UI.Tests;

[CollectionDefinition(Name)]
public sealed class UiCollectionDefinition : ICollectionFixture<UiFixture>
{
    public const string Name = "UI";
}

/// Runs one XAML app on its own UI thread for the whole test run.
public sealed class UiFixture : IDisposable
{
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern nint FindWindowEx(nint parent, nint after, string className, string? title);

    [DllImport("user32.dll")]
    private static extern bool PostMessage(nint window, uint message, nint wparam, nint lparam);

    [DllImport("user32.dll")]
    private static extern bool GetKeyboardState(byte[] state);

    [DllImport("user32.dll")]
    private static extern bool SetKeyboardState(byte[] state);

    private const uint KeyDownMessage = 0x0100;
    private const uint KeyUpMessage = 0x0101;
    private const int KeyDownFlags = 1;
    private const int KeyUpFlags = unchecked((int)0xC0000001);
    private const byte KeyHeld = 0x80;

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

    public async Task PressAsync(FrameworkElement element, VirtualKey key, params VirtualKey[] modifiers)
    {
        var window = Win32Interop.GetWindowFromWindowId(element.XamlRoot.ContentIslandEnvironment.AppWindowId);
        var bridge = FindWindowEx(window, 0, "Microsoft.UI.Content.DesktopChildSiteBridge", null);
        var input = FindWindowEx(bridge, 0, "InputSiteWindowClass", null);
        Assert.NotEqual(0, input);

        var released = new byte[256];
        _ = GetKeyboardState(released);
        var held = (byte[])released.Clone();
        foreach (var modifier in modifiers)
        {
            held[(int)modifier] = KeyHeld;
        }

        _ = SetKeyboardState(held);
        try
        {
            foreach (var modifier in modifiers)
            {
                _ = PostMessage(input, KeyDownMessage, (nint)modifier, KeyDownFlags);
            }

            _ = PostMessage(input, KeyDownMessage, (nint)key, KeyDownFlags);
            _ = PostMessage(input, KeyUpMessage, (nint)key, KeyUpFlags);
            foreach (var modifier in modifiers.Reverse())
            {
                _ = PostMessage(input, KeyUpMessage, (nint)modifier, KeyUpFlags);
            }

            await IdleAsync();
        }
        finally
        {
            _ = SetKeyboardState(released);
        }
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