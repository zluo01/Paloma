using Windows.System;

namespace Paloma.Helpers;

public static class Browser
{
    /// <summary>Opens the URL in the default browser and returns the parsed
    /// Uri. A string that is not an absolute URL opens nothing and returns
    /// null.</summary>
    public static Uri? Open(string url)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri))
        {
            return null;
        }

        _ = Launcher.LaunchUriAsync(uri);
        return uri;
    }
}