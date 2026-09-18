using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using Hover.Images;

namespace Hover;

public partial class App : Application
{
    /// Launch with this to open the new screenshot tray on its own. Scaffolding for
    /// the rewrite: it goes once the tray has its real edge window.
    private const string PreviewTrayFlag = "--preview-tray";

    public override void Initialize() => AvaloniaXamlLoader.Load(this);

    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            var args = desktop.Args ?? Array.Empty<string>();
            if (args.Contains(PreviewTrayFlag, StringComparer.OrdinalIgnoreCase))
            {
                // A preview window is the only way in, so closing it closes the app.
                desktop.ShutdownMode = ShutdownMode.OnMainWindowClose;
                desktop.MainWindow = new TrayPreviewWindow();
            }
            else
            {
                // No main window: Hover lives in the system tray. The edge panels are
                // created on demand, and are not moved over yet.
                desktop.ShutdownMode = ShutdownMode.OnExplicitShutdown;
                Services.Tray.Install(this);
            }
        }

        base.OnFrameworkInitializationCompleted();
    }
}
