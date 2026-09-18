using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;

namespace Hover;

public partial class App : Application
{
    public override void Initialize() => AvaloniaXamlLoader.Load(this);

    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            // No main window: Hover lives in the system tray, and its two panels are
            // edge windows that come and go. Closing a window must not end the app.
            desktop.ShutdownMode = ShutdownMode.OnExplicitShutdown;
            Services.Tray.Install(this);
        }

        base.OnFrameworkInitializationCompleted();
    }
}
