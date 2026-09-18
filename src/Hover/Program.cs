using System.Threading;
using Avalonia;

namespace Hover;

internal static class Program
{
    /// Per-session, not machine-wide: two people signed in at once each get their own
    /// copy, and neither blocks the other.
    private const string OnlyOne = @"Local\Hover.SingleInstance";

    [STAThread]
    public static void Main(string[] args)
    {
        // One copy at a time. The app has no window, so a second copy would be invisible
        // except for a second tray icon and two panels fighting over the same edges.
        using var only = new Mutex(true, OnlyOne, out var first);
        if (!first) return;

        BuildAvaloniaApp().StartWithClassicDesktopLifetime(args);
    }

    public static AppBuilder BuildAvaloniaApp() =>
        AppBuilder.Configure<App>()
            .UsePlatformDetect()
            .LogToTrace();
}
