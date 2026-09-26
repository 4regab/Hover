using System.Diagnostics;
using System.IO;

namespace Hover.Core;

/// One line per event, to %APPDATA%\Hover\hover.log and the debugger.
public static class Log
{
    private static readonly object Gate = new();

    public static void Line(string message)
    {
        var stamp = $"{DateTime.Now:HH:mm:ss.fff} hover: {message}";
        Debug.WriteLine(stamp);
        try
        {
            lock (Gate) File.AppendAllText(Paths.Log, stamp + Environment.NewLine);
        }
        catch { /* logging must never take the app down */ }
    }
}
