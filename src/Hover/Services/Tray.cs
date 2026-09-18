using Avalonia;
using Avalonia.Controls;
using Avalonia.Platform;
using Hover.Core;
using Hover.Images;
using Hover.Interop;

namespace Hover.Services;

/// The way back into an app with no window.
///
/// Hover has no main window, so the tray icon and a few global shortcuts are the whole
/// front door: write a note, take a screenshot, look at what is already there, or quit.
public static class Tray
{
    private static HotKeys? _keys;

    /// Puts the icon in the tray, builds the panels and claims the shortcuts. Called
    /// once at startup.
    public static void Install(Application app)
    {
        Panels.Install();

        var icon = new TrayIcon
        {
            Icon = Picture(),
            ToolTipText = "Hover",
            IsVisible = true,
            Menu = Menu(),
        };
        // A left click opens the notes, because that is the reason to come back.
        icon.Clicked += (_, _) => Panels.NewNote();

        // Held by the application, or the icon is collected and vanishes from the tray.
        TrayIcon.SetIcons(app, new TrayIcons { icon });

        _keys = new HotKeys();
        Claim(Settings.ScSnip, Snip.Begin);
        Claim(Settings.ScNewNote, Panels.NewNote);
        Claim(Settings.ScAllNotes, () => Windows.LibraryWindow.Open());
        Claim(Settings.ScArchive, () => Windows.LibraryWindow.Open(true));
    }

    private static void Claim(Shortcut shortcut, Action action)
    {
        if (_keys is null) return;
        if (!_keys.Register(shortcut, action))
            Log.Line($"another app already owns {shortcut} — use the tray menu instead");
    }

    private static NativeMenu Menu()
    {
        var menu = new NativeMenu();

        menu.Add(Item($"New note  {Settings.ScNewNote}", Panels.NewNote));
        menu.Add(Item($"New screenshot  {Settings.ScSnip}", Snip.Begin));
        menu.Add(Item("Notes", Panels.ShowNotes));
        menu.Add(Item("Screenshots", Panels.ShowShots));
        menu.Add(new NativeMenuItemSeparator());
        menu.Add(Item($"All notes  {Settings.ScAllNotes}", () => Windows.LibraryWindow.Open()));
        menu.Add(Item($"Archive  {Settings.ScArchive}", () => Windows.LibraryWindow.Open(true)));
        menu.Add(Item("Settings", Windows.SettingsWindow.Open));
        menu.Add(new NativeMenuItemSeparator());
        menu.Add(Item("Quit Hover", Quit));

        return menu;
    }

    private static NativeMenuItem Item(string header, Action click)
    {
        var item = new NativeMenuItem(header);
        item.Click += (_, _) => click();
        return item;
    }

    private static void Quit()
    {
        _keys?.Dispose();
        _keys = null;
        Panels.Dispose();
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
