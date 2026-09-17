using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace Hover.Windows;

/// A pasted picture shown full size. The bitmap is already decrypted in memory —
/// nothing is written to disk in the clear. Esc or a click closes it.
public sealed class ImagePreviewWindow : Window
{
    private ImagePreviewWindow(BitmapSource image, Window? owner)
    {
        Title = "Hover — Image";
        Owner = owner;
        Background = new SolidColorBrush(Color.FromRgb(0x14, 0x14, 0x16));
        WindowStartupLocation = owner is null
            ? WindowStartupLocation.CenterScreen
            : WindowStartupLocation.CenterOwner;

        // Never open bigger than the screen it lands on; the image scales down to fit
        // and never up past its own pixels.
        var area = SystemParameters.WorkArea;
        Width = Math.Min(image.Width + 48, area.Width * 0.9);
        Height = Math.Min(image.Height + 48, area.Height * 0.9);

        Content = new Image
        {
            Source = image,
            Stretch = Stretch.Uniform,
            StretchDirection = StretchDirection.DownOnly,
            Margin = new Thickness(12),
        };

        MouseLeftButtonUp += (_, _) => Close();
        PreviewKeyDown += (_, e) =>
        {
            if (e.Key == Key.Escape) Close();
        };
    }

    public static void Show(BitmapSource image, Window? owner)
    {
        var window = new ImagePreviewWindow(image, owner);
        window.Show();
        window.Activate();
    }
}
