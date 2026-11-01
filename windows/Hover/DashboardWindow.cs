using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.Graphics;

namespace Hover;

/// <summary>
/// The dashboard, as small as it can be: the name, how the backend is doing and where the
/// data lives. Built in code; there are no .xaml files.
/// </summary>
public sealed class DashboardWindow : Window
{
    readonly TextBlock status;

    /// <summary>The element the pictures draw.</summary>
    public FrameworkElement Root { get; }

    public DashboardWindow(string dataDir, BackendSession backend)
    {
        Title = "Hover";
        AppWindow.Resize(new SizeInt32(960, 600));

        status = new TextBlock { Text = backend.Status, TextWrapping = TextWrapping.Wrap };
        var panel = new StackPanel { Spacing = 12, Padding = new Thickness(32) };
        panel.Children.Add(new TextBlock { Text = "Hover", FontSize = 32, FontWeight = FontWeights.SemiBold });
        panel.Children.Add(status);
        panel.Children.Add(new TextBlock
        {
            Text = $"Data folder: {dataDir}",
            TextWrapping = TextWrapping.Wrap,
            Opacity = 0.7,
        });

        // The window's own background is not part of the element tree, so the pictures draw this.
        var page = new Grid { Background = (Microsoft.UI.Xaml.Media.Brush)Application.Current.Resources["ApplicationPageBackgroundThemeBrush"] };
        page.Children.Add(panel);
        Root = page;
        Content = page;

        backend.Changed += () => DispatcherQueue.TryEnqueue(() => status.Text = backend.Status);
    }
}
