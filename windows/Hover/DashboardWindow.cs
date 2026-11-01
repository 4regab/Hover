using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Hover.Windowing;
using Windows.Graphics;

namespace Hover;

/// <summary>
/// The dashboard, as small as it can be: the name, how the backend is doing and where the
/// data lives, and the place the office goes. Built in code; there are no .xaml files.
/// </summary>
public sealed class DashboardWindow : Window
{
    readonly TextBlock status;

    /// <summary>The element the pictures draw.</summary>
    public FrameworkElement Root { get; }

    /// <summary>
    /// Where the office is drawn in this window (the Go app shows it here as well as in the notch).
    /// It is the Direct3D 11 view's place; until that exists the frame says so.
    /// </summary>
    public Border OfficeArea { get; }

    /// <summary>True while this window has the keyboard.</summary>
    public bool IsActive { get; private set; }

    public DashboardWindow(string dataDir, BackendSession backend, OfficeHost? office = null)
    {
        Title = "Hover";
        AppWindow.Resize(new SizeInt32(960, 600));
        Activated += (_, e) => IsActive = e.WindowActivationState != WindowActivationState.Deactivated;

        status = new TextBlock { Text = backend.Status, TextWrapping = TextWrapping.Wrap };
        var header = new StackPanel { Spacing = 6, Padding = new Thickness(32, 24, 32, 12) };
        header.Children.Add(new TextBlock { Text = "Hover", FontSize = 32, FontWeight = FontWeights.SemiBold });
        header.Children.Add(status);
        header.Children.Add(new TextBlock
        {
            Text = $"Data folder: {dataDir}",
            TextWrapping = TextWrapping.Wrap,
            Opacity = 0.7,
        });

        OfficeArea = new Border
        {
            Margin = new Thickness(32, 0, 32, 32),
            CornerRadius = new CornerRadius(16),
            BorderThickness = new Thickness(1.5),
            BorderBrush = new SolidColorBrush(Color(0x40, 0x80, 0x80, 0x80)),
            Child = new StackPanel
            {
                VerticalAlignment = VerticalAlignment.Center,
                HorizontalAlignment = HorizontalAlignment.Center,
                Spacing = 4,
                Children =
                {
                    new TextBlock { Text = "The office draws here", FontSize = 15, FontWeight = FontWeights.Medium, Opacity = 0.65, HorizontalAlignment = HorizontalAlignment.Center },
                    new TextBlock { Text = "Direct3D 11 view", FontSize = 12, Opacity = 0.45, HorizontalAlignment = HorizontalAlignment.Center },
                },
            },
        };

        // The window's own background is not part of the element tree, so the pictures draw this.
        var page = new Grid { Background = (Brush)Application.Current.Resources["ApplicationPageBackgroundThemeBrush"] };
        page.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        page.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        page.Children.Add(header);
        Grid.SetRow(OfficeArea, 1);
        page.Children.Add(OfficeArea);
        Root = page;
        Content = page;

        backend.Changed += () => DispatcherQueue.TryEnqueue(() => status.Text = backend.Status);
        office?.AttachDashboard(OfficeArea, WinRT.Interop.WindowNative.GetWindowHandle(this));
    }

    static Windows.UI.Color Color(byte a, byte r, byte g, byte b) => Windows.UI.Color.FromArgb(a, r, g, b);
}
