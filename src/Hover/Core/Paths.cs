using System.IO;

namespace Hover.Core;

/// Everything Hover owns lives in one folder under %APPDATA%.
public static class Paths
{
    public static string Support { get; } = Init();

    private static string Init()
    {
        var overridden = Environment.GetEnvironmentVariable("HOVER_DATA_DIR");
        if (!string.IsNullOrWhiteSpace(overridden))
        {
            var forced = Path.GetFullPath(overridden);
            Directory.CreateDirectory(forced);
            return forced;
        }

        var appData = OperatingSystem.IsMacOS()
            ? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "Library", "Application Support")
            : Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
        var dir = Path.Combine(appData, "Hover");

        // The app used to be called Noty. Bring an existing install's notes,
        // settings and key across the first time the renamed build runs, so nothing
        // is lost. Only when there is an old folder and no new one yet.
        var legacy = Path.Combine(appData, "Noty");
        if (!Directory.Exists(dir) && Directory.Exists(legacy))
        {
            // Not Log.Line here: logging resolves Paths.Log, which needs Support,
            // which is the very property this runs inside — so a failure is written
            // straight to the debugger instead.
            try { Directory.Move(legacy, dir); }
            catch (Exception e) { System.Diagnostics.Debug.WriteLine($"hover: data migration failed — {e.Message}"); }
        }

        Directory.CreateDirectory(dir);
        return dir;
    }

    public static string Key => Path.Combine(Support, "note.key");
    public static string SettingsFile => Path.Combine(Support, "settings.json");
    public static string Log => Path.Combine(Support, "hover.log");
}
