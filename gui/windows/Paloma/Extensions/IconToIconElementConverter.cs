using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Data;
using Microsoft.UI.Xaml.Media;
using Paloma.Helpers;
using Icon = PalomaCore.Icon;

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
        return icon switch
        {
            Icon.Embedded embedded => AsIcon(CapabilityIcons.DecodeEmbedded(embedded.V1)),
            Icon.Path path => AsIcon(CapabilityIcons.ImageFromPath(path.V1)),
            Icon.Name name when CapabilityIcons.IsGlyph(name.V1) => new FontIcon { Glyph = name.V1 },
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