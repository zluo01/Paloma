using System.Globalization;
using Paloma.Models;
using ProviderBackendId = PalomaCore.ProviderBackendId;
using UserDecision = PalomaCore.UserDecision;

namespace Paloma.Extensions;

public static class Display
{
    public static string Backend(ProviderBackendId id) => $"{id.ProviderId} / {id.BackendId}";

    public static string Decision(UserDecision decision)
    {
        return decision switch
        {
            UserDecision.AllowOnce => "Allow once",
            UserDecision.Allow allow => allow.Glob
                ? $"Always allow {allow.Command} *"
                : $"Always allow {allow.Command}",
            UserDecision.AllowSession => "Allow for this session",
            UserDecision.IgnorePermission => "Stop asking this session",
            _ => "Deny",
        };
    }

    public static string Glyph(OverlayMode mode)
    {
        return mode switch
        {
            OverlayMode.Chat => "\uE8BD",
            OverlayMode.Sessions => "\uE81C",
            _ => "\uE721",
        };
    }

    public static string Placeholder(OverlayMode mode)
    {
        return mode switch
        {
            OverlayMode.Chat => "Reply…",
            OverlayMode.Sessions => "Search sessions",
            _ => "Search or ask anything",
        };
    }

    public static string RelativeTime(long unixSeconds)
    {
        var updated = DateTimeOffset.FromUnixTimeSeconds(unixSeconds);
        var delta = DateTimeOffset.UtcNow - updated;
        return delta switch
        {
            { TotalMinutes: < 1 } => "just now",
            { TotalMinutes: < 60 } => $"{(int)delta.TotalMinutes} min ago",
            { TotalHours: < 24 } => $"{(int)delta.TotalHours} hr ago",
            { TotalDays: < 2 } => "yesterday",
            { TotalDays: < 7 } => $"{(int)delta.TotalDays} days ago",
            _ => updated.ToLocalTime().ToString("d", CultureInfo.CurrentCulture),
        };
    }
}