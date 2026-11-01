using System.Runtime.InteropServices;
using Hover.Host;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;

namespace Hover;

/// <summary>
/// The entry point. There is no App.xaml, so there is no generated Main either: this one
/// finds the data folder, keeps to one copy per user, and starts the WinUI application.
/// </summary>
public static class Program
{
    [STAThread]
    public static int Main(string[] args)
    {
        // Before the data folder and the single-instance check, so it answers while Hover runs.
        // (A Windows GUI program has no console: the number shows in a terminal that pipes it, as the Go app's did.)
        if (args.Contains("--version"))
        {
            Console.WriteLine("Hover " + Version);
            return 0;
        }

        var launch = LaunchOptions.Parse(args);

        string dataDir;
        try
        {
            dataDir = DataFolder.Resolve();
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or InvalidOperationException or ArgumentException or NotSupportedException)
        {
            return Fatal($"Hover can't use its data folder: {e.Message}", quiet: launch.ShotsDirectory is not null);
        }
        HostLog.Path = DataFolder.LogFile(dataDir);
        HostLog.Line($"main ({string.Join(' ', args)})");

        // Pictures do not claim the instance: they run beside an open Hover.
        SingleInstance? instance = null;
        if (launch.ShotsDirectory is null)
        {
            instance = SingleInstance.Claim(App.RequestDashboard);
            // Another copy runs, and has been asked to open its dashboard.
            if (instance is null) return 0;
        }

        // A crash in the runtime, before any window, would leave nothing behind but an exit code.
        AppDomain.CurrentDomain.UnhandledException += (_, e) => HostLog.Line($"crashed — {e.ExceptionObject}");
        try
        {
            WinRT.ComWrappersSupport.InitializeComWrappers();
            Application.Start(startup =>
            {
                try
                {
                    SynchronizationContext.SetSynchronizationContext(
                        new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread()));
                    _ = new App(launch, dataDir);
                }
                catch (Exception e)
                {
                    // An exception that leaves this callback ends the process with only an exit code.
                    HostLog.Line($"the application object was not made (0x{e.HResult:X8}) — {e}");
                    Environment.Exit(1);
                }
            });
            HostLog.Line("the application ended");
        }
        catch (Exception e)
        {
            HostLog.Line($"the application did not start — {e}");
            instance?.Dispose();
            return 1;
        }

        instance?.Dispose();
        return App.ExitCode;
    }

    /// <summary>The number in VERSION, which the build stamps (build.ps1 -Version passes it on).</summary>
    static string Version => typeof(Program).Assembly.GetCustomAttributes(typeof(System.Reflection.AssemblyInformationalVersionAttribute), false)
        is [System.Reflection.AssemblyInformationalVersionAttribute v, ..] ? v.InformationalVersion.Split('+')[0] : "unknown";

    static int Fatal(string message, bool quiet)
    {
        Console.Error.WriteLine(message);
        // A dialog on a machine with nobody at it would wait for ever, so pictures never show one.
        if (!quiet) MessageBoxW(0, message, "Hover", 0x10);
        return 1;
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int MessageBoxW(nint window, string text, string caption, uint type);
}
