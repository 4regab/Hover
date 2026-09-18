using Avalonia;
using Avalonia.Controls;
using Avalonia.Platform;
using Hover.Core;
using Hover.Images;
using Hover.Interop;

namespace Hover.Services;

/// The way back into an app with no window.
///
/// Hover has no main window, so the tray icon and one global shortcut are the whole
/// front door: take a screenshot, look at the ones already taken, or quit.
public static class Tray
{
    private static HotKeys? _keys;
    private static TrayPreviewWindow? _shots;

    /// Puts the icon in the tray and claims the screenshot shortcut. Called once at
    /// startup.
    public static void Install(Application app)
    {
        var icon = new TrayIcon
        {
            Icon = Picture(),
            ToolTipText = "Hover",
            IsVisible = true,
            Menu = Menu(),
        };
        // A left click goes straight to the screenshots, because that is the reason to
        // come back to the app.
        icon.Clicked += (_, _) => ShowShots();

        // Held by the application, or the icon is collected and vanishes from the tray.
        TrayIcon.SetIcons(app, new TrayIcons { icon });

        _keys = new HotKeys();
        if (!_keys.Register(Settings.ScSnip, Snip.Begin))
            Log.Line($"another app already owns {Settings.ScSnip} — use the tray menu instead");
    }

    private static NativeMenu Menu()
    {
        var menu = new NativeMenu();

        var snip = new NativeMenuItem($"New screenshot  {Settings.ScSnip}");
        snip.Click += (_, _) => Snip.Begin();
        menu.Add(snip);

        var shots = new NativeMenuItem("Screenshots…");
        shots.Click += (_, _) => ShowShots();
        menu.Add(shots);

        menu.Add(new NativeMenuItemSeparator());

        var quit = new NativeMenuItem("Quit Hover");
        quit.Click += (_, _) => Quit();
        menu.Add(quit);

        return menu;
    }

    /// One screenshots window, reused. Opening it twice would give two panels showing
    /// the same pictures.
    private static void ShowShots()
    {
        if (_shots is null)
        {
            _shots = new TrayPreviewWindow();
            _shots.Closed += (_, _) => _shots = null;
        }
        _shots.Show();
        _shots.Activate();
    }

    private static void Quit()
    {
        _keys?.Dispose();
        _keys = null;
        if (Application.Current?.ApplicationLifetime is
            Avalonia.Controls.ApplicationLifetimes.IClassicDesktopStyleApplicationLifetime desktop)
        {
            desktop.Shutdown();
        }
    }

    /// The app icon, read out of the app's own resources. Falls back to no icon rather
    /// than refusing to start: an app with no tray picture is still usable, an app that
    /// will not launch is not.
    private static WindowIcon? Picture()
    {
        try
        {
            using var file = AssetLoader.Open(new Uri("avares://Hover/Assets/hover.ico"));
            return new WindowIcon(file);
        }
        catch (Exception e)
        {
            Log.Line($"the tray icon could not be loaded — {e.Message}");
            return null;
        }
    }
}
