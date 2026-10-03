using System.Runtime.InteropServices;
using Windows.ApplicationModel.DataTransfer;
using Serilog;
using SystemClipboard = Windows.ApplicationModel.DataTransfer.Clipboard;

namespace Paloma.Helpers;

internal static class Clipboard
{
    private const int Busy = unchecked((int)0x800401D0);
    private const int FlushAttempts = 5;

    private static readonly TimeSpan RetryDelay = TimeSpan.FromMilliseconds(50);

    public static bool Copy(string text)
    {
        var package = new DataPackage();
        package.SetText(text);
        return Write(() => SystemClipboard.SetContent(package), SystemClipboard.Flush, Task.Delay);
    }

    internal static bool Write(Action setContent, Action flush, Func<TimeSpan, Task> delay)
    {
        try
        {
            setContent();
        }
        catch (Exception e)
        {
            Log.Error(e, "fail to copy to the clipboard");
            return false;
        }

        _ = FlushAsync(flush, delay);
        return true;
    }

    // Flush make the content survive after app exists.
    private static async Task FlushAsync(Action flush, Func<TimeSpan, Task> delay)
    {
        for (var attempt = 1; attempt <= FlushAttempts; attempt++)
        {
            try
            {
                flush();
                return;
            }
            catch (COMException e) when (e.HResult == Busy && attempt < FlushAttempts)
            {
                await delay(RetryDelay);
            }
            catch (Exception e)
            {
                Log.Error(e, "fail to flush the clipboard");
                return;
            }
        }
    }
}