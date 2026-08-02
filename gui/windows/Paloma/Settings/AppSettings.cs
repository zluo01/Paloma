using System.ComponentModel;
using System.Text.Json;
using Microsoft.Win32;
using Paloma.Models;
using Serilog;

namespace Paloma.Settings;

public sealed partial class AppSettings : IDisposable
{
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string ValueName = "Paloma";

    private static string Path => System.IO.Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "Paloma",
        "frontend.json");

    private GlobalHotkey? HotKey { get; set; }

    private AppSettings(GlobalHotkey? hotKey)
    {
        HotKey = hotKey;
    }

    public static AppSettings Load()
    {
        var binding = KeyBinding.Default;
        try
        {
            if (File.Exists(Path))
            {
                var config = JsonSerializer.Deserialize<AppConfig>(File.ReadAllText(Path));
                if (config?.KeyBinding is { } saved)
                {
                    binding = saved;
                }
            }
        }
        catch (Exception e)
        {
            Log.Warning(e, "settings load failed, using defaults");
        }

        GlobalHotkey? hotKey = null;
        try
        {
            hotKey = new GlobalHotkey(binding);
        }
        catch (Win32Exception e)
        {
            // A taken combination must not be fatal: the app stays reachable
            // through the tray, and settings can re-bind later.
            Log.Warning(e, "hotkey bind failed for {HotKey}", binding.GetLabel());
        }

        return new AppSettings(hotKey);
    }

    public static bool IsAutostartEnabled()
    {
        using var key = Registry.CurrentUser.OpenSubKey(RunKey);
        return key?.GetValue(ValueName) is string;
    }

    public static void SetAutostart(bool enabled)
    {
        if (enabled)
        {
            using var key = Registry.CurrentUser.CreateSubKey(RunKey);
            key.SetValue(ValueName, $"\"{Environment.ProcessPath}\"");
        }
        else
        {
            using var key = Registry.CurrentUser.OpenSubKey(RunKey, writable: true);
            key?.DeleteValue(ValueName, throwOnMissingValue: false);
        }
    }

    public string HotKeyLabel => HotKey?.Binding.GetLabel() ?? "";

    public void UpdateHotKey(KeyBinding binding)
    {
        if (HotKey is null)
        {
            HotKey = new GlobalHotkey(binding);
        }
        else if (!HotKey.Update(binding))
        {
            throw new Win32Exception($"{binding.GetLabel()} is taken by another application.");
        }

        Save();
    }

    private void Save()
    {
        if (HotKey is null)
        {
            return;
        }

        Directory.CreateDirectory(System.IO.Path.GetDirectoryName(Path)!);
        File.WriteAllText(Path, JsonSerializer.Serialize(new AppConfig(HotKey.Binding)));
    }

    public void Dispose()
    {
        HotKey?.Dispose();
    }
}