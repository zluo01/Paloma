using Microsoft.UI.Input;
using Windows.System;
using Windows.UI.Core;
using Windows.Win32.UI.Input.KeyboardAndMouse;

namespace Paloma.Helpers;

internal static class Keyboard
{
    public static HOT_KEY_MODIFIERS GetPressedModifiers()
    {
        return GetPressedModifiers(IsDown);
    }

    internal static HOT_KEY_MODIFIERS GetPressedModifiers(Func<VirtualKey, bool> isDown)
    {
        var modifiers = default(HOT_KEY_MODIFIERS);
        if (isDown(VirtualKey.Menu))
        {
            modifiers |= HOT_KEY_MODIFIERS.MOD_ALT;
        }

        if (isDown(VirtualKey.Control))
        {
            modifiers |= HOT_KEY_MODIFIERS.MOD_CONTROL;
        }

        if (isDown(VirtualKey.Shift))
        {
            modifiers |= HOT_KEY_MODIFIERS.MOD_SHIFT;
        }

        if (isDown(VirtualKey.LeftWindows) || isDown(VirtualKey.RightWindows))
        {
            modifiers |= HOT_KEY_MODIFIERS.MOD_WIN;
        }

        return modifiers;
    }

    private static bool IsDown(VirtualKey key)
    {
        return InputKeyboardSource.GetKeyStateForCurrentThread(key).HasFlag(CoreVirtualKeyStates.Down);
    }
}
