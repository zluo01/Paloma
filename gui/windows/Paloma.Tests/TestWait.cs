namespace Paloma.Tests;

internal static class TestWait
{
    public static async Task UntilAsync(Func<bool> condition, int timeoutMs = 2000)
    {
        var start = Environment.TickCount64;
        while (!condition())
        {
            if (Environment.TickCount64 - start > timeoutMs)
            {
                throw new TimeoutException("condition not met within the timeout");
            }
            await Task.Delay(10);
        }
    }
}
