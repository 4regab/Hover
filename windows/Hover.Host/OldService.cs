using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.Versioning;

namespace Hover.Host;

/// <summary>
/// Earlier versions of Hover installed a scheduled task, "Hover Service", that started
/// `hoverai --service` at log-on. There is no service now. The first start after an update
/// undoes what the installer made, with no window and no question, as the Go app does
/// (<c>app.removeOldService</c>). The marker file is written only once the task is gone, so a
/// removal that failed is tried again at the next start.
/// </summary>
[SupportedOSPlatform("windows")]
public static class OldService
{
    public const string TaskName = "Hover Service";
    const string MarkerText = "The background service of earlier versions was looked for and is not there.\n";

    /// <summary>Does nothing when the marker is there. Call it without waiting; it logs what it did.</summary>
    public static async Task RemoveOnceAsync(string dataDir, string taskName = TaskName)
    {
        var marker = DataFolder.OldServiceMarker(dataDir);
        if (File.Exists(marker)) return;
        var (said, gone) = await RemoveAsync(taskName).ConfigureAwait(false);
        HostLog.Line($"old background service: {said}");
        if (gone) await File.WriteAllTextAsync(marker, MarkerText).ConfigureAwait(false);
    }

    /// <summary>What was done, and whether the task is gone now.</summary>
    public static async Task<(string Said, bool Gone)> RemoveAsync(string taskName = TaskName)
    {
        try
        {
            if ((await Schtasks("/Query", "/TN", taskName)).ExitCode != 0) return ("not installed", true);
            await Schtasks("/End", "/TN", taskName);
            var (code, error) = await Schtasks("/Delete", "/TN", taskName, "/F");
            if (code == 0) return ($"removed the scheduled task “{taskName}”", true);
            var line = error.Split('\n')[0].Trim();
            return ("couldn’t remove the scheduled task: " + (line == "" ? "failed" : line), false);
        }
        catch (Win32Exception e)
        {
            return ($"schtasks didn’t start ({e.Message}), so it was not looked for", false);
        }
    }

    static async Task<(int ExitCode, string Error)> Schtasks(params string[] args)
    {
        var info = new ProcessStartInfo("schtasks")
        {
            UseShellExecute = false, CreateNoWindow = true,
            RedirectStandardOutput = true, RedirectStandardError = true,
        };
        foreach (var a in args) info.ArgumentList.Add(a);
        using var process = Process.Start(info)!;
        _ = process.StandardOutput.ReadToEndAsync();
        var error = process.StandardError.ReadToEndAsync();
        await process.WaitForExitAsync().ConfigureAwait(false);
        return (process.ExitCode, await error.ConfigureAwait(false));
    }
}
