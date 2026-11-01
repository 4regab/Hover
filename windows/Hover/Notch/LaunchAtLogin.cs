namespace Hover.Windowing;

/// <summary>
/// "Launch at Login": the Run value the Go app writes and reads, <c>Hover</c> under
/// HKCU\Software\Microsoft\Windows\CurrentVersion\Run, holding the quoted path of the program.
/// </summary>
static class LaunchAtLogin
{
    // The value itself is Hover.Settings' RunValue, which Settings → General writes too.
    static readonly Hover.Settings.RunValue Run = new();

    public static bool Enabled => Run.Enabled;

    public static void Set(bool on) => Run.Set(on);
}
