namespace Hover;

/// <summary>What the command line asked for.</summary>
public sealed record LaunchOptions
{
    /// <summary>
    /// <c>--shots DIR</c>: draw the app's views into JPEG files in DIR, then exit. It does not
    /// claim the single instance, so it can run beside a Hover that is open.
    /// </summary>
    public string? ShotsDirectory { get; init; }

    /// <summary>
    /// <c>--background</c>: start without the app window (the default now that the notch and
    /// the tray exist; kept so a command line that has it still works). A later launch opens the dashboard.
    /// </summary>
    public bool Background { get; init; }

    /// <summary>
    /// <c>--dashboard</c>: open the app window at start. Without it the app starts as the Go app
    /// does, with the notch and the tray icon, and the window opens from the tray.
    /// </summary>
    public bool Dashboard { get; init; }

    /// <summary><c>--settings</c>: open the app window on Settings (as the tray menu's Settings… does).</summary>
    public bool Settings { get; init; }

    public static LaunchOptions Parse(string[] args)
    {
        string? shots = null;
        var background = false;
        var dashboard = false;
        var settings = false;
        for (var i = 0; i < args.Length; i++)
        {
            switch (args[i])
            {
                case "--shots" when i + 1 < args.Length:
                    shots = Path.GetFullPath(args[++i]);
                    break;
                case "--background":
                    background = true;
                    break;
                case "--dashboard":
                    dashboard = true;
                    break;
                case "--settings":
                    settings = true;
                    break;
            }
        }
        return new LaunchOptions { ShotsDirectory = shots, Background = background, Dashboard = dashboard, Settings = settings };
    }
}
