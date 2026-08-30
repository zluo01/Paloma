using Paloma.Client;

namespace Paloma.Helpers;

internal static class ClientGuard
{
    /// <summary>Runs the operation and returns whether it succeeded. A failure
    /// is reported as one "label: detail" line instead of propagating.</summary>
    public static async Task<bool> TryAsync(
        Func<Task> operation,
        Action<string> report,
        string failureLabel)
    {
        try
        {
            await operation();
            return true;
        }
        catch (Exception e)
        {
            report($"{failureLabel}: {PalomaClient.Describe(e)}");
            return false;
        }
    }

    /// <summary>Variant for operations that return their own bool. A failure
    /// is reported the same way and yields false.</summary>
    public static async Task<bool> TryAsync(
        Func<Task<bool>> operation,
        Action<string> report,
        string failureLabel)
    {
        try
        {
            return await operation();
        }
        catch (Exception e)
        {
            report($"{failureLabel}: {PalomaClient.Describe(e)}");
            return false;
        }
    }
}