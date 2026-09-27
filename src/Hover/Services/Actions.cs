using System.Windows;
using System.Windows.Controls;
using Hover.Core;
using Hover.Owl;

namespace Hover.Services;

/// Everything the tray icon's menu and the settings can ask the app to do.
public static class Actions
{
    /// Raised only when the shortcut is actually rebound. Re-registering a global
    /// hotkey means letting go of it and taking it again.
    public static Action? OnShortcutsChanged { get; set; }

    public static void Quit() => Application.Current.Shutdown();

    /// A shortcut was rebound: hand the whole set back and take it again.
    public static void ShortcutsChanged() => OnShortcutsChanged?.Invoke();

    // MARK: The tray icon's menu

    public static ContextMenu BuildMainMenu()
    {
        var menu = new ContextMenu();
        menu.Items.Add(Item($"Open Workspace  {Settings.ScWorkspace}", () => OwlApp.ShowWorkspace?.Invoke()));
        menu.Items.Add(Item("Open App Window", () => OwlApp.OpenDashboard?.Invoke()));
        menu.Items.Add(new Separator());
        menu.Items.Add(Check("Launch at Login", Settings.LaunchAtLogin, () => Settings.LaunchAtLogin = !Settings.LaunchAtLogin));
        menu.Items.Add(new Separator());
        menu.Items.Add(Item("Settings…", () => OwlApp.OpenSettings?.Invoke()));
        menu.Items.Add(Item("Quit Hover", Quit));
        return menu;
    }

    private static MenuItem Item(string header, Action action)
    {
        var item = new MenuItem { Header = header };
        item.Click += (_, _) => action();
        return item;
    }

    private static MenuItem Check(string header, bool on, Action action)
    {
        var item = new MenuItem { Header = header, IsCheckable = true, IsChecked = on };
        item.Click += (_, _) => action();
        return item;
    }
}
