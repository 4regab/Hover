using Avalonia;
using HoverAvalonia.Ui;

namespace HoverAvalonia;

static class Program
{
    public static Options Opt = new();

    [STAThread]
    public static int Main(string[] args)
    {
        // hover-measure launches the exe with no arguments; HOVER_PROTO_ARGS carries them for scripted runs.
        args = args.Concat((Environment.GetEnvironmentVariable("HOVER_PROTO_ARGS") ?? "").Split(' ', StringSplitOptions.RemoveEmptyEntries)).ToArray();
        string? A(string n) { int i = Array.IndexOf(args, n); return i >= 0 && i + 1 < args.Length ? args[i + 1] : null; }
        bool F(string n) => args.Contains(n);
        Opt = new Options(
            Preset: A("--preset") ?? "demo", Bots: int.Parse(A("--bots") ?? "3"), StartOpen: F("--open"), ChatOpen: F("--chat"),
            DropMs: int.Parse(A("--drop-ms") ?? "30000"), Fps: int.Parse(A("--fps") ?? "30"), Selftest: A("--selftest"), ShotDir: A("--shots"),
            Backdrop: F("--backdrop"), NoHotkey: F("--no-hotkey"), Day: F("--day"), ViewCal: double.Parse(A("--view-cal") ?? "1.0", System.Globalization.CultureInfo.InvariantCulture));
        // This sandbox has no GPU: Mesa's llvmpipe is the only GL, and Avalonia refuses it by default (renderer blacklist). Real GPUs need no flag.
        if (F("--software-gl")) Environment.SetEnvironmentVariable("AVALONIA_GLX_IGNORE_RENDERER_BLACKLIST", "1");
        if (F("--version")) { Console.WriteLine("hover-avalonia prototype"); return 0; }
        if (Environment.GetEnvironmentVariable("HOVER_LOG") == "1") System.Diagnostics.Trace.Listeners.Add(new System.Diagnostics.ConsoleTraceListener(true));
        return BuildAvaloniaApp().StartWithClassicDesktopLifetime(args);
    }

    public static AppBuilder BuildAvaloniaApp() => AppBuilder.Configure<App>().UsePlatformDetect().LogToTrace(Environment.GetEnvironmentVariable("HOVER_LOG") == "1" ? Avalonia.Logging.LogEventLevel.Information : Avalonia.Logging.LogEventLevel.Warning);
}
