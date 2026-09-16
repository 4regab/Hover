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

        var appData = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
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

    public static string Db => Path.Combine(Support, "notes.db");
    public static string Key => Path.Combine(Support, "note.key");
    public static string SettingsFile => Path.Combine(Support, "settings.json");
    public static string Log => Path.Combine(Support, "hover.log");

    /// The image tray's folder — plain PNGs under Pictures, so a picture dragged out
    /// of the tray is a real file any other app can take. An env override points it
    /// elsewhere for tests. Made on first use.
    public static string Shots
    {
        get
        {
            var overridden = Environment.GetEnvironmentVariable("HOVER_SHOTS_DIR");
            var dir = string.IsNullOrWhiteSpace(overridden)
                ? Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.MyPictures),
                    "Hover Shots")
                : Path.GetFullPath(overridden);
            Directory.CreateDirectory(dir);
            return dir;
        }
    }
}
