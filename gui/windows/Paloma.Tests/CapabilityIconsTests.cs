using Google.Protobuf;
using Paloma.Helpers;
using Xunit;
using CapabilityIcon = Paloma.Extension.V1.CapabilityIcon;

namespace Paloma.Tests;

// Only the render-free paths run here: a successful render constructs
// WinUI image sources, which need the XAML runtime the test host lacks.
public sealed class CapabilityIconsTests
{
    [Fact]
    public void IsSvg_SeesThroughBomAndWhitespace()
    {
        Assert.True(CapabilityIcons.IsSvg("<svg/>"u8.ToArray()));
        Assert.True(CapabilityIcons.IsSvg([0xEF, 0xBB, 0xBF, (byte)'<']));
        Assert.True(CapabilityIcons.IsSvg("  \r\n<svg/>"u8.ToArray()));
        Assert.False(CapabilityIcons.IsSvg([0x89, (byte)'P', (byte)'N', (byte)'G']));
        Assert.False(CapabilityIcons.IsSvg([0xEF, 0xBB, 0xBF]));
    }

    [Fact]
    public void IsGlyph_OnlyForSingleGlyphCodepoints()
    {
        Assert.True(CapabilityIcons.IsGlyph("\uE8EF"));
        Assert.False(CapabilityIcons.IsGlyph("image-png"));
        Assert.False(CapabilityIcons.IsGlyph(string.Empty));
        Assert.False(CapabilityIcons.IsGlyph("ab"));
    }

    [Fact]
    public void CanLoad_OnlyForRenderableCases()
    {
        Assert.False(CapabilityIcons.CanLoad(new CapabilityIcon()));
        Assert.False(CapabilityIcons.CanLoad(new CapabilityIcon { Name = "image-png" }));
        Assert.True(CapabilityIcons.CanLoad(new CapabilityIcon { Name = "\uE8EF" }));
        Assert.False(CapabilityIcons.CanLoad(new CapabilityIcon { Embedded = ByteString.Empty }));
        Assert.True(CapabilityIcons.CanLoad(new CapabilityIcon { Embedded = ByteString.CopyFrom(1, 2) }));
        Assert.True(CapabilityIcons.CanLoad(new CapabilityIcon { Path = @"C:\anything" }));
    }

    [Fact]
    public async Task Load_WithAName_YieldsNoImage()
    {
        Assert.Null(await CapabilityIcons.LoadAsync(new CapabilityIcon { Name = "image-png" }));
    }

    [Fact]
    public async Task Load_WithEmptyEmbeddedBytes_YieldsNoImage()
    {
        Assert.Null(
            await CapabilityIcons.LoadAsync(new CapabilityIcon { Embedded = ByteString.Empty }));
    }

    [Fact]
    public void Load_SamePathWhileInFlight_SharesOneRender()
    {
        var first = CapabilityIcons.LoadPathAsync(@"Z:\paloma-tests\missing-a");
        var second = CapabilityIcons.LoadPathAsync(@"Z:\paloma-tests\missing-a");

        Assert.Same(first, second);
    }

    [Fact]
    public async Task Load_AfterAFailedRender_RetriesWithAFreshTask()
    {
        var first = CapabilityIcons.LoadPathAsync(@"Z:\paloma-tests\missing-b");
        Assert.Null(await first);

        // The failed task is evicted so the next request re-renders.
        await TestWait.UntilAsync(
            () => !ReferenceEquals(first, CapabilityIcons.LoadPathAsync(@"Z:\paloma-tests\missing-b")));
    }
}