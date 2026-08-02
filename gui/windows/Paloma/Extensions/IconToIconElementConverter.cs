using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Data;
using Microsoft.UI.Xaml.Media;
using Paloma.Helpers;
using Icon = Paloma.Binding.V1.Icon;

namespace Paloma.Extensions;

// Returns elements, not icon sources: an ImageIconSource renders blank
// anywhere outside the handful of controls with an IconSource property.
// https://learn.microsoft.com/windows/windows-app-sdk/api/winrt/microsoft.ui.xaml.controls.imageiconsource
public sealed partial class IconToIconElementConverter : IValueConverter
{
    public object? Convert(object? value, Type targetType, object parameter, string language)
    {
        return value is Icon icon ? Resolve(icon) : null;
    }

    public object ConvertBack(object? value, Type targetType, object parameter, string language) =>
        throw new NotSupportedException();

    private static IconElement Resolve(Icon icon)
    {
        return icon.IconCase switch
        {
            Icon.IconOneofCase.Embedded =>
                AsIcon(CapabilityIcons.DecodeEmbedded(icon.Embedded.ToByteArray())),
            Icon.IconOneofCase.Path => AsIcon(CapabilityIcons.ImageFromPath(icon.Path)),
            Icon.IconOneofCase.Name when CapabilityIcons.IsGlyph(icon.Name) =>
                new FontIcon { Glyph = icon.Name },
            _ => Fallback(),
        };
    }

    private static IconElement AsIcon(ImageSource? image)
    {
        return image is null ? Fallback() : new ImageIcon { Source = image };
    }

    private static FontIcon Fallback()
    {
        return new FontIcon { Glyph = "\uE897" };
    }
}