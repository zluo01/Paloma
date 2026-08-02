using Windows.System;
using Windows.Win32;
using Windows.Win32.UI.Input.KeyboardAndMouse;

namespace Paloma.Models;

public sealed record KeyBinding
{
    public static readonly KeyBinding Default = new(HOT_KEY_MODIFIERS.MOD_ALT, VirtualKey.Space);

    public HOT_KEY_MODIFIERS Modifiers { get; }

    public VirtualKey VirtualKey { get; }

    public KeyBinding(HOT_KEY_MODIFIERS modifiers, VirtualKey virtualKey)
    {
        // Manually apply no repeat flag to force single toggle per key press
        Modifiers = modifiers | HOT_KEY_MODIFIERS.MOD_NOREPEAT;
        VirtualKey = virtualKey;
    }

    /// <summary>Human-readable form of the combo, e.g. "Alt+Space".</summary>
    public string GetLabel()
    {
        var modifiers = Modifiers & ~HOT_KEY_MODIFIERS.MOD_NOREPEAT;
        var parts = new List<string>();
        if (modifiers.HasFlag(HOT_KEY_MODIFIERS.MOD_CONTROL))
        {
            parts.Add("Ctrl");
        }

        if (modifiers.HasFlag(HOT_KEY_MODIFIERS.MOD_ALT))
        {
            parts.Add("Alt");
        }

        if (modifiers.HasFlag(HOT_KEY_MODIFIERS.MOD_SHIFT))
        {
            parts.Add("Shift");
        }

        if (modifiers.HasFlag(HOT_KEY_MODIFIERS.MOD_WIN))
        {
            parts.Add("Win");
        }

        parts.Add(KeyName());
        return string.Join("+", parts);
    }

    /// <summary>
    /// Returns the key's display name from the active keyboard layout,
    /// falling back to the hex virtual-key code when the layout cannot
    /// name it.
    /// </summary>
    private string KeyName()
    {
        var scanCode = PInvoke.MapVirtualKey((uint)VirtualKey, MAP_VIRTUAL_KEY_TYPE.MAPVK_VK_TO_VSC);
        if (scanCode == 0)
        {
            return $"0x{(uint)VirtualKey:X2}";
        }

        var lparam = (int)(scanCode << 16);
        if (IsExtended())
        {
            lparam |= 1 << 24;
        }

        Span<char> name = stackalloc char[64];
        var length = PInvoke.GetKeyNameText(lparam, name);
        return length > 0 ? new string(name[..length]) : $"0x{(uint)VirtualKey:X2}";
    }

    /// <summary>
    /// Returns true when the key needs the extended-key flag to be named
    /// correctly — these keys share scan codes with others, so without the
    /// flag Up would label as Num 8 and NumLock as Pause.
    /// </summary>
    private bool IsExtended()
    {
        return VirtualKey is VirtualKey.Divide or VirtualKey.NumberKeyLock
            or >= VirtualKey.PageUp and <= VirtualKey.Delete or >= VirtualKey.LeftWindows and <= VirtualKey.Application;
    }
}