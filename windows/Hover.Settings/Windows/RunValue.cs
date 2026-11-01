using Microsoft.Win32;

namespace Hover.Settings;

/// <summary>
/// "Launch at login": the Run value the Go app writes and reads, <c>Hover</c> under
/// HKCU\Software\Microsoft\Windows\CurrentVersion\Run, holding the quoted path of the program
/// (<c>core.SystemAutostart</c>). The backend must not do it: its own path is hover-backend.exe.
/// </summary>
public sealed class RunValue(string? subKey = null, string name = "Hover")
{
    /// <summary>Where Windows looks for what to start at login.</summary>
    public const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";

    readonly string key = subKey ?? RunKey;

    /// <summary>Whether the value is there with a path in it.</summary>
    public bool Enabled => Path is not null;

    /// <summary>The path in the value (unquoted), or null.</summary>
    public string? Path
    {
        get
        {
            if (!OperatingSystem.IsWindows()) return null;
            try
            {
                using var k = Registry.CurrentUser.OpenSubKey(key);
                return k?.GetValue(name) is string { Length: > 0 } s ? s.Trim('"') : null;
            }
            catch (Exception e) when (e is System.Security.SecurityException or UnauthorizedAccessException or IOException) { return null; }
        }
    }

    /// <summary>Writes the value with this program's path, or removes it. True when it is as asked.</summary>
    public bool Set(bool on, string? exe = null)
    {
        if (!OperatingSystem.IsWindows()) return false;
        try
        {
            if (!on)
            {
                using var existing = Registry.CurrentUser.OpenSubKey(key, writable: true);
                existing?.DeleteValue(name, throwOnMissingValue: false);
                return true;
            }
            exe ??= Environment.ProcessPath;
            if (string.IsNullOrEmpty(exe)) return false;
            using var k = Registry.CurrentUser.CreateSubKey(key, writable: true);
            k.SetValue(name, "\"" + exe + "\"", RegistryValueKind.String);
            return true;
        }
        catch (Exception e) when (e is System.Security.SecurityException or UnauthorizedAccessException or IOException) { return false; }
    }
}
