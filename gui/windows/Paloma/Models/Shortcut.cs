using Windows.System;
using Windows.Win32.UI.Input.KeyboardAndMouse;

namespace Paloma.Models;

internal sealed record KeyChord(VirtualKey Key, HOT_KEY_MODIFIERS Modifiers = default, bool AnyModifiers = false)
{
    public bool Matches(VirtualKey key, HOT_KEY_MODIFIERS modifiers)
    {
        return key == Key && (AnyModifiers || modifiers == Modifiers);
    }

    public IEnumerable<string> Keys()
    {
        if (Modifiers.HasFlag(HOT_KEY_MODIFIERS.MOD_CONTROL))
        {
            yield return "Ctrl";
        }

        if (Modifiers.HasFlag(HOT_KEY_MODIFIERS.MOD_ALT))
        {
            yield return "Alt";
        }

        if (Modifiers.HasFlag(HOT_KEY_MODIFIERS.MOD_SHIFT))
        {
            yield return "Shift";
        }

        yield return Key switch
        {
            VirtualKey.Up => "↑",
            VirtualKey.Down => "↓",
            VirtualKey.PageUp => "PgUp",
            VirtualKey.PageDown => "PgDn",
            VirtualKey.Escape => "Esc",
            VirtualKey.Delete => "Del",
            _ => Key.ToString(),
        };
    }
}

internal sealed class Shortcut(string description, params KeyChord[] chords)
{
    public string Description { get; } = description;

    public IReadOnlyList<string> Keys { get; } = [.. chords.SelectMany(chord => chord.Keys())];

    public string Label { get; } = string.Join(" / ", chords.Select(chord => string.Join("+", chord.Keys())));

    public bool Matches(VirtualKey key, HOT_KEY_MODIFIERS modifiers)
    {
        return chords.Any(chord => chord.Matches(key, modifiers));
    }
}
