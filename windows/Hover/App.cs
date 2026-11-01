using System.Runtime.InteropServices;
using Hover.Chatting;
using Hover.Configuring;
using Hover.Settings;
using Hover.Sharing;
using Hover.Host;
using Hover.Notch;
using Hover.Speaking;
using Hover.Windowing;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Markup;
using Microsoft.UI.Xaml.XamlTypeInfo;
using WinRT.Interop;

namespace Hover;

/// <summary>
/// The application object. It owns the backend and, for now, the dashboard window; the
/// notch, the tray and the office join it in later phases.
///
/// It answers the XAML engine's questions about types with the controls library's own
/// metadata, as the XAML compiler's generated code would for an App.xaml.
/// </summary>
public sealed class App : Application, IXamlMetadataProvider
{
    readonly XamlControlsXamlMetaDataProvider controls = new();

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
    NotchHost? notch;
    OfficeHost? office;
    BeatsHost? beats;
    Gfx? gfx;
    UiCore? panels;
    SettingsHost? configuring;
    DiscordHost? discord;
    bool quitting;

    public App(LaunchOptions launch, string dataDir)
    {
        this.launch = launch;
        this.dataDir = dataDir;
        UnhandledException += (_, e) =>
        {
            HostLog.Line($"unhandled — {e.Exception}");
            e.Handled = true;
        };
    }

    public IXamlType GetXamlType(Type type) => controls.GetXamlType(type);

    public IXamlType GetXamlType(string fullName) => controls.GetXamlType(fullName);

    public XmlnsDefinition[] GetXmlnsDefinitions() => controls.GetXmlnsDefinitions();

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
        // The controls' own styles; with no App.xaml they are merged here. Application.Resources
        // cannot be read until the application has started, so not in the constructor.
        try
        {
            Resources.MergedDictionaries.Add(new XamlControlsResources());
        }
        catch (Exception e)
        {
            // The window still opens, with the engine's plain styles.
            HostLog.Line($"the controls' styles did not load (0x{e.HResult:X8}) — {e}");
        }

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
            // The 3D office, shown in the open notch or in the dashboard.
            office = new OfficeHost(queue!, backend);
            // The office beats: played while the switch is on and an office (the open notch, or a shown window) is in view.
            // Pictures leave the machine as they found it, so they make no sound.
            if (launch.ShotsDirectory is null)
                beats = new BeatsHost(queue!, office, () => notch?.Controller.State != NotchState.Rest || dashboard?.AppWindow.IsVisible == true);
            // The panels share one graphics device with the notch.
            try
            {
                gfx = Gfx.Create();
                panels = new UiCore(gfx, queue!, backend, dataDir, office, inNotch: launch.ShotsDirectory is null);
            }
            catch (Exception e)
            {
                HostLog.Line($"the panels could not start — {e}");
                panels?.Dispose();
                panels = null;
                gfx?.Dispose();
                gfx = null;
            }
            // Settings is drawn with the panels' canvas: with no graphics device there is none.
            if (panels is not null)
            {
                configuring = new SettingsHost(queue!, backend, panels, () => notch?.VoiceFlow) { Quit = () => _ = QuitAsync() };
            }
            // Pictures run without the notch, the tray or the shortcut: they leave the machine as they found it.
            if (launch.ShotsDirectory is null)
            {
                notch = NotchHost.TryStart(queue!, backend, new NotchCallbacks(OpenDashboard, () => _ = QuitAsync(), () => dashboard?.IsActive == true, OpenSettings), office, gfx, panels);
                if (notch is not null && configuring is not null) notch.AttachSettings(configuring);
                // The office's top menu has a Settings row: it opens where the office is (the open notch, else the window).
                if (panels is not null) panels.SettingsRequested += () =>
                {
                    if (notch is { } n && n.Controller.State != NotchState.Rest) configuring?.ShowInNotch(Section.General);
                    else OpenSettings();
                };
                // Hover on the Discord status, when the switch in Settings → Integrations is on.
                discord = new DiscordHost(backend);
            }
            // With no notch there would be nothing to see, so the window opens.
            if (launch.Settings) OpenSettings();
            else if (launch.Dashboard || showAtOnce || launch.ShotsDirectory is not null || (notch is null && !launch.Background)) OpenDashboard();
            await backend.StartAsync(dataDir);

            if (launch.ShotsDirectory is { } directory)
            {
                await Shots.TakeAsync(directory, dashboard!, panels);
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

    /// <summary>Settings in the app window (the tray menu's Settings…).</summary>
    void OpenSettings()
    {
        OpenDashboard();
        configuring?.ShowInDashboard(Section.General);
    }

    void OpenDashboard()
    {
        if (quitting) return;
        HostLog.Line("opening the dashboard");
        if (dashboard is null)
        {
            dashboard = new DashboardWindow(dataDir, backend, office, panels, configuring);
            // Closing the window hides it; the app keeps running in the tray. Quit is in the tray
            // menu. Without a notch there is no tray to bring it back from, so closing quits.
            dashboard.AppWindow.Closing += (_, closing) =>
            {
                if (quitting || notch is null) return;
                closing.Cancel = true;
                dashboard.AppWindow.Hide();
                HostLog.Line("the dashboard was hidden to the tray");
            };
            dashboard.Activated += (_, activation) =>
            {
                if (activation.WindowActivationState != WindowActivationState.Deactivated) notch?.Seen();
            };
            dashboard.Closed += (_, _) =>
            {
                dashboard = null;
                if (!quitting) _ = QuitAsync();
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
        HostLog.Line("quitting");
        // The tray icon goes first, so it does not linger after the process; then the backend
        // writes the history and the settings and stops.
        beats?.Dispose();
        beats = null;
        discord?.Dispose();
        discord = null;
        configuring?.Dispose();
        configuring = null;
        notch?.Dispose();
        notch = null;
        panels?.Dispose();
        panels = null;
        gfx?.Dispose();
        gfx = null;
        office?.Dispose();
        office = null;
        await backend.DisposeAsync();
        Exit();
    }

    [DllImport("user32.dll")]
    static extern bool SetForegroundWindow(nint window);
}
