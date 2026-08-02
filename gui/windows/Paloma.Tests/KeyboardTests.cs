using System.Runtime.InteropServices;
using Windows.System;
using Windows.Win32.UI.Input.KeyboardAndMouse;
using Paloma.Helpers;
using Xunit;

namespace Paloma.Tests;

// Injects real key events: the helper reads the physical keyboard state, so
// a mock would only test the mock.
public sealed class KeyboardTests
{
    private const uint Release = 0x2;

    [Fact]
    public void GetPressedModifiers_SeesTheHeldShiftAndItsRelease()
    {
        try
        {
            keybd_event((byte)VirtualKey.Shift, 0, 0, 0);
            Assert.True(
                Keyboard.GetPressedModifiers().HasFlag(HOT_KEY_MODIFIERS.MOD_SHIFT));
        }
        finally
        {
            keybd_event((byte)VirtualKey.Shift, 0, Release, 0);
        }

        Assert.False(
            Keyboard.GetPressedModifiers().HasFlag(HOT_KEY_MODIFIERS.MOD_SHIFT));
    }

    [Fact]
    public void GetPressedModifiers_MapsTheHeldControl()
    {
        try
        {
            keybd_event((byte)VirtualKey.Control, 0, 0, 0);
            Assert.True(
                Keyboard.GetPressedModifiers().HasFlag(HOT_KEY_MODIFIERS.MOD_CONTROL));
        }
        finally
        {
            keybd_event((byte)VirtualKey.Control, 0, Release, 0);
        }

        Assert.False(
            Keyboard.GetPressedModifiers().HasFlag(HOT_KEY_MODIFIERS.MOD_CONTROL));
    }

    [DllImport("user32.dll")]
    private static extern void keybd_event(byte vk, byte scan, uint flags, nuint extra);
}
