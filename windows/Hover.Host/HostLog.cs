using System.Text;

namespace Hover.Host;

/// <summary>
/// One line per event, to <c>hover.log</c> in the data folder: the file the Go backend
/// writes to as well, in the same shape (<c>HH:mm:ss.fff hover: text</c>). Both sides open
/// it per line and let the other write, so their lines interleave instead of failing.
/// </summary>
public static class HostLog
{
    static readonly object Gate = new();

    /// <summary>Set once the data folder is known; until then lines go to stderr only.</summary>
    public static string? Path { get; set; }

    public static void Line(string message)
    {
        var stamp = $"{DateTime.Now:HH:mm:ss.fff} hover: {message}";
        try { Console.Error.WriteLine(stamp); } catch { /* a closed stderr is not an error */ }
        var file = Path;
        if (file is null) return;
        try
        {
            lock (Gate)
            {
                using var stream = new FileStream(file, FileMode.Append, FileAccess.Write, FileShare.ReadWrite);
                stream.Write(Encoding.UTF8.GetBytes(stamp + Environment.NewLine));
            }
        }
        catch
        {
            // Logging must never take the app down.
        }
    }
}
