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
    /// <c>--background</c>: start without a window. A later launch opens the dashboard, as it
    /// does for a Hover that was started at login.
    /// </summary>
    public bool Background { get; init; }

    public static LaunchOptions Parse(string[] args)
    {
        string? shots = null;
        var background = false;
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
            }
        }
        return new LaunchOptions { ShotsDirectory = shots, Background = background };
    }
}
