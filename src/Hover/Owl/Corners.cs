using System.Globalization;
using System.Windows;
using System.Windows.Data;

namespace Hover.Owl;

/// A button's corner radius, from its Tag, capped at half its height and width.
/// WPF's Border draws a radius larger than that as an ellipse, not a pill, so a
/// large Tag (99) on a wide button gives a pill and on a square one a circle.
/// Bound in Owl.xaml as (Tag, ActualHeight, ActualWidth).
public sealed class CapsuleCorners : IMultiValueConverter
{
    public object Convert(object[] values, Type targetType, object parameter, CultureInfo culture)
    {
        var r = values[0] switch
        {
            CornerRadius c => c.TopLeft,
            double d => d,
            int i => i,
            string s when double.TryParse(s, NumberStyles.Float, CultureInfo.InvariantCulture, out var d) => d,
            _ => 0,
        };
        var h = values[1] is double hh ? hh : 0;
        var w = values[2] is double ww ? ww : 0;
        var cap = Math.Min(h, w) / 2;
        return new CornerRadius(cap > 0 ? Math.Min(r, cap) : r);
    }

    public object[] ConvertBack(object value, Type[] targetTypes, object parameter, CultureInfo culture) =>
        throw new NotSupportedException();
}
