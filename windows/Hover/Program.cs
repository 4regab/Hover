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
                SynchronizationContext.SetSynchronizationContext(
                    new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread()));
                _ = new App(launch, dataDir);
            });
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
