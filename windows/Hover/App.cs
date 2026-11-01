using System.Runtime.InteropServices;
using Hover.Host;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using WinRT.Interop;

namespace Hover;

/// <summary>
/// The application object. It owns the backend and, for now, the dashboard window; the
/// notch, the tray and the office join it in later phases.
/// </summary>
public sealed class App : Application
{
    // A second launch can signal before the application exists; it is remembered until it does.
    static readonly object Gate = new();
    static App? current;
    static bool showPending;

    public static int ExitCode { get; private set; }

    readonly LaunchOptions launch;
    readonly string dataDir;
    readonly BackendSession backend = new();
    DispatcherQueue? queue;
    DashboardWindow? dashboard;
    bool quitting;

    public App(LaunchOptions launch, string dataDir)
    {
        this.launch = launch;
        this.dataDir = dataDir;
        // The controls' own styles; with no App.xaml they are merged here.
        HostLog.Line("merging the controls' styles");
        Resources.MergedDictionaries.Add(new XamlControlsResources());
        HostLog.Line("the application object is made");
        UnhandledException += (_, e) =>
        {
            HostLog.Line($"unhandled — {e.Exception}");
            e.Handled = true;
        };
    }

    /// <summary>A later launch asked for the dashboard. Callable from any thread.</summary>
    public static void RequestDashboard()
    {
        HostLog.Line("a second launch asked for the dashboard");
        App? app;
        lock (Gate)
        {
            app = current;
            if (app?.queue is null)
            {
                showPending = true;
                return;
            }
        }
        app.queue.TryEnqueue(app.OpenDashboard);
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        bool show;
        lock (Gate)
        {
            queue = DispatcherQueue.GetForCurrentThread();
            current = this;
            show = showPending;
            showPending = false;
        }
        _ = RunAsync(show);
    }

    async Task RunAsync(bool showAtOnce)
    {
        try
        {
            HostLog.Line("started");
            // Not for pictures: they leave the machine as they found it.
            if (launch.ShotsDirectory is null) _ = Task.Run(() => OldService.RemoveOnceAsync(dataDir));
            if (!launch.Background || showAtOnce || launch.ShotsDirectory is not null) OpenDashboard();
            await backend.StartAsync(dataDir);

            if (launch.ShotsDirectory is { } directory)
            {
                await Shots.TakeAsync(directory, dashboard!);
                await QuitAsync();
            }
        }
        catch (Exception e)
        {
            HostLog.Line($"failed — {e}");
            ExitCode = 1;
            await QuitAsync();
        }
    }

    void OpenDashboard()
    {
        if (quitting) return;
        HostLog.Line("opening the dashboard");
        if (dashboard is null)
        {
            dashboard = new DashboardWindow(dataDir, backend);
            // Until the tray and the notch exist there is nothing to leave running, so closing
            // the dashboard quits.
            dashboard.Closed += (_, _) =>
            {
                dashboard = null;
                _ = QuitAsync();
            };
        }
        dashboard.AppWindow.Show();
        dashboard.Activate();
        // The second launch passed on the right to take focus, so this reaches the front.
        SetForegroundWindow(WindowNative.GetWindowHandle(dashboard));
    }

    async Task QuitAsync()
    {
        if (quitting) return;
        quitting = true;
        await backend.DisposeAsync();
        Exit();
    }

    [DllImport("user32.dll")]
    static extern bool SetForegroundWindow(nint window);
}
