using Windows.System;
using Windows.Win32.UI.Input.KeyboardAndMouse;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Paloma.Helpers;
using Paloma.Models;

namespace Paloma.Views.Overlay.Query;

// https://github.com/microsoft/microsoft-ui-xaml/issues/10049
public sealed partial class QueryBox : RichEditBox
{
    private const HOT_KEY_MODIFIERS Ctrl = HOT_KEY_MODIFIERS.MOD_CONTROL;
    private const HOT_KEY_MODIFIERS CtrlShift = HOT_KEY_MODIFIERS.MOD_CONTROL | HOT_KEY_MODIFIERS.MOD_SHIFT;
    private const VirtualKey OemPlus = (VirtualKey)187;
    private const VirtualKey OemComma = (VirtualKey)188;
    private const VirtualKey OemPeriod = (VirtualKey)190;

    private static readonly KeyChord[] FormattingChords =
    [
        new(VirtualKey.E, Ctrl), // Center paragraph
        new(VirtualKey.J, Ctrl), // Justify paragraph
        new(VirtualKey.R, Ctrl), // Right-align paragraph
        new(VirtualKey.Number1, Ctrl), // Single line spacing
        new(VirtualKey.Number2, Ctrl), // Double line spacing
        new(VirtualKey.Number5, Ctrl), // 1.5 line spacing
        new(OemPlus, Ctrl), // Subscript
        new(OemPlus, CtrlShift), // Superscript
        new(OemComma, CtrlShift), // Smaller font
        new(OemPeriod, CtrlShift), // Bigger font
        new(VirtualKey.A, CtrlShift), // All caps
        new(VirtualKey.L, CtrlShift), // Bullet list
    ];

    protected override void OnKeyDown(KeyRoutedEventArgs e)
    {
        var modifiers = Keyboard.GetPressedModifiers();
        if (FormattingChords.Any(chord => chord.Matches(e.Key, modifiers)))
        {
            return;
        }

        base.OnKeyDown(e);
    }
}