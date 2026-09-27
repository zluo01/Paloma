using System.Runtime.InteropServices;
using Windows.System;
using Windows.Win32.UI.Input.KeyboardAndMouse;
using Paloma.Helpers;
using Xunit;

namespace Paloma.Tests;

public sealed class KeyboardTests
{
    private const uint Release = 0x2;

    [Fact]
    public void GetPressedModifiers_WithNothingHeld_IsEmpty()
    {
        Assert.Equal(default, Keyboard.GetPressedModifiers(_ => false));
    }

    [Theory]
    [InlineData(VirtualKey.Menu, HOT_KEY_MODIFIERS.MOD_ALT)]
    [InlineData(VirtualKey.Control, HOT_KEY_MODIFIERS.MOD_CONTROL)]
    [InlineData(VirtualKey.Shift, HOT_KEY_MODIFIERS.MOD_SHIFT)]
    [InlineData(VirtualKey.LeftWindows, HOT_KEY_MODIFIERS.MOD_WIN)]
    [InlineData(VirtualKey.RightWindows, HOT_KEY_MODIFIERS.MOD_WIN)]
    public void GetPressedModifiers_MapsEachModifierKey(VirtualKey held, HOT_KEY_MODIFIERS expected)
    {
        Assert.Equal(expected, Keyboard.GetPressedModifiers(key => key == held));
    }

    [Fact]
    public void GetPressedModifiers_CombinesHeldModifiers()
    {
        var held = new[] { VirtualKey.Control, VirtualKey.Shift, VirtualKey.LeftWindows };

        Assert.Equal(
            HOT_KEY_MODIFIERS.MOD_CONTROL | HOT_KEY_MODIFIERS.MOD_SHIFT | HOT_KEY_MODIFIERS.MOD_WIN,
            Keyboard.GetPressedModifiers(held.Contains));
    }

    [Fact]
    public void GetPressedModifiers_ProbesOnlyModifierKeys()
    {
        var probed = new List<VirtualKey>();

        Keyboard.GetPressedModifiers(key =>
        {
            probed.Add(key);
            return false;
        });

        Assert.Equal(
            [VirtualKey.Menu, VirtualKey.Control, VirtualKey.Shift, VirtualKey.LeftWindows, VirtualKey.RightWindows],
            probed);
    }

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
