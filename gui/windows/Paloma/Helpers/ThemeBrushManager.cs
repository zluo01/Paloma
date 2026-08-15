using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;

namespace Paloma.Helpers;

internal sealed class ThemeBrushManager(ElementTheme theme)
{
    // Walking the theme dictionaries is slow interop, so each brush
    // resolves once per theme and is reused from here.
    private static readonly Dictionary<(string Key, ElementTheme Theme), Brush> BrushCache = [];

    private ElementTheme _theme = theme;

    public void Refresh(ElementTheme theme)
    {
        _theme = theme;
    }

    public Brush ThemeBrush(string key)
    {
        if (BrushCache.TryGetValue((key, _theme), out var cached))
        {
            return cached;
        }

        var brush = ResolveBrush(key, _theme);
        BrushCache[(key, _theme)] = brush;
        return brush;
    }

    private static Brush ResolveBrush(string key, ElementTheme theme)
    {
        string[] names = theme == ElementTheme.Light ? ["Light", "Default"] : ["Dark", "Default"];
        foreach (var name in names)
        {
            if (FindThemeDictionary(Application.Current.Resources, name) is { } dictionary
                && dictionary.TryGetValue(key, out var themed)
                && themed is Brush brush)
            {
                return brush;
            }
        }

        return Application.Current.Resources.TryGetValue(key, out var value)
               && value is Brush plain
            ? plain
            : new SolidColorBrush(Colors.Transparent);
    }

    private static ResourceDictionary? FindThemeDictionary(ResourceDictionary root, string name)
    {
        if (root.ThemeDictionaries.TryGetValue(name, out var value)
            && value is ResourceDictionary themed)
        {
            return themed;
        }

        foreach (var merged in root.MergedDictionaries)
        {
            if (FindThemeDictionary(merged, name) is { } found)
            {
                return found;
            }
        }

        return null;
    }
}