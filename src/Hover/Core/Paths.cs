using System.IO;
using System.Runtime.InteropServices;

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

    /// The image tray's folder — plain PNGs on disk, so a picture dragged out of the
    /// tray is a real file any other app can take. Made on first use.
    ///
    /// Order of precedence: the test override, then the folder chosen in Settings,
    /// then the default under Downloads.
    public static string Shots
    {
        get
        {
            var dir = Wanted();
            try
            {
                Directory.CreateDirectory(dir);
                return dir;
            }
            catch (Exception e)
            {
                // The chosen folder may be on a drive that is no longer attached, or
                // may have been deleted. Fall back rather than throw, because every
                // tray redraw reads this. Logged once per path for the same reason.
                //
                // Log is qualified because this class has its own Log property, which
                // is the path of the log file.
                if (_warnedShotsFolder != dir)
                {
                    _warnedShotsFolder = dir;
                    Hover.Core.Log.Line($"shots folder unusable, using the default — {dir} — {e.Message}");
                }
                var fallback = DefaultShots;
                Directory.CreateDirectory(fallback);
                return fallback;
            }
        }
    }

    private static string? _warnedShotsFolder;

    private static string Wanted()
    {
        var overridden = Environment.GetEnvironmentVariable("HOVER_SHOTS_DIR");
        if (!string.IsNullOrWhiteSpace(overridden)) return Path.GetFullPath(overridden);

        var chosen = Settings.ShotsFolder;
        return string.IsNullOrWhiteSpace(chosen) ? DefaultShots : Path.GetFullPath(chosen);
    }

    /// Downloads rather than Pictures. Pictures is commonly redirected to a cloud sync
    /// folder, which would upload every screenshot. Downloads stays local by default.
    public static string DefaultShots => Path.Combine(
        KnownFolder(FolderIdDownloads)
            ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "Downloads"),
        "Hover Shots");

    /// Where earlier builds kept pictures. Read only when moving them to the current
    /// folder. Never created.
    public static string LegacyPictureShots => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.MyPictures), "Hover Shots");

    // Downloads has no Environment.SpecialFolder value, so the shell has to supply it.
    // This P/Invoke stays here rather than in Interop because Core has no other
    // dependency on the Win32 layer.
    private static readonly Guid FolderIdDownloads = new("374DE290-123F-4565-9164-39C4925E467B");

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    private static extern int SHGetKnownFolderPath(in Guid id, uint flags, IntPtr token, out IntPtr path);

    private static string? KnownFolder(Guid id)
    {
        var ptr = IntPtr.Zero;
        try
        {
            if (SHGetKnownFolderPath(id, 0, IntPtr.Zero, out ptr) != 0) return null;
            var path = Marshal.PtrToStringUni(ptr);
            return string.IsNullOrWhiteSpace(path) ? null : path;
        }
        catch
        {
            return null;
        }
        finally
        {
            if (ptr != IntPtr.Zero) Marshal.FreeCoTaskMem(ptr);
        }
    }
}
