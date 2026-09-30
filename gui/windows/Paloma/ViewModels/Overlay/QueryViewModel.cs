using Paloma.Models;
using UserPromptAttachment = PalomaCore.UserPromptAttachment;

namespace Paloma.ViewModels.Overlay;

public sealed class QueryViewModel
{
    private readonly Dictionary<string, InlineImage> _images = [];

    public string Add(InlineImage image)
    {
        var key = Guid.NewGuid().ToString("N");
        _images[key] = image;
        return key;
    }

    public UserPromptAttachment[] Attachments(IEnumerable<string> keys)
    {
        var attachments = new List<UserPromptAttachment>();
        foreach (var key in keys)
        {
            if (_images.TryGetValue(key, out var image))
            {
                attachments.Add(new UserPromptAttachment.Image((uint)attachments.Count + 1, image.MediaType, image.Data));
            }
        }

        return [.. attachments];
    }

    public void Clear()
    {
        _images.Clear();
    }
}
