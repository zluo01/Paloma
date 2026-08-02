using System.Text.Json;
using Paloma.Models;
using Windows.System;
using Windows.Win32.UI.Input.KeyboardAndMouse;
using Xunit;

namespace Paloma.Tests;

public sealed class AppSettingsTests
{
    [Fact]
    public void Serialize_WritesOnlyStoredSettings()
    {
        var json = JsonSerializer.Serialize(new AppConfig(KeyBinding.Default));

        Assert.Contains("KeyBinding", json);
        Assert.Contains("Modifiers", json);
        Assert.Contains("VirtualKey", json);
        // The label is derived from the two stored values; persisting it
        // would just leave a stale copy in frontend.json.
        Assert.DoesNotContain("Label", json);
    }

    [Fact]
    public void Serialize_RoundTripsSettings()
    {
        var config = new AppConfig(
            new KeyBinding(HOT_KEY_MODIFIERS.MOD_CONTROL, VirtualKey.Space));

        var json = JsonSerializer.Serialize(config);
        var restored = JsonSerializer.Deserialize<AppConfig>(json)!;

        Assert.Equal(config.KeyBinding, restored.KeyBinding);
    }
}
