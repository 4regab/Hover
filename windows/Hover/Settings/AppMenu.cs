using System.Diagnostics;
using Hover.Chatting;
using Hover.Host;
using Hover.Settings;
using Microsoft.UI.Xaml.Controls;

namespace Hover.Configuring;

/// <summary>
/// The app window's File, Settings and Help menus (<c>internal/ui/barmenu.go</c>): the same items in the same order, with the same
/// links. They are the system's own menu bar; Settings opens in the window.
/// </summary>
static class AppMenu
{
    /// <summary>What the menus ask of the window.</summary>
    public sealed record Actions(Action NewChat, Action OpenFolder, Action CloseWindow, Action<Section> OpenSettings, string DataFolder);

    static readonly (string Name, string Url, bool Rule)[] Help =
    [
        ("Documentation", "https://tryhover.co/docs", false),
        ("Releases", "https://github.com/4regab/Hover/releases", false),
        ("Report a bug", "https://github.com/4regab/Hover/issues/new?template=bug_report.yml", true),
        ("Request a feature", "https://github.com/4regab/Hover/issues/new?template=feature_request.yml", false),
    ];

    /// <summary>The version the program was built with (the VERSION file's).</summary>
    public static string Version => typeof(AppMenu).Assembly.GetCustomAttributes(typeof(System.Reflection.AssemblyInformationalVersionAttribute), false)
        is [System.Reflection.AssemblyInformationalVersionAttribute v, ..] ? v.InformationalVersion.Split('+')[0] : "";

    static MenuFlyoutItem Item(string text, Action act)
    {
        var item = new MenuFlyoutItem { Text = text };
        item.Click += (_, _) =>
        {
            try { act(); }
            catch (Exception e) { HostLog.Line($"menu {text}: {e}"); }
        };
        return item;
    }

    public static MenuBar Build(Actions a)
    {
        var bar = new MenuBar();

        var file = new MenuBarItem { Title = "File" };
        file.Items.Add(Item("New chat", a.NewChat));
        file.Items.Add(Item("Open folder…", a.OpenFolder));
        file.Items.Add(new MenuFlyoutSeparator());
        file.Items.Add(Item("Close window", a.CloseWindow));
        bar.Items.Add(file);

        var settings = new MenuBarItem { Title = "Settings" };
        foreach (var s in new[] { Section.General, Section.Integrations, Section.Projects, Section.Voice })
        {
            var section = s;
            settings.Items.Add(Item(section.Title(), () => a.OpenSettings(section)));
        }
        settings.Items.Add(new MenuFlyoutSeparator());
        settings.Items.Add(new MenuFlyoutItem { Text = "AGENTS", IsEnabled = false });
        foreach (var t in Tools.All)
        {
            var section = Sections.Of(t);
            settings.Items.Add(Item(t.Name, () => a.OpenSettings(section)));
        }
        bar.Items.Add(settings);

        var help = new MenuBarItem { Title = "Help" };
        foreach (var (name, url, rule) in Help)
        {
            if (rule) help.Items.Add(new MenuFlyoutSeparator());
            help.Items.Add(Item(name, () => OpenUrl(url)));
        }
        help.Items.Add(new MenuFlyoutSeparator());
        help.Items.Add(Item("Open the logs folder", () => OpenFolder(a.DataFolder)));
        if (Version is { Length: > 0 } version) help.Items.Add(new MenuFlyoutItem { Text = "Hover " + version, IsEnabled = false });
        bar.Items.Add(help);
        return bar;
    }

    static void OpenUrl(string url)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri) || uri.Scheme is not ("http" or "https")) return;
        Process.Start(new ProcessStartInfo(uri.AbsoluteUri) { UseShellExecute = true });
    }

    static void OpenFolder(string folder)
    {
        if (Directory.Exists(folder)) Process.Start(new ProcessStartInfo(folder) { UseShellExecute = true });
    }
}
