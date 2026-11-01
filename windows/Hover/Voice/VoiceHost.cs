using Hover.Host;
using Hover.Host.Protocol;
using Hover.Notch;
using Hover.Voice;
using Microsoft.UI.Dispatching;

namespace Hover.Speaking;

/// <summary>
/// Voice in the app: the service that presses and releases, the card's controller, the shortcut
/// and what the settings say about it. It runs the parts of <c>internal/shell/voice.go</c> that
/// are not drawing; the notch draws <see cref="View"/> and asks <see cref="Card"/> for what a
/// button does. Everything it raises comes on the UI thread.
/// </summary>
sealed class VoiceHost : IDisposable
{
    readonly DispatcherQueue queue;
    readonly BackendSession backend;
    VoiceHotkey? key;
    bool disposed;

    public VoiceService Service { get; }
    public VoiceCardController Card { get; }

    /// <summary>The card's keyboard and mouse state, and the aura between frames.</summary>
    public CardUi Ui { get; } = new();

    readonly Aura aura = new();
    AuraMode? auraMode;

    /// <summary>The card (or the shortcut's error) changed and the notch must be redrawn.</summary>
    public event Action? Changed;

    /// <summary>A new interaction began that the notch must make room for: it folds and takes the keyboard.</summary>
    public event Action? Began;

    /// <summary>The keyboard can go back to what had it: the card has nothing more to type into.</summary>
    public event Action? Done;

    /// <summary>The card asked for Settings → Voice.</summary>
    public event Action? SettingsRequested;

    /// <summary>Dictation: the words (and the screenshots) for the open chat's reply box.</summary>
    public event Action<VoiceState>? Dictated;

    /// <summary>Dictation could not write the words down: why.</summary>
    public event Action<string>? DictationFailed;

    /// <summary>Whether a chat with its reply box open is in view: then the words go there and no card shows.</summary>
    public Func<bool> DictationHere { get; set; } = () => false;

    /// <summary>The voice project open in the office (its id), or null.</summary>
    public Func<string?> ActiveProject { get; set; } = () => null;

    public VoiceHost(DispatcherQueue queue, BackendSession backend)
    {
        this.queue = queue;
        this.backend = backend;
        Service = new VoiceService(backend.Send);
        Card = new VoiceCardController(Service, () => Chime.Play());
        backend.Event += OnBackend;
        Service.SettingsChanged += s => queue.TryEnqueue(() => Register(s));
        Service.Dictated += v => queue.TryEnqueue(() => Dictated?.Invoke(v));
        Service.DictationFailed += why => queue.TryEnqueue(() => DictationFailed?.Invoke(why));
        Card.Changed += () => queue.TryEnqueue(() => { if (!disposed) Changed?.Invoke(); });
        Card.SettingsRequested += () => queue.TryEnqueue(() => SettingsRequested?.Invoke());
        Service.Changed += st => queue.TryEnqueue(() => Moved(st));
    }

    StateEvent? lastState;
    ProjectsEvent? lastProjects;

    /// <summary>The voice project (its id) whose folder is the chat open in the office, or null: it wins a tie when the words name two.</summary>
    public string? ProjectOf(int? session)
    {
        if (session is not { } id || lastState?.Sessions.FirstOrDefault(s => s.Id == id) is not { } chat) return null;
        foreach (var p in lastProjects?.Projects ?? [])
        {
            var enabled = p.Extra is not null && p.Extra.TryGetValue("Voice", out var v) && v.ValueKind == System.Text.Json.JsonValueKind.True;
            if (enabled && Folders.Same(p.Folder, chat.Folder)) return p.Id;
        }
        return null;
    }

    void OnBackend(BackendEvent e)
    {
        if (disposed) return;
        if (e is StateEvent st) lastState = st;
        else if (e is ProjectsEvent { Ok: true } pr) lastProjects = pr;
        // The service reads the voice messages and the settings; the card reads the tools, projects and repositories.
        Service.OnBackend(e);
        Card.OnBackend(e);
    }

    /// <summary>The card as it should be drawn now, or null when nothing shows.</summary>
    public VoiceView? View => disposed ? null : Card.View;

    public bool Showing => View is not null;

    /// <summary>The aura is moving: the listening and working cards redraw every frame.</summary>
    public bool Animating => View?.Card.Kind is 1 or 2;

    /// <summary>Makes the aura's frame for the card now showing (the notch's clock in seconds).</summary>
    public void Frame(double t, bool animations)
    {
        Ui.Now = t;
        var view = View;
        Ui.UpdateFlash(view?.Flash == true, t);
        if (view?.Card.Kind is not (1 or 2))
        {
            if (auraMode is not null) aura.Reset();
            auraMode = null;
            Ui.AuraPixels = null;
            return;
        }
        var mode = view.Card.Kind == 1 ? AuraMode.Listening : AuraMode.Working;
        auraMode = mode;
        var (r, g, b) = new VoiceSettings { AuraColor = view.AuraColor }.AuraRgb();
        // A pixel for each device pixel of its 66 DIPs.
        var px = (int)(66 * scale + 0.5);
        Ui.AuraPixels = aura.Frame(mode, (float)t, view.Level, px, !animations, (r, g, b));
        Ui.AuraPx = px;
    }

    double scale = 1;

    /// <summary>The notch's scale (device pixels to DIPs), for the aura's size.</summary>
    public void SetScale(double value) => scale = value <= 0 ? 1 : value;

    /// <summary>A stage the card shows or does not: the notch keeps the keyboard while it has something to type into.</summary>
    void Moved(VoiceState st)
    {
        if (disposed) return;
        if (st.Stage is VoiceStage.Idle or VoiceStage.Started or VoiceStage.Cancelled) Done?.Invoke();
        else if (Service.Dictating) { }
        Changed?.Invoke();
    }

    // MARK: The shortcut

    void Register(VoiceSettings s)
    {
        if (disposed) return;
        key?.Dispose();
        key = null;
        // The projects voice may start tasks in: the open chat's, if it is one, wins a tie.
        backend.Send(new RawCommand("projects", new System.Text.Json.Nodes.JsonObject { ["op"] = "list" }));
        if (!s.Enabled)
        {
            Card.HoldError = "";
            // Voice turned off in the middle of an interaction ends it.
            if (!Service.State.Fresh && !Service.Trial) Service.Cancel();
            return;
        }
        key = VoiceHotkey.Register(s.Shortcut, out var error);
        Card.HoldError = error ?? "";
        if (key is null) return;
        var mine = key;
        mine.Pressed += () => queue.TryEnqueue(() => { if (!disposed && key == mine) Pressed(); });
    }

    /// <summary>The chord went down (WM_HOTKEY): a press, and the keys are looked at until they come up.</summary>
    void Pressed()
    {
        var mine = key;
        if (mine is null) return;
        var dictate = Service.State.Fresh && DictationHere();
        var result = Service.PressFromKeys(mine.IsDown, dictate, ActiveProject());
        HostLog.Line($"voice: the shortcut was pressed ({result})");
        if (result == PressResult.Started && !dictate) Began?.Invoke();
        Changed?.Invoke();
    }

    /// <summary>Settings' Try it: the same interaction with nothing started (its stages show in Settings).</summary>
    public void PressTrial() => Service.Press(trial: true);

    public void Dispose()
    {
        if (disposed) return;
        disposed = true;
        backend.Event -= OnBackend;
        key?.Dispose();
        key = null;
        Service.Cancel();
        Card.Dispose();
        Service.Dispose();
    }
}
