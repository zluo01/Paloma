using Windows.System;
using Windows.Win32;
using Windows.Win32.UI.Input.KeyboardAndMouse;

namespace Paloma.Helpers;

internal static class Keyboard
{
    // The most significant bit of GetAsyncKeyState's short marks a held key.
    private const int KeyDownBit = 0x8000;

    public static HOT_KEY_MODIFIERS GetPressedModifiers()
    {
        var modifiers = default(HOT_KEY_MODIFIERS);
        if (IsDown(VirtualKey.Menu))
        {
            modifiers |= HOT_KEY_MODIFIERS.MOD_ALT;
        }

        if (IsDown(VirtualKey.Control))
        {
            modifiers |= HOT_KEY_MODIFIERS.MOD_CONTROL;
        }

        if (IsDown(VirtualKey.Shift))
        {
            modifiers |= HOT_KEY_MODIFIERS.MOD_SHIFT;
        }

        if (IsDown(VirtualKey.LeftWindows) || IsDown(VirtualKey.RightWindows))
        {
            modifiers |= HOT_KEY_MODIFIERS.MOD_WIN;
        }

        return modifiers;
    }

    private static bool IsDown(VirtualKey key)
    {
        return (PInvoke.GetAsyncKeyState((int)key) & KeyDownBit) != 0;
    }
}