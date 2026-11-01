using Microsoft.Win32;

namespace Hover.Windowing;

/// <summary>
/// "Launch at Login": the Run value the Go app writes and reads, <c>Hover</c> under
/// HKCU\Software\Microsoft\Windows\CurrentVersion\Run, holding the quoted path of the program.
/// </summary>
static class LaunchAtLogin
{
    const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    const string ValueName = "Hover";

    public static bool Enabled
    {
        get
        {
            using var key = Registry.CurrentUser.OpenSubKey(RunKey);
            return key?.GetValue(ValueName) is string { Length: > 0 };
        }
    }

    public static void Set(bool on)
    {
        if (!on)
        {
            using var existing = Registry.CurrentUser.OpenSubKey(RunKey, writable: true);
            existing?.DeleteValue(ValueName, throwOnMissingValue: false);
            return;
        }
        var exe = Environment.ProcessPath;
        if (exe is null) return;
        using var key = Registry.CurrentUser.CreateSubKey(RunKey, writable: true);
        key.SetValue(ValueName, "\"" + exe + "\"", RegistryValueKind.String);
    }
}
