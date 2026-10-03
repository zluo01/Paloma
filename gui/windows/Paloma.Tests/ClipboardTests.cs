using System.Runtime.InteropServices;
using Paloma.Helpers;
using Xunit;

namespace Paloma.Tests;

public sealed class ClipboardTests
{
    private const int Busy = unchecked((int)0x800401D0);
    private const int BadData = unchecked((int)0x800401D3);

    [Fact]
    public void GivenFreeClipboardWhenWritingShouldSetAndFlushOnce()
    {
        var (set, flush) = (0, 0);

        var copied = Clipboard.Write(() => set++, () => flush++, NoDelay);

        Assert.Equal((true, 1, 1), (copied, set, flush));
    }

    [Fact]
    public void GivenBusyClipboardWhenFlushingShouldRetryWithoutSettingAgain()
    {
        var (set, flush) = (0, 0);

        var copied = Clipboard.Write(() => set++, () => FailUntil(3, ++flush, Busy), NoDelay);

        Assert.Equal((true, 1, 3), (copied, set, flush));
    }

    [Fact]
    public void GivenClipboardStaysBusyWhenFlushingShouldGiveUpAfterFiveAttempts()
    {
        var flush = 0;

        var copied = Clipboard.Write(() => { }, () => FailUntil(int.MaxValue, ++flush, Busy), NoDelay);

        Assert.Equal((true, 5), (copied, flush));
    }

    [Fact]
    public void GivenOtherFailureWhenFlushingShouldNotRetry()
    {
        var flush = 0;

        var copied = Clipboard.Write(() => { }, () => FailUntil(int.MaxValue, ++flush, BadData), NoDelay);

        Assert.Equal((true, 1), (copied, flush));
    }

    [Fact]
    public void GivenBusyClipboardWhenSettingShouldReportFailureWithoutFlushing()
    {
        var flush = 0;

        var copied = Clipboard.Write(() => FailUntil(int.MaxValue, 1, Busy), () => flush++, NoDelay);

        Assert.Equal((false, 0), (copied, flush));
    }

    private static Task NoDelay(TimeSpan delay)
    {
        return Task.CompletedTask;
    }

    private static void FailUntil(int attempt, int current, int error)
    {
        if (current < attempt)
        {
            throw Marshal.GetExceptionForHR(error)!;
        }
    }
}