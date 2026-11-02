using System.Diagnostics;
using Hover.Chat.Ui;
using Hover.Chatting;
using Hover.Host;
using Hover.Host.Protocol;
using Hover.Settings;
using Hover.Speaking;
using Hover.Voice;
using Hover.Windowing;
using Microsoft.UI.Dispatching;
using Vortice.Direct2D1;

namespace Hover.Configuring;

/// <summary>
/// Settings in the app: one controller following the backend, drawn by one <see cref="SettingsUi"/> in the open notch and another
/// in the app window (only one shows at a time for the user, but both can be open). It is the app's side of
/// <c>app.Host</c>: the pickers, Launch at login, the system's light or dark, the voice flow's Try it and its shortcut, and the theme
/// the app window wears. Everything runs on the UI thread.
/// </summary>
sealed class SettingsPane : IDisposable
{
    readonly DispatcherQueue queue;
    readonly BackendSession backend;
    readonly UiCore core;
    readonly Func<VoiceHost?> voice;
    readonly DispatcherQueueTimer themeTimer;
    readonly Action<BackendEvent> onEvent;
    readonly Env env;
    bool disposed, started, lastDark, voiceHooked;
    int themeLooks;

    public SettingsController Controller { get; }

    /// <summary>The graphics device the panels share.</summary>
    public Gfx Gfx => core.Gfx;

    /// <summary>Settings as the open notch draws it (rounded, with a close button).</summary>
    public SettingsUi NotchUi { get; }

    /// <summary>Settings as the app window draws it.</summary>
    public SettingsUi DashUi { get; }

    public bool NotchOpen { get; private set; }
    public bool DashOpen { get; private set; }

    /// <summary>Something to draw again, in either view.</summary>
    public event Action? Changed;

    /// <summary>Settings opened or closed in the notch; the notch lays itself out again.</summary>
    public event Action? NotchOpenChanged;

    public event Action? DashOpenChanged;

    /// <summary>Windows switched between light and dark.</summary>
    public event Action? SystemThemeChanged;

    /// <summary>Settings was asked for in the app window (from a menu, say), whether or not it was open: the page takes the keyboard.</summary>
    public event Action? DashShown;

    /// <summary>The notch is to fold (its close button, Esc).</summary>
    public event Action? FoldRequested;

    /// <summary>The palette Settings and the app window wear changed.</summary>
    public event Action<AppPalette>? PaletteChanged;

    /// <summary>Launch at login was switched from Settings (the tray menu shows the tick).</summary>
    public event Action? LoginChanged;

    /// <summary>The user chose Quit on the page.</summary>
    public Action Quit { get; set; } = () => { };

    public SettingsPane(DispatcherQueue queue, BackendSession backend, UiCore core, Func<VoiceHost?> voice)
    {
        this.queue = queue;
        this.backend = backend;
        this.core = core;
        this.voice = voice;
        env = new Env(this);
        Controller = new SettingsController(env);
        var clipboard = new Win32Clipboard();
        NotchUi = new SettingsUi(core.Shaper, Controller, new SettingsUiEnv { Back = CloseNotch, Fold = () => { CloseNotch(); FoldRequested?.Invoke(); }, Clipboard = clipboard, InNotch = true });
        DashUi = new SettingsUi(core.Shaper, Controller, new SettingsUiEnv { Back = CloseDash, Clipboard = clipboard, InNotch = false });
        Controller.Changed += () => Changed?.Invoke();
        Controller.PaletteChanged += p => PaletteChanged?.Invoke(p);
        onEvent = e => queue.TryEnqueue(() =>
        {
            if (disposed) return;
            try { Controller.Accept(e); }
            catch (Exception ex) { HostLog.Line($"settings: a message failed — {ex}"); }
        });
        backend.Event += onEvent;
        backend.Changed += () => queue.TryEnqueue(BeginWhenReady);
        // Windows' own light or dark is watched: the appearance "System" follows it.
        lastDark = SystemTheme.IsDark();
        themeTimer = queue.CreateTimer();
        themeTimer.Interval = TimeSpan.FromSeconds(1);
        themeTimer.Tick += (_, _) =>
        {
            var dark = SystemTheme.IsDark();
            // For the checks of the real app: what each look at Windows' setting found (a line only when it changed, and the first few).
            if (UiDump.On && (dark != lastDark || themeLooks++ < 3)) HostLog.Line($"windows setting looked at: dark {dark} (was {lastDark})");
            if (dark == lastDark) return;
            lastDark = dark;
            HostLog.Line($"windows is now {(dark ? "dark" : "light")}");
            Controller.Apply();
            Changed?.Invoke();
            SystemThemeChanged?.Invoke();
        };
        themeTimer.Start();
        BeginWhenReady();
    }

    void BeginWhenReady()
    {
        if (disposed || started || !backend.Ready) return;
        started = true;
        Controller.Start();
    }

    // MARK: Opening and closing

    /// <summary>Opens Settings in the notch (the notch is opened by whoever asks).</summary>
    public void ShowInNotch(Section section)
    {
        Controller.Open(section);
        HookVoice();
        if (!NotchOpen)
        {
            NotchOpen = true;
            NotchOpenChanged?.Invoke();
        }
        Changed?.Invoke();
    }

    public void ShowInDashboard(Section section)
    {
        Controller.Open(section);
        HookVoice();
        if (!DashOpen)
        {
            DashOpen = true;
            DashOpenChanged?.Invoke();
        }
        Changed?.Invoke();
        DashShown?.Invoke();
    }

    public void CloseNotch()
    {
        if (!NotchOpen) return;
        NotchOpen = false;
        Controller.StopRecording();
        NotchOpenChanged?.Invoke();
        Changed?.Invoke();
    }

    public void CloseDash()
    {
        if (!DashOpen) return;
        DashOpen = false;
        Controller.StopRecording();
        DashOpenChanged?.Invoke();
        Changed?.Invoke();
    }

    /// <summary>Opens the page of a tool or a section by name from the app window's menu.</summary>
    public void ShowInDashboard(int sectionIndex) => ShowInDashboard(Sections.All[Math.Clamp(sectionIndex, 0, Sections.All.Length - 1)]);

    // MARK: Voice

    /// <summary>Ties Try it, the shortcut's error and the microphone list to the voice flow, once it exists.</summary>
    void HookVoice()
    {
        if (voiceHooked || voice() is not { } v) return;
        voiceHooked = true;
        v.Service.Changed += st => queue.TryEnqueue(() =>
        {
            if (disposed) return;
            // Try it's result waits in Settings until the next press; any other interaction is the card's, not the page's.
            if (v.Service.Trial) Controller.SetTry(VoicePageCards.Try(st, Controller.Data));
            else if (st.Fresh) Controller.SetTry(null);
        });
        v.Changed += () => Controller.SetShortcutError(string.IsNullOrEmpty(v.Card.HoldError) ? null : v.Card.HoldError);
        Controller.SetShortcutError(string.IsNullOrEmpty(v.Card.HoldError) ? null : v.Card.HoldError);
    }

    // MARK: Drawing

    /// <summary>Draws a frame of the view on the context, which must be between BeginDraw and EndDraw. Sizes are in device-independent pixels.</summary>
    public void Paint(SettingsUi ui, ID2D1DeviceContext dc, float width, float height)
    {
        core.Canvas.Begin(dc);
        ui.Paint(core.Canvas, width, height, core.NowMs);
    }

    // MARK: The app's side of Settings

    sealed class Env(SettingsPane host) : ISettingsEnv
    {
        readonly Hover.Settings.RunValue run = new();

        public void Send(BackendCommand command) => host.backend.Send(command);

        public bool LaunchAtLogin() => run.Enabled;

        public void SetLaunchAtLogin(bool on)
        {
            if (!run.Set(on)) HostLog.Line("launch at login: the Run value could not be written");
            host.LoginChanged?.Invoke();
        }

        public bool SystemDark() => SystemTheme.IsDark();

        public void Quit() => host.Quit();

        public void ChooseFolder(Action<string?> done) => done(Dialogs.Folder("Choose a folder"));

        public void ChooseThemeFile(Action<string?> done) => done(Dialogs.ThemeFile());

        public void OpenInEditor(string path)
        {
            // The file opens in the user's editor for it; with none for .json, in whatever opens it.
            foreach (var verb in new[] { "edit", "open" })
            {
                try
                {
                    Process.Start(new ProcessStartInfo(path) { UseShellExecute = true, Verb = verb });
                    return;
                }
                catch (Exception e) when (e is System.ComponentModel.Win32Exception or InvalidOperationException)
                {
                    HostLog.Line($"could not {verb} {path}: {e.Message}");
                }
            }
        }

        public IReadOnlyList<string> Microphones() => MicDevices.Names();

        public void TrialPress() => host.voice()?.Service.Press(trial: true);

        public void TrialRelease() => host.voice()?.Service.Released();
    }

    public void Dispose()
    {
        if (disposed) return;
        disposed = true;
        themeTimer.Stop();
        backend.Event -= onEvent;
    }
}
