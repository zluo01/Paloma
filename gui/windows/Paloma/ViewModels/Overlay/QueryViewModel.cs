using Paloma.Models;

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

    public List<InlineImage> Attachments(IEnumerable<string> keys)
    {
        var images = new List<InlineImage>();
        foreach (var key in keys)
        {
            if (_images.TryGetValue(key, out var image))
            {
                images.Add(image);
            }
        }

        return images;
    }

    public void Clear()
    {
        _images.Clear();
    }
}
