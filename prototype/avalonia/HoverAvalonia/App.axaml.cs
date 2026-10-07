using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using Avalonia.Media;
using HoverAvalonia.Ui;

namespace HoverAvalonia;

public partial class App : Application
{
    public override void Initialize() => AvaloniaXamlLoader.Load(this);

    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime d)
        {
            var o = Program.Opt;
            // --backdrop: a plain window behind the notch, in the colour the baseline screenshots use (#3a4a5e), so transparency and corners are visible.
            if (o.Backdrop)
            {
                var b = new Window { Background = new SolidColorBrush(Color.Parse("#3a4a5e")), Width = 1920, Height = 1080, Position = new PixelPoint(0, 0), WindowStartupLocation = WindowStartupLocation.Manual, WindowDecorations = WindowDecorations.None, ShowInTaskbar = false, CanResize = false };
                b.Show();
            }
            var w = new NotchWindow(o);
            d.MainWindow = w;
            w.Show();
            if (o.Selftest != null) SelfTest.Start(w, o);
        }
        base.OnFrameworkInitializationCompleted();
    }
}
