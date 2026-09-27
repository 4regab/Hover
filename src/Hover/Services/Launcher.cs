using System.Diagnostics;
using System.IO;
using Hover.Core;

namespace Hover.Services;

/// Runs a header button's command in a new terminal: a Windows Terminal tab when it
/// is installed, otherwise a PowerShell window. The shell stays open afterwards, so
/// the tool's output, or why it failed, can still be read.
internal static class Launcher
{
    public static void Run(LaunchButton button)
    {
        var dir = Folder(button.Folder);
        var shell = Quota.OnPath("pwsh") ?? "powershell.exe";
        string[] run = { shell, "-NoLogo", "-NoExit", "-Command", button.Command };
        ProcessStartInfo psi;
        if (Quota.OnPath("wt") is { } wt)
        {
            psi = new ProcessStartInfo(wt);
            foreach (var a in new[] { "-d", dir, "--title", button.Name }) psi.ArgumentList.Add(a);
            // Windows Terminal reads a bare ";" as the start of its next command.
            foreach (var a in run) psi.ArgumentList.Add(a.Replace(";", "\\;"));
        }
        else
        {
            psi = new ProcessStartInfo(shell) { WorkingDirectory = dir };
            foreach (var a in run.Skip(1)) psi.ArgumentList.Add(a);
        }
        psi.UseShellExecute = false;
        try
        {
            Process.Start(psi)?.Dispose();
            Log.Line($"ran button {button.Name}: {button.Command}");
        }
        catch (Exception e)
        {
            Log.Line($"button {button.Name} failed to start — {e.Message}");
        }
    }

    /// The button's folder, with %VARIABLES% expanded; the user's folder when it is
    /// empty or no longer there.
    public static string Folder(string? folder)
    {
        var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        if (string.IsNullOrWhiteSpace(folder)) return home;
        var dir = Environment.ExpandEnvironmentVariables(folder.Trim().Trim('"'));
        return Directory.Exists(dir) ? dir : home;
    }
}
