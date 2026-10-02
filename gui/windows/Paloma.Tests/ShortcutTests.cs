using Windows.System;
using Windows.Win32.UI.Input.KeyboardAndMouse;
using Paloma.Models;
using Xunit;

namespace Paloma.Tests;

public sealed class ShortcutTests
{
    private const HOT_KEY_MODIFIERS None = default;
    private const HOT_KEY_MODIFIERS Ctrl = HOT_KEY_MODIFIERS.MOD_CONTROL;
    private const HOT_KEY_MODIFIERS CtrlShift = HOT_KEY_MODIFIERS.MOD_CONTROL | HOT_KEY_MODIFIERS.MOD_SHIFT;

    [Theory]
    [InlineData(VirtualKey.H, Ctrl, true)]
    [InlineData(VirtualKey.H, None, false)]
    [InlineData(VirtualKey.H, CtrlShift, false)]
    [InlineData(VirtualKey.J, Ctrl, false)]
    public void GivenChordWhenMatchingShouldNeedItsKeyAndExactModifiers(VirtualKey key, HOT_KEY_MODIFIERS modifiers,
        bool expected)
    {
        var chord = new KeyChord(VirtualKey.H, Ctrl);

        Assert.Equal(expected, chord.Matches(key, modifiers));
    }

    [Theory]
    [InlineData(None)]
    [InlineData(Ctrl)]
    [InlineData(CtrlShift)]
    public void GivenChordForAnyModifiersWhenMatchingShouldIgnoreThem(HOT_KEY_MODIFIERS modifiers)
    {
        var chord = new KeyChord(VirtualKey.Escape, AnyModifiers: true);

        Assert.True(chord.Matches(VirtualKey.Escape, modifiers));
    }

    [Theory]
    [InlineData(VirtualKey.Up, true)]
    [InlineData(VirtualKey.Down, true)]
    [InlineData(VirtualKey.Left, false)]
    public void GivenShortcutWithTwoChordsWhenMatchingShouldAcceptEither(VirtualKey key, bool expected)
    {
        var shortcut = new Shortcut("Move", new KeyChord(VirtualKey.Up), new KeyChord(VirtualKey.Down));

        Assert.Equal(expected, shortcut.Matches(key, None));
    }
}