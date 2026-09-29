using Paloma.Models;
using Paloma.ViewModels.Overlay;
using Xunit;

namespace Paloma.Tests;

public sealed class QueryViewModelTests
{
    private static readonly InlineImage Png = new("image/png", [1, 2, 3]);
    private static readonly InlineImage Jpeg = new("image/jpeg", [4, 5]);

    [Fact]
    public void GivenSameImageWhenAddedTwiceShouldGiveDistinctKeys()
    {
        var vm = new QueryViewModel();
        var first = vm.Add(Png);
        var second = vm.Add(Png);
        Assert.NotEqual(first, second);
        Assert.NotEmpty(first);
    }

    [Fact]
    public void GivenKeysWhenGettingAttachmentsShouldFollowKeyOrderAndSkipUnknown()
    {
        var vm = new QueryViewModel();
        var png = vm.Add(Png);
        var jpeg = vm.Add(Jpeg);

        Assert.Equal([Jpeg, Png, Png], vm.Attachments([jpeg, "missing", png, png]));
        Assert.Empty(vm.Attachments([]));
    }

    [Fact]
    public void GivenAddedImageWhenGettingAttachmentsShouldReturnTheSameInstance()
    {
        var vm = new QueryViewModel();
        var key = vm.Add(Png);

        var attachment = Assert.Single(vm.Attachments([key]));

        Assert.Same(Png, attachment);
        Assert.Same(Png.Data, attachment.Data);
    }

    [Fact]
    public void GivenAddedImageWhenClearedShouldClearAll()
    {
        var vm = new QueryViewModel();
        var key = vm.Add(Png);
        vm.Clear();
        Assert.Empty(vm.Attachments([key]));
    }
}
