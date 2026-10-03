using System.Runtime.InteropServices;
using Windows.ApplicationModel.DataTransfer;

namespace Paloma.UI.Tests.Helpers;

/// Opens the clipboard to read its text as soon as it changes, like a clipboard manager does.
/// Starts from a placeholder text and puts back the text the clipboard held when it is disposed,
/// so a test's copy is always a change. Start and dispose on the UI thread.
internal sealed class ClipboardListener : IAsyncDisposable
{
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern nint CreateWindowExW(
        uint exStyle, string className, string? name, uint style, int x, int y, int width, int height,
        nint parent, nint menu, nint instance, nint param);

    [DllImport("user32.dll")]
    private static extern bool DestroyWindow(nint window);

    [DllImport("user32.dll")]
    private static extern bool AddClipboardFormatListener(nint window);

    [DllImport("user32.dll")]
    private static extern int GetMessageW(out Message message, nint window, uint min, uint max);

    [DllImport("user32.dll")]
    private static extern bool PostThreadMessageW(uint thread, uint message, nint wparam, nint lparam);

    [DllImport("user32.dll")]
    private static extern bool OpenClipboard(nint window);

    [DllImport("user32.dll")]
    private static extern bool CloseClipboard();

    [DllImport("user32.dll")]
    private static extern nint GetClipboardData(uint format);

    [DllImport("kernel32.dll")]
    private static extern nint GlobalLock(nint handle);

    [DllImport("kernel32.dll")]
    private static extern bool GlobalUnlock(nint handle);

    [DllImport("kernel32.dll")]
    private static extern uint GetCurrentThreadId();

    private const uint ClipboardUpdateMessage = 0x031D;
    private const uint QuitMessage = 0x0012;
    private const uint UnicodeTextFormat = 13;
    private const nint MessageOnlyParent = -3;
    private const int WriteAttempts = 20;
    private const string Placeholder = "Paloma.UI.Tests";

    private static readonly TimeSpan RetryDelay = TimeSpan.FromMilliseconds(50);
    private static readonly TimeSpan Timeout = TimeSpan.FromSeconds(5);

    private readonly TaskCompletionSource<string> _text = new();
    private readonly ManualResetEventSlim _listening = new();
    private readonly string _saved;
    private readonly Thread _thread;
    private uint _threadId;

    public Task<string> Text => _text.Task.WaitAsync(Timeout);

    private ClipboardListener(string saved)
    {
        _saved = saved;
        _thread = new Thread(() =>
        {
            _threadId = GetCurrentThreadId();
            var window = CreateWindowExW(0, "STATIC", null, 0, 0, 0, 0, 0, MessageOnlyParent, 0, 0, 0);
            _ = AddClipboardFormatListener(window);
            _listening.Set();
            while (GetMessageW(out var message, 0, 0, 0) > 0)
            {
                if (message.Id == ClipboardUpdateMessage && OpenClipboard(window))
                {
                    var handle = GetClipboardData(UnicodeTextFormat);
                    var text = Marshal.PtrToStringUni(GlobalLock(handle)) ?? string.Empty;
                    _ = GlobalUnlock(handle);
                    _ = CloseClipboard();
                    if (text != Placeholder)
                    {
                        _text.TrySetResult(text);
                    }
                }
            }

            _ = DestroyWindow(window);
        });
        _thread.Start();
        _listening.Wait();
    }

    public static async Task<ClipboardListener> StartAsync()
    {
        var content = Clipboard.GetContent();
        var saved = content.Contains(StandardDataFormats.Text) ? await content.GetTextAsync() : string.Empty;
        await WriteAsync(Placeholder);
        return new ClipboardListener(saved);
    }

    public async ValueTask DisposeAsync()
    {
        _ = PostThreadMessageW(_threadId, QuitMessage, 0, 0);
        _thread.Join();
        _listening.Dispose();
        await WriteAsync(_saved);
    }

    private static async Task WriteAsync(string text)
    {
        var package = new DataPackage();
        package.SetText(text);
        for (var attempt = 0; attempt < WriteAttempts; attempt++)
        {
            try
            {
                Clipboard.SetContent(package);
                Clipboard.Flush();
                return;
            }
            catch (COMException)
            {
                await Task.Delay(RetryDelay);
            }
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct Message
    {
        public nint Window;
        public uint Id;
        public nint WParam;
        public nint LParam;
        public uint Time;
        public int X;
        public int Y;
    }
}