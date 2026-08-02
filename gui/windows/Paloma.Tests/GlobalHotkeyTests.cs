using System.ComponentModel;
using Windows.System;
using Windows.Win32.UI.Input.KeyboardAndMouse;
using Paloma.Models;
using Paloma.Settings;
using Xunit;

namespace Paloma.Tests;

public sealed class GlobalHotkeyTests
{
    private const int HotkeyAlreadyRegistered = 1409;

    [Fact]
    public void Register_TakenCombination_SurfacesTheRealError()
    {
        var binding = new KeyBinding(
            HOT_KEY_MODIFIERS.MOD_CONTROL | HOT_KEY_MODIFIERS.MOD_ALT | HOT_KEY_MODIFIERS.MOD_SHIFT,
            VirtualKey.F24);
        using var owner = new GlobalHotkey(binding);

        var e = Assert.Throws<Win32Exception>(() => new GlobalHotkey(binding));

        // The cleanup path must not clobber the registration error.
        Assert.Equal(HotkeyAlreadyRegistered, e.NativeErrorCode);
    }
}
