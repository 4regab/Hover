using System.Windows;
using System.Windows.Media;
using Hover.Core;
using Microsoft.Win32;

namespace Hover.Owl;

/// The colours in use: Hover's own light or dark, or a theme taken from VS Code.
/// Code-built elements take Ui's brushes when they are made, so a switch raises
/// Changed and the views are built again; the XAML styles read the same colours as
/// dynamic resources, which Publish rewrites in place.
internal static class Theme
{
    public static Palette Current { get; private set; } = Palette.HoverDark;
    public static bool Dark => Current.Dark;
    public static event Action? Changed;

    public static void Start()
    {
        Current = Resolve();
        Publish();
        // Windows' own light/dark switch, for Appearance = System.
        SystemEvents.UserPreferenceChanged += (_, e) =>
        {
            if (e.Category == UserPreferenceCategory.General)
                Application.Current?.Dispatcher.BeginInvoke(Refresh);
        };
    }

    /// Settings changed, or Windows did. Rebuilds only when the result differs.
    public static void Refresh()
    {
        var p = Resolve();
        if (p == Current) return;
        Current = p;
        Publish();
        Changed?.Invoke();
    }

    private static Palette Resolve()
    {
        if (Settings.Theme is { } t) return Palette.From(t);
        var dark = Settings.Appearance switch
        {
            Appearance.Light => false,
            Appearance.Dark => true,
            _ => SystemDark(),
        };
        return dark ? Palette.HoverDark : Palette.HoverLight;
    }

    /// Windows keeps "app mode" per user; a missing value means the light default.
    public static bool SystemDark()
    {
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
            return key?.GetValue("AppsUseLightTheme") is int v && v == 0;
        }
        catch { return false; }
    }

    /// The colours the XAML styles (Owl.xaml, Styles.xaml) look up by key. Setting
    /// them on the application overrides the dark defaults in those dictionaries.
    private static void Publish()
    {
        var r = Application.Current?.Resources;
        if (r is null) return;
        var p = Current;
        void Set(string key, uint argb) => r[key] = Frozen(argb);

        // Label and secondary label, the hover shade, the accent and its focus ring,
        // the grey fills, and the selected segment of a segmented control.
        Set("Owl.Ink", p.Ink);
        Set("Owl.InkDim", p.InkDim);
        Set("Owl.Lift", p.Ink);
        Set("Owl.Accent", p.Blue);
        Set("Owl.Focus", Palette.Alpha(p.Blue, 0xB3));
        Set("Owl.Fill", p.Fill);
        Set("Owl.Thumb", p.Thumb);
        Set("Owl.RowHover", p.RowHover);
        Set("Owl.SwitchOff", p.SwitchOff);
        Set("Owl.SwitchOn", p.Green);
        Set("Owl.Handle", p.Handle);

        Set("MenuPaper", p.Sheet);
        Set("MenuEdge", p.SheetEdge);
        Set("MenuInk", p.Ink);
        Set("MenuInkDim", p.InkDim);
        Set("MenuInkFaint", p.InkFaint);
        Set("MenuHighlight", p.Blue);
        Set("MenuRule", p.Separator);
    }

    private static Brush Frozen(uint argb)
    {
        var b = new SolidColorBrush(Ui.Argb(argb));
        b.Freeze();
        return b;
    }
}
