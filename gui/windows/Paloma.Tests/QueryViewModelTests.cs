using Paloma.Helpers;
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
    public void GivenImagesWhenComposingShouldNumberPlaceholdersAndAttachmentsTogetherAndSkipUnknown()
    {
        var vm = new QueryViewModel();
        var png = vm.Add(Png);
        var jpeg = vm.Add(Jpeg);
        var keys = new Dictionary<int, string> { [1] = jpeg, [3] = "missing", [4] = png, [5] = png };

        var (prompt, attachments) = vm.Compose($"a{Images.ImageCharacter}b{Images.ImageCharacter}{Images.ImageCharacter}{Images.ImageCharacter}", position => keys[position]);

        Assert.Equal("a[Image #1]b[Image #2][Image #3]", prompt);
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
    }

    [Fact]
    public void GivenTextWithoutImagesWhenComposingShouldKeepItAsIs()
    {
        var (prompt, attachments) = new QueryViewModel().Compose("a\rb", _ => throw new InvalidOperationException());

        Assert.Equal("a\rb", prompt);
        Assert.Empty(attachments);
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
    public void GivenAddedImageWhenClearedShouldForgetIt()
    {
        var vm = new QueryViewModel();
        var key = vm.Add(Png);

        vm.Clear();

        Assert.False(vm.TryGetImage(key, out _));
        var (prompt, attachments) = vm.Compose($"{Images.ImageCharacter}", _ => key);
        Assert.Empty(prompt);
        Assert.Empty(attachments);
    }
}
