using Paloma.Models;
using Paloma.ViewModels.Overlay;
using Xunit;
using UserPromptAttachment = PalomaCore.UserPromptAttachment;

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
    public void GivenKeysWhenGettingAttachmentsShouldNumberThemInKeyOrderAndSkipUnknown()
    {
        var vm = new QueryViewModel();
        var png = vm.Add(Png);
        var jpeg = vm.Add(Jpeg);

        var attachments = vm.Attachments([jpeg, "missing", png, png]);

        Assert.Equal(3, attachments.Length);
        var first = Assert.IsType<UserPromptAttachment.Image>(attachments[0]);
        Assert.Equal(1u, first.Id);
        Assert.Equal("image/jpeg", first.MediaType);
        Assert.Same(Jpeg.Data, first.Data);
        var second = Assert.IsType<UserPromptAttachment.Image>(attachments[1]);
        Assert.Equal(2u, second.Id);
        Assert.Equal("image/png", second.MediaType);
        Assert.Same(Png.Data, second.Data);
        var third = Assert.IsType<UserPromptAttachment.Image>(attachments[2]);
        Assert.Equal(3u, third.Id);
        Assert.Same(Png.Data, third.Data);
        Assert.Empty(vm.Attachments([]));
    }

    [Fact]
    public void GivenAddedImageWhenLookingUpItsKeyShouldReturnIt()
    {
        var vm = new QueryViewModel();
        var key = vm.Add(Png);

        Assert.True(vm.TryGetImage(key, out var image));
        Assert.Same(Png, image);
    }

    [Fact]
    public void GivenUnknownKeyWhenLookingUpShouldFindNothing()
    {
        var vm = new QueryViewModel();
        vm.Add(Png);

        Assert.False(vm.TryGetImage("missing", out _));
    }

    [Fact]
    public void GivenAddedImageWhenClearedShouldNotFindIt()
    {
        var vm = new QueryViewModel();
        var key = vm.Add(Png);
        vm.Clear();

        Assert.False(vm.TryGetImage(key, out _));
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
