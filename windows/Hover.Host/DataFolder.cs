namespace Hover.Host;

/// <summary>
/// Where Hover keeps its files. It is the folder the Go app uses, so either build opens the
/// other's settings, history and keys: <c>%APPDATA%\Hover</c>, or <c>HOVER_DATA_DIR</c>.
/// </summary>
public static class DataFolder
{
    public const string OverrideVariable = "HOVER_DATA_DIR";

    /// <summary>
    /// The data folder, made if it is missing. A blank override counts as none. Pass the
    /// parameters only in tests; the defaults read the environment.
    /// </summary>
    public static string Resolve(string? overridden = null, string? appData = null)
    {
        overridden ??= Environment.GetEnvironmentVariable(OverrideVariable);
        string dir;
        if (!string.IsNullOrWhiteSpace(overridden))
        {
            dir = Path.GetFullPath(overridden);
        }
        else
        {
            appData ??= Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
            if (string.IsNullOrEmpty(appData))
                throw new InvalidOperationException("Windows has no application data folder.");
            dir = Path.Combine(appData, "Hover");
            // The app used to be called Noty: an old folder comes across once, when there is
            // no Hover folder yet (core.ResolveDataDir does the same).
            var legacy = Path.Combine(appData, "Noty");
            if (!Directory.Exists(dir) && Directory.Exists(legacy))
            {
                try { Directory.Move(legacy, dir); }
                catch (IOException e) { Console.Error.WriteLine($"hover: data migration failed — {e.Message}"); }
            }
        }
        Directory.CreateDirectory(dir);
        return dir;
    }

    public static string KeyFile(string dir) => Path.Combine(dir, "note.key");
    public static string SettingsFile(string dir) => Path.Combine(dir, "settings.json");
    public static string SecretsFile(string dir) => Path.Combine(dir, "secrets.dat");
    public static string OldServiceMarker(string dir) => Path.Combine(dir, "old-service-removed");
    public static string LogFile(string dir) => Path.Combine(dir, "hover.log");
    public static string AgentsIndex(string dir) => Path.Combine(dir, "agents", "index.dat");
}
