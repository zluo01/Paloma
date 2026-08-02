using Paloma.Models;
using Windows.System;
using Windows.Win32.UI.Input.KeyboardAndMouse;
using Xunit;

namespace Paloma.Tests;

public sealed class KeyBindingTests
{
    [Fact]
    public void GetLabel_RendersModifiersInConventionalOrder()
    {
        var binding = new KeyBinding(
            HOT_KEY_MODIFIERS.MOD_WIN
                | HOT_KEY_MODIFIERS.MOD_SHIFT
                | HOT_KEY_MODIFIERS.MOD_ALT
                | HOT_KEY_MODIFIERS.MOD_CONTROL,
            VirtualKey.A);

        Assert.Equal("Ctrl+Alt+Shift+Win+A", binding.GetLabel());
    }

    [Fact]
    public void GetLabel_DoesNotRenderTheImplicitNoRepeatFlag()
    {
        // The constructor forces MOD_NOREPEAT onto every binding; it is an
        // OS behavior flag, not a key the user pressed.
        var binding = new KeyBinding(HOT_KEY_MODIFIERS.MOD_ALT, VirtualKey.Space);

        Assert.True(binding.Modifiers.HasFlag(HOT_KEY_MODIFIERS.MOD_NOREPEAT));
        Assert.Equal("Alt+Space", binding.GetLabel());
    }

    [Fact]
    public void GetLabel_NamesExtendedKeysCorrectly()
    {
        // Without the extended scan-code bit, GetKeyNameText calls NumLock
        // "Pause" and has no name at all for the Application key.
        Assert.Equal(
            "Alt+Num Lock",
            new KeyBinding(HOT_KEY_MODIFIERS.MOD_ALT, VirtualKey.NumberKeyLock).GetLabel());
        Assert.Equal(
            "Alt+Application",
            new KeyBinding(HOT_KEY_MODIFIERS.MOD_ALT, VirtualKey.Application).GetLabel());
    }

    [Fact]
    public void GetLabel_DistinguishesArrowsFromTheNumpad()
    {
        // Up and Num 8 share a scan code; only the extended flag tells the
        // layout which one to name.
        Assert.Equal(
            "Alt+Up",
            new KeyBinding(HOT_KEY_MODIFIERS.MOD_ALT, VirtualKey.Up).GetLabel());
        Assert.Equal(
            "Alt+Num 8",
            new KeyBinding(HOT_KEY_MODIFIERS.MOD_ALT, VirtualKey.NumberPad8).GetLabel());
    }
}
