using System.Windows;
using System.Windows.Media;
using Hover.Core;
using Microsoft.Win32;

namespace Hover.Owl;

/// Light or dark. Code-built elements take Ui's brushes when they are made, so a
/// switch raises Changed and the views are built again; the XAML styles read the
/// same colours as dynamic resources, which Publish rewrites in place.
internal static class Theme
{
    public static bool Dark { get; private set; } = true;
    public static event Action? Changed;

    public static void Start()
    {
        Dark = Resolve();
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
        var dark = Resolve();
        if (dark == Dark) return;
        Dark = dark;
        Publish();
        Changed?.Invoke();
    }

    private static bool Resolve() => Settings.Appearance switch
    {
        Appearance.Light => false,
        Appearance.Dark => true,
        _ => SystemDark(),
    };

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
        var d = Dark;
        void Set(string key, uint dark, uint light) => r[key] = Frozen(d ? dark : light);

        // Apple's system colours: label, secondaryLabel, the grey fills, and the
        // selected segment of a segmented control.
        Set("Owl.Ink", 0xFFFFFFFF, 0xFF000000);
        Set("Owl.InkDim", 0x99EBEBF5, 0x993C3C43);
        Set("Owl.Lift", 0xFFFFFFFF, 0xFF000000);
        Set("Owl.Accent", 0xFF0A84FF, 0xFF007AFF);
        Set("Owl.Focus", 0xB30A84FF, 0xB3007AFF);
        Set("Owl.Fill", 0x52787880, 0x29787880);
        Set("Owl.Thumb", 0xFF636366, 0xFFFFFFFF);
        Set("Owl.RowHover", 0x14FFFFFF, 0x0D000000);
        Set("Owl.SwitchOff", 0xFF39393D, 0xFFE9E9EB);
        Set("Owl.SwitchOn", 0xFF30D158, 0xFF34C759);
        Set("Owl.Handle", 0x66FFFFFF, 0x40000000);

        Set("MenuPaper", 0xFF2C2C2E, 0xFFFFFFFF);
        Set("MenuEdge", 0x1FFFFFFF, 0x1A000000);
        Set("MenuInk", 0xFFFFFFFF, 0xFF000000);
        Set("MenuInkDim", 0x99EBEBF5, 0x993C3C43);
        Set("MenuInkFaint", 0x4DEBEBF5, 0x4D3C3C43);
        Set("MenuHighlight", 0xFF0A84FF, 0xFF007AFF);
        Set("MenuRule", 0x99545458, 0x4A3C3C43);
    }

    private static Brush Frozen(uint argb)
    {
        var b = new SolidColorBrush(Color.FromArgb((byte)(argb >> 24), (byte)(argb >> 16), (byte)(argb >> 8), (byte)argb));
        b.Freeze();
        return b;
    }
}
