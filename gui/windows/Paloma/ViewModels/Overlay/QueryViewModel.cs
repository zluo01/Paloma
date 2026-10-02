using System.Diagnostics.CodeAnalysis;
using System.Text;
using Paloma.Helpers;
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

    public bool TryGetImage(string key, [MaybeNullWhen(false)] out InlineImage image)
    {
        return _images.TryGetValue(key, out image);
    }

    public (string Prompt, UserPromptAttachment[] Attachments) Compose(string text, Func<int, string> keyAt)
    {
        var prompt = new StringBuilder(text.Length);
        var attachments = new List<UserPromptAttachment>();
        for (var i = 0; i < text.Length; i++)
        {
            if (text[i] != Images.ImageCharacter)
            {
                prompt.Append(text[i]);
                continue;
            }

            if (_images.TryGetValue(keyAt(i), out var image))
            {
                var id = (uint)attachments.Count + 1;
                attachments.Add(new UserPromptAttachment.Image(id, image.MediaType, image.Data));
                prompt.Append("[Image #").Append(id).Append(']');
            }
        }

        return (prompt.ToString(), [.. attachments]);
    }

    public void Clear()
    {
        _images.Clear();
    }
}
