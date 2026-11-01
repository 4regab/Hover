using System.Diagnostics;
using Hover.Host;
using Hover.Host.Protocol;
using Hover.Notch;
using Microsoft.UI.Dispatching;
using Microsoft.Win32;
using static Hover.Windowing.Native;

namespace Hover.Windowing;

/// <summary>What the notch asks of the rest of the app.</summary>
sealed record NotchCallbacks(Action OpenDashboard, Action Quit, Func<bool> DashboardFocused);

/// <summary>
/// The notch as the user sees it: the window, the pointer and shortcut rules, the tray icon, and
/// what the island and the question's card show from the backend's messages. It ties
/// <see cref="NotchController"/> (the rules) to <see cref="NotchWindow"/> (Windows) and
/// <see cref="NotchRenderer"/> (the pixels). Everything runs on the UI thread.
/// </summary>
sealed class NotchHost : IDisposable
{
    const int ShortcutId = 1;

    /// <summary>The same easing the Go app gives the words that change: fast, then settling.</summary>
    sealed class Tween
    {
        double start = double.MaxValue, delay, duration = 1;
        public double Value(double now) => now < start + delay ? (start == double.MaxValue ? 1 : 0) : 1 - Math.Pow(1 - Math.Clamp((now - start - delay) / duration, 0, 1), 3);
        public bool Active(double now) => start != double.MaxValue && now < start + delay + duration;
        public void Begin(double now, double delayMs, double durationMs) { start = now; delay = delayMs; duration = durationMs; }
        public void Reset() => start = double.MaxValue;
    }

    readonly DispatcherQueue queue;
    readonly BackendSession backend;
    readonly NotchCallbacks callbacks;
    readonly Gfx gfx;
    readonly NotchRenderer renderer;
    readonly NotchWindow window;
    readonly NotchController ctl;
    readonly NotchScene scene = new();
    readonly Stopwatch clock = Stopwatch.StartNew();
    readonly Tween rise = new(), fade = new();
    readonly DispatcherQueueTimer pollTimer, animTimer, secondTimer, glowTimer;
    NotchSurface? surface;
    NotchSettings settings = NotchSettings.Default;
    StateEvent? state;
    Dictionary<string, QuotaValue> quotas = [];
    Island island = new(IslandKind.None, new Seg(), false, [], "");
    Unseen? unseen;
    CardData? card;
    bool cardOpen, renderQueued, animations = true, disposed;
    int speaker, ticks;
    string? lastShortcut;
    NotchState lastState = NotchState.Rest;
    // What the island showed last, to tell which kind of change a new one is.
    (bool Set, IslandKind Kind, string Key, string Words, int Ends) old;

    double Now => clock.Elapsed.TotalMilliseconds;

    /// <summary>The notch window, for the tests and the log.</summary>
    public NotchWindow Window => window;

    public NotchController Controller => ctl;

    NotchHost(DispatcherQueue queue, BackendSession backend, NotchCallbacks callbacks, Gfx gfx, NotchWindow window)
    {
        this.queue = queue;
        this.backend = backend;
        this.callbacks = callbacks;
        this.gfx = gfx;
        this.window = window;
        renderer = new NotchRenderer(gfx);
        ctl = new NotchController(window, () => Now);

        pollTimer = queue.CreateTimer();
        pollTimer.Interval = TimeSpan.FromMilliseconds(Geometry.PollMs);
        pollTimer.Tick += (_, _) => Guard("poll", Poll);
        animTimer = queue.CreateTimer();
        animTimer.Interval = TimeSpan.FromMilliseconds(16);
        animTimer.Tick += (_, _) => Guard("animation", Animate);
        secondTimer = queue.CreateTimer();
        secondTimer.Interval = TimeSpan.FromSeconds(1);
        secondTimer.Tick += (_, _) => Guard("clock", SecondTick);
        glowTimer = queue.CreateTimer();
        glowTimer.IsRepeating = false;
        glowTimer.Interval = TimeSpan.FromSeconds(6);
        glowTimer.Tick += (_, _) => Guard("glow", UpdateRest);
    }

    /// <summary>
    /// Makes the notch, its shortcut and its tray icon. Returns null, with the reason in the log,
    /// when the window or the graphics cannot be made; the app then runs without a notch.
    /// </summary>
    public static NotchHost? TryStart(DispatcherQueue queue, BackendSession backend, NotchCallbacks callbacks)
    {
        Gfx? gfx = null;
        NotchWindow? window = null;
        try
        {
            gfx = Gfx.Create();
            window = new NotchWindow();
            var host = new NotchHost(queue, backend, callbacks, gfx, window);
            host.Start();
            return host;
        }
        catch (Exception e)
        {
            HostLog.Line($"the notch could not start — {e}");
            window?.Dispose();
            gfx?.Dispose();
            return null;
        }
    }

    void Guard(string what, Action action)
    {
        if (disposed) return;
        try { action(); }
        catch (Exception e) { HostLog.Line($"notch {what} failed — {e}"); }
    }

    void Start()
    {
        animations = AnimationsOn();
        ApplyAppearance();
        window.Invalidated += () => Guard("resize", EnsureSurface);
        window.Hotkey += id => Guard("shortcut", () => { if (id == ShortcutId) Toggle(); });
        window.Deactivated += () => Guard("deactivate", Deactivated);
        window.TrayLeftClick += () => Guard("tray", callbacks.OpenDashboard);
        window.TrayPick += i => Guard("tray menu", () => TrayPick(i));
        window.Pressed += (x, y) => Guard("press", ctl.Stay);
        window.Released += (x, y) => Guard("click", () => Clicked(x, y));
        window.KeyDown += vk => Guard("key", () => KeyPressed(vk));
        backend.Event += e => queue.TryEnqueue(() => Guard("message", () => OnBackend(e)));

        ctl.Size = settings.Size;
        ctl.HoverOpens = settings.HoverOpens;
        ctl.Layout();
        RegisterShortcut(settings.Shortcut);
        window.TrayStart();
        window.SetTrayMenu(TrayMenu.Build(settings.Shortcut.Label, LaunchAtLogin.Enabled));
        pollTimer.Start();
        UpdateRest();
        HostLog.Line("notch ready");
    }

    // MARK: Messages from the backend

    void OnBackend(BackendEvent e)
    {
        switch (e)
        {
            case StateEvent s:
                state = s;
                UpdateRest();
                break;
            case QuotasEvent q:
                quotas = q.Values;
                UpdateRest();
                break;
            case SettingsEvent s:
                Apply(NotchSettings.From(s.Model));
                break;
            case EndedEvent ended:
                Ended(ended);
                break;
        }
    }

    void Apply(NotchSettings next)
    {
        var relayout = next.Size != settings.Size;
        var shortcut = next.Shortcut != settings.Shortcut;
        var appearance = next.Appearance != settings.Appearance;
        settings = next;
        HostLog.Line($"notch settings: hover opens {next.HoverOpens}, {next.Items.Count} quotas, size {next.Size}, shortcut {next.Shortcut.Label}");
        ctl.HoverOpens = next.HoverOpens;
        ctl.Size = next.Size;
        if (appearance) ApplyAppearance();
        if (shortcut)
        {
            RegisterShortcut(next.Shortcut);
            window.SetTrayMenu(TrayMenu.Build(next.Shortcut.Label, LaunchAtLogin.Enabled));
        }
        if (relayout) ctl.Layout();
        UpdateRest();
    }

    /// <summary>A task ended. Where nobody is looking, the island keeps it until it is seen and the tray says so.</summary>
    void Ended(EndedEvent e)
    {
        if (Watching) return;
        var result = e.Ok ? EndState.Completed : e.Text.Equals("Failed", StringComparison.OrdinalIgnoreCase) ? EndState.Failed : EndState.Stopped;
        unseen = new Unseen((unseen?.Count ?? 0) + 1, e.Tool, result, e.Task, e.Took);
        window.TrayNotify(e.Title, e.Body.Length > 0 ? e.Body : e.Text);
        UpdateRest();
    }

    /// <summary>The open notch, or an app window with the keyboard: someone is looking, so ends count as seen.</summary>
    bool Watching => ctl.State != NotchState.Rest || callbacks.DashboardFocused();

    /// <summary>The ends announced so far have been seen.</summary>
    public void Seen()
    {
        if (unseen is null) return;
        unseen = null;
        UpdateRest();
    }

    // MARK: Appearance

    static bool AnimationsOn()
    {
        var on = 1;
        return !SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, ref on, 0) || on != 0;
    }

    void ApplyAppearance()
    {
        var dark = settings.Appearance switch
        {
            "Dark" => true,
            "Light" => false,
            // The system's choice for apps: 1 is light; no value is read as dark.
            _ => Registry.GetValue(@"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "AppsUseLightTheme", null) is not int light || light == 0,
        };
        scene.Dark = dark;
        scene.Panel = dark ? 0x000000u : 0xf2f2f7u;
    }

    // MARK: The shortcut

    void RegisterShortcut(Shortcut sc)
    {
        window.UnregisterHotkey(ShortcutId);
        if (!sc.IsSet) return;
        var label = sc.Label;
        if (sc.VirtualKey is not { } vk)
        {
            HostLog.Line($"hotkey {label} has no Windows virtual-key mapping");
            return;
        }
        uint mods = 0;
        if ((sc.Modifiers & Shortcut.Control) != 0) mods |= MOD_CONTROL;
        if ((sc.Modifiers & Shortcut.Alt) != 0) mods |= MOD_ALT;
        if ((sc.Modifiers & Shortcut.Shift) != 0) mods |= MOD_SHIFT;
        if ((sc.Modifiers & Shortcut.Windows) != 0) mods |= MOD_WIN;
        var error = window.RegisterHotkey(ShortcutId, vk, mods);
        if (error == 0)
        {
            lastShortcut = label;
            HostLog.Line($"hotkey {label} registered");
            return;
        }
        HostLog.Line($"hotkey {label} could not be registered (Win32 error {error})");
        // Said once for each shortcut, as a balloon from the tray icon.
        if (lastShortcut != "!" + label)
        {
            lastShortcut = "!" + label;
            window.TrayNotify("Global shortcut unavailable",
                $"Hover couldn't register the notch shortcut, {label}. Windows has reserved it or another app is already using it. Choose a different shortcut in Settings → General.");
        }
    }

    // MARK: Opening and closing

    public void Toggle()
    {
        ctl.Toggle();
        AfterChange(peek: false, focus: true);
    }

    /// <summary>Called after anything that may have opened or folded the notch.</summary>
    void AfterChange(bool peek, bool focus)
    {
        var now = ctl.State;
        if (now != lastState)
        {
            if (lastState == NotchState.Rest)
            {
                HostLog.Line($"notch: opening (peek {peek.ToString().ToLowerInvariant()}, focus {focus.ToString().ToLowerInvariant()})");
                // The office shows the question itself; ends are seen.
                if (cardOpen) { cardOpen = false; card = null; }
                scene.Glow = 0;
                unseen = null;
            }
            else if (now == NotchState.Rest)
            {
                HostLog.Line("notch: folding");
            }
            lastState = now;
            UpdateRest();
        }
        EnsureAnimating();
        QueueRender();
    }

    void Deactivated()
    {
        if (ctl.State != NotchState.Rest && !window.ForegroundIsOurs())
        {
            ctl.Collapse();
            AfterChange(false, false);
        }
    }

    void KeyPressed(int vk)
    {
        if (cardOpen)
        {
            if (vk == VK_ESCAPE) AnswerAsked("deny");
            else if (vk == VK_RETURN) AnswerAsked((GetKeyState(VK_SHIFT) & 0x8000) != 0 ? "trust" : "allow");
        }
        else if (vk == VK_ESCAPE && ctl.State != NotchState.Rest)
        {
            ctl.Collapse();
            AfterChange(false, false);
        }
    }

    // MARK: The pointer

    void Poll()
    {
        var before = ctl.State;
        var peeked = false;
        var action = ctl.Tick(out var relayout);
        if (action == NotchAction.Peek) peeked = true;
        if (relayout)
        {
            ctl.Layout();
            UpdateRest();
        }
        if (ctl.State != before) AfterChange(peeked, false);
        // The window takes the pointer only over the shape; the tick above keeps that up to date.
        ctl.Shape();
        UpdateHover();
        EnsureAnimating();
    }

    void UpdateHover()
    {
        string? hovered = null;
        if (ctl.Over)
        {
            var (x, y) = window.Cursor();
            double dx = (x - ctl.Window.Left) / ctl.Scale, dy = (y - ctl.Window.Top) / ctl.Scale;
            hovered = renderer.Targets.LastOrDefault(t => t.Action != "shape" && t.Contains(dx, dy))?.Action;
        }
        if (hovered == scene.Hover) return;
        scene.Hover = hovered;
        QueueRender();
    }

    void Clicked(int px, int py)
    {
        double x = px / ctl.Scale, y = py / ctl.Scale;
        var target = renderer.Targets.LastOrDefault(t => t.Contains(x, y));
        if (target is null) return;
        switch (target.Action)
        {
            case "shape":
                // A click on the island, outside its buttons, opens the office; on the card it does nothing.
                if (ctl.State == NotchState.Rest && ctl.RestKind == RestKind.Pill)
                {
                    ctl.Expand(false, false);
                    AfterChange(false, false);
                }
                break;
            case "deny": AnswerAsked("deny"); break;
            case "review": OpenCard(); break;
            case "cardAllow": AnswerAsked("allow"); break;
            case "cardTrust": AnswerAsked("trust"); break;
            case "cardDeny": AnswerAsked("deny"); break;
        }
    }

    // MARK: The question's card

    void OpenCard()
    {
        if (ctl.State != NotchState.Rest) return;
        var seg = island.Seg;
        if (seg.Kind != SegKind.Ask) return;
        // A question's choices are in the office, in its chat: Review opens it there.
        if (seg.Ask!.Kind == "question")
        {
            ctl.Expand(false, true);
            AfterChange(false, true);
            return;
        }
        window.RememberForeground();
        window.SetAcceptsKeys(true);
        window.Focus();
        cardOpen = true;
        UpdateRest();
    }

    void CloseCard(bool giveBack)
    {
        if (!cardOpen) return;
        cardOpen = false;
        card = null;
        if (giveBack) window.RestoreForeground();
        window.SetAcceptsKeys(false);
        UpdateRest();
    }

    /// <summary>Answers the question in front: allow, trust or deny.</summary>
    void AnswerAsked(string answer)
    {
        var seg = island.Seg;
        if (seg.Kind != SegKind.Ask || seg.Ask is null) return;
        backend.Send(new AnswerCommand(seg.Session, seg.Ask.Id, answer));
        HostLog.Line($"notch: answered {answer}");
        CloseCard(true);
    }

    // MARK: The resting shape

    void UpdateRest()
    {
        var sessions = state?.Sessions ?? [];
        var nowMs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        var next = Rest.Make(settings.Items.Contains, id => quotas.GetValueOrDefault(id), sessions, unseen, speaker, nowMs, cardOpen,
            id => state?.Tools.FirstOrDefault(t => t.Id == id)?.Name is { Length: > 0 } name ? name : id);
        // No question left: the card goes, and the keyboard goes back.
        if (cardOpen && next.Seg.Kind != SegKind.Ask)
        {
            CloseCard(true);
            return;
        }
        island = next;
        var seg = next.Seg;
        var kind = next.Kind;
        var atRest = ctl.State == NotchState.Rest;
        var words = seg.Kind switch
        {
            SegKind.Work => $"{seg.Name}:{seg.Verb}:{seg.Obj}",
            SegKind.Done => $"done:{seg.Count}:{seg.Title}",
            SegKind.Ask => $"ask:{seg.Ask?.Id}:{seg.Total}",
            _ => "",
        };
        var ends = seg.Kind == SegKind.Done ? seg.Count : 0;
        // The kind or the items changed: the content cross-fades (held 90 ms, in over 230). Only
        // the words changed: they rise into place over 320.
        if (atRest && animations && old.Set && (old.Kind != kind || (kind == IslandKind.Pill && old.Key != next.Key))) fade.Begin(Now, 90, 230);
        else if (old.Set && animations && old.Words != words && old.Kind == kind && old.Key == next.Key) rise.Begin(Now, 1, 320);

        uint glow = 0;
        card = null;
        if (seg.Kind == SegKind.Ask)
        {
            glow = 0xffb340;
            card = CardOf(next);
        }
        else if (seg.Kind == SegKind.Done)
        {
            // A new end glows for 6 s: green done, red failed. A stop does not.
            if (ends > old.Ends)
            {
                glowTimer.Stop();
                glowTimer.Start();
            }
            if (glowTimer.IsRunning) glow = seg.State switch { EndState.Completed => 0x32d74bu, EndState.Failed => 0xff453au, _ => 0u };
        }
        if (atRest) scene.Glow = glow;
        old = (true, kind, next.Key, words, ends);
        // A question waits: hovering does not open the office.
        ctl.Asking = seg.Kind == SegKind.Ask;

        // The one-second clock runs while someone works or asks; the speaker moves on every 3 s.
        var busy = seg.Kind is SegKind.Work or SegKind.Ask;
        if (busy && !secondTimer.IsRunning) secondTimer.Start();
        else if (!busy)
        {
            secondTimer.Stop();
            ticks = 0;
        }

        scene.Island = next;
        scene.Card = card;
        scene.Kind = (RestKind)(int)kind;
        ctl.RestKind = (RestKind)(int)kind;
        ctl.Still = !animations;
        var rest = MeasureRest(kind);
        if (ctl.RestTarget != rest) ctl.SetRest(rest);
        EnsureAnimating();
        QueueRender();
    }

    Size MeasureRest(IslandKind kind)
    {
        FillScene(measureOnly: true);
        switch (kind)
        {
            case IslandKind.Pill:
                // The island's items start 11 in, and its padding after the last is added.
                return Geometry.RestSize(RestKind.Pill, renderer.PillWidth(scene, gfx.Context), 0);
            case IslandKind.Card when scene.Card is not null:
                var (w, h) = renderer.CardSize(scene, gfx.Context);
                return Geometry.RestSize(RestKind.Card, w, h);
        }
        return Geometry.RestSize(RestKind.None, 0, 0);
    }

    CardData? CardOf(Island isl)
    {
        var seg = isl.Seg;
        var ask = seg.Ask;
        if (ask is null) return null;
        var session = state?.Sessions.FirstOrDefault(s => s.Id == seg.Session);
        var folder = session is null ? "" : LastPart(session.Folder);
        var lines = new List<PreviewLine>();
        if (ask.Preview is not null)
        {
            foreach (var line in ask.Preview.Split('\n'))
                lines.Add(new PreviewLine(line, line.StartsWith('+') ? 1 : line.StartsWith('-') ? -1 : 0));
        }
        var path = ask.Path ?? (ask.Preview is null ? ask.Title : "");
        return new CardData(seg.Tool, ask.Title, $"{folder} · {session?.Title}", seg.Total > 1 ? $"1 of {seg.Total}" : "",
            ask.Command ?? "", path, lines, ask.Reason, ask.Danger, ask.Allow);
    }

    static string LastPart(string folder)
    {
        var trimmed = folder.TrimEnd('\\', '/');
        var cut = trimmed.LastIndexOfAny(['\\', '/']);
        return cut >= 0 ? trimmed[(cut + 1)..] : trimmed;
    }

    void SecondTick()
    {
        ticks++;
        if (ticks % 3 == 0) speaker++;
        UpdateRest();
    }

    // MARK: Drawing

    void FillScene(bool measureOnly = false)
    {
        var shape = ctl.Shape();
        scene.ApplyShape(shape, ctl.OpenSize);
        scene.T = clock.Elapsed.TotalSeconds;
        scene.Rise = rise.Value(Now);
        scene.Fade = fade.Value(Now);
        if (!measureOnly && ctl.State != NotchState.Rest) scene.Glow = 0;
    }

    void EnsureSurface()
    {
        var (w, h) = window.ClientSize;
        if (surface is null)
        {
            surface = new NotchSurface(gfx, window.Handle, w, h);
            HostLog.Line($"notch surface made ({w} x {h})");
        }
        else surface.Resize(w, h);
        Render();
    }

    void QueueRender()
    {
        if (renderQueued || disposed) return;
        renderQueued = true;
        queue.TryEnqueue(() =>
        {
            renderQueued = false;
            Guard("draw", Render);
        });
    }

    void Render()
    {
        if (surface is null || disposed) return;
        FillScene();
        surface.Draw(dc => renderer.Draw(dc, scene, ctl.WindowWidth), ctl.Scale);
    }

    // MARK: Animation

    bool Moving => ctl.Animating || fade.Active(Now) || rise.Active(Now) || (animations && island.Seg.Kind is SegKind.Work or SegKind.Ask && ctl.State == NotchState.Rest);

    void EnsureAnimating()
    {
        if (Moving) { if (!animTimer.IsRunning) animTimer.Start(); }
    }

    void Animate()
    {
        Render();
        if (!Moving) animTimer.Stop();
    }

    // MARK: The tray

    void TrayPick(int index)
    {
        switch (index)
        {
            case 0: Toggle(); break;
            case 1: callbacks.OpenDashboard(); break;
            case 3:
                LaunchAtLogin.Set(!LaunchAtLogin.Enabled);
                window.SetTrayMenu(TrayMenu.Build(settings.Shortcut.Label, LaunchAtLogin.Enabled));
                break;
            case 5: callbacks.OpenDashboard(); break;
            case 6: callbacks.Quit(); break;
        }
    }

    public void Dispose()
    {
        if (disposed) return;
        disposed = true;
        pollTimer.Stop();
        animTimer.Stop();
        secondTimer.Stop();
        glowTimer.Stop();
        window.Dispose();
        surface?.Dispose();
        renderer.Dispose();
        gfx.Dispose();
    }
}
