using Microsoft.UI.Xaml.Data;
using HealthLevel = PalomaCore.HealthLevel;

namespace Paloma.Extensions;

public sealed partial class HealthToLabelConverter : IValueConverter
{
    public object Convert(object value, Type targetType, object parameter, string language) =>
        $"{parameter}: " + value switch
        {
            HealthLevel.Healthy => "healthy",
            HealthLevel.Degraded => "degraded",
            HealthLevel.Down => "down",
            _ => "not configured",
        };

    public object ConvertBack(object value, Type targetType, object parameter, string language) =>
        throw new NotSupportedException();
}