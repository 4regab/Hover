using System.Threading.Channels;
using Hover.Host;
using Hover.Host.Protocol;

namespace Hover.Voice;

/// <summary>What a press did.</summary>
public enum PressResult
{
    /// <summary>A new interaction began and the microphone is being opened.</summary>
    Started,
    /// <summary>Toggle mode: the press ended the recording that was under way.</summary>
    Ended,
    /// <summary>An interaction is in progress: the press was turned away (the card glows amber for a moment).</summary>
    Busy,
}

/// <summary>
/// The host's side of voice, with no window: the press and release, the microphone feeding the
/// backend, the backend's `voice` event, and the buttons of the card as commands. A port of the
/// parts of <c>internal/shell/voice.go</c> that are not drawing. The flow itself (transcribing,
/// cleaning up, routing, the preview, starting) is in the backend.
/// </summary>
public sealed class VoiceService : IDisposable
{
    readonly Action<BackendCommand> send;
    readonly MicOpener openMic;
    readonly Channel<Func<Task>> work = Channel.CreateUnbounded<Func<Task>>(new UnboundedChannelOptions { SingleReader = true });
    readonly Task worker;
    readonly object gate = new();
    VoiceState state = VoiceState.Idle;
    VoiceSettings settings = new();
    IMicrophone? mic;
    bool trial, dictating, polling, starting, disposed;
    Timer? closeTimer;
    CancellationTokenSource? poll;

    /// <summary>How long the Started card stays (4 s in the Go app).</summary>
    public TimeSpan StartedFor { get; set; } = TimeSpan.FromSeconds(4);

    /// <summary>How long the Cancelled card stays (1.5 s).</summary>
    public TimeSpan CancelledFor { get; set; } = TimeSpan.FromMilliseconds(1500);

    /// <summary>How often the held keys are looked at (30 ms).</summary>
    public TimeSpan PollEvery { get; set; } = TimeSpan.FromMilliseconds(30);

    public VoiceService(Action<BackendCommand> send, MicOpener? openMic = null)
    {
        this.send = send;
        this.openMic = openMic ?? MicDevices.Open;
        worker = Task.Run(RunAsync);
    }

    public VoiceState State { get { lock (gate) return state; } }

    public VoiceSettings Settings { get { lock (gate) return settings; } }

    /// <summary>The interaction under way is Settings' Try it: its stages show there, not in the notch.</summary>
    public bool Trial { get { lock (gate) return trial; } }

    /// <summary>The interaction is dictation into a chat's reply box: no card.</summary>
    public bool Dictating { get { lock (gate) return dictating; } }

    /// <summary>The backend's `voice` event arrived. Raised on the backend reader's thread.</summary>
    public event Action<VoiceState>? Changed;

    /// <summary>Dictation heard these words (and took these screenshots). Raised on the backend reader's thread, before the interaction is dismissed.</summary>
    public event Action<VoiceState>? Dictated;

    /// <summary>Dictation could not write the words down; this is why, in the card's words.</summary>
    public event Action<string>? DictationFailed;

    /// <summary>A press was turned away because an interaction is in progress.</summary>
    public event Action? Busy;

    /// <summary>The voice settings changed (also the first time they arrive).</summary>
    public event Action<VoiceSettings>? SettingsChanged;

    public event Action<PhononState>? PhononChanged;

    public event Action<GroqCheck>? GroqChecked;

    /// <summary>Hand every backend message to this; the ones about voice are taken, the rest ignored.</summary>
    public void OnBackend(BackendEvent e)
    {
        switch (e)
        {
            case SettingsEvent s:
                var next = VoiceSettings.From(s.Model);
                lock (gate)
                {
                    if (next == settings) return;
                    settings = next;
                }
                SettingsChanged?.Invoke(next);
                return;
            case UnknownEvent { Type: "phonon" }:
                if (PhononState.From(e) is { } p) PhononChanged?.Invoke(p);
                return;
            case UnknownEvent { Type: "groqCheck" }:
                if (GroqCheck.From(e) is { } g) GroqChecked?.Invoke(g);
                return;
        }
        if (VoiceState.From(e) is { } v) Apply(v);
    }

    void Apply(VoiceState v)
    {
        VoiceState before;
        bool wasDictating;
        lock (gate)
        {
            before = state;
            state = v;
            wasDictating = dictating;
        }
        // The backend ended the recording (the cap, a failed device, a cancel): the device goes. Just
        // after a press, an event of a finished interaction may still be on its way: only a stage
        // that is busy with something else says the press was turned away.
        bool stop;
        lock (gate)
        {
            if (starting)
            {
                if (v.Stage == VoiceStage.Recording) starting = false;
                stop = !v.Fresh && v.Stage != VoiceStage.Recording;
                if (stop) starting = false;
            }
            else stop = v.Stage != VoiceStage.Recording && before.Stage == VoiceStage.Recording;
        }
        if (stop) Post(StopMic);

        if (wasDictating && v.Stage != before.Stage)
        {
            switch (v.Stage)
            {
                case VoiceStage.Dictated:
                    Dictated?.Invoke(v);
                    EndDictation();
                    break;
                case VoiceStage.Error or VoiceStage.Cancelled or VoiceStage.Idle:
                    if (v.Stage == VoiceStage.Error) DictationFailed?.Invoke(v.Message);
                    EndDictation();
                    break;
            }
        }
        if (!wasDictating) CloseLater(v, before);
        Changed?.Invoke(v);
    }

    void EndDictation()
    {
        lock (gate) dictating = false;
        send(VoiceCommands.Dismiss);
    }

    /// <summary>The Started and Cancelled cards close by themselves, as in the Go app.</summary>
    void CloseLater(VoiceState v, VoiceState before)
    {
        if (v.Stage == before.Stage && v.Session == before.Session) return;
        bool isTrial;
        lock (gate) isTrial = trial;
        if (isTrial) return;
        TimeSpan after;
        VoiceStage stage = v.Stage;
        switch (stage)
        {
            case VoiceStage.Started: after = StartedFor; break;
            case VoiceStage.Cancelled: after = CancelledFor; break;
            default: return;
        }
        var session = v.Session;
        lock (gate)
        {
            closeTimer?.Dispose();
            closeTimer = new Timer(_ =>
            {
                lock (gate)
                {
                    if (disposed || state.Stage != stage || state.Session != session) return;
                }
                send(VoiceCommands.Dismiss);
            }, null, after, Timeout.InfiniteTimeSpan);
        }
    }

    // MARK: Press and release

    /// <summary>
    /// The shortcut went down (or Try it was pressed). In toggle mode a press while it listens ends the
    /// recording. A press while another interaction is in progress is turned away. Otherwise a new
    /// interaction begins: the backend is told, then the microphone is opened and its blocks follow.
    /// </summary>
    public PressResult Press(bool trial = false, bool dictate = false, string? activeProject = null)
    {
        VoiceState st;
        bool fresh;
        lock (gate)
        {
            st = state;
            if (!settings.Hold && (st.Stage == VoiceStage.Recording || starting))
            {
                Post(ReleaseNow);
                return PressResult.Ended;
            }
            fresh = st.Fresh;
            // A Try it's result waits in Settings until the next press: the shortcut's press replaces it.
            if (!trial && this.trial && st.Stage == VoiceStage.Preview)
            {
                send(VoiceCommands.Cancel);
                fresh = true;
            }
            if (fresh)
            {
                this.trial = trial;
                dictating = !trial && dictate;
                starting = true;
                closeTimer?.Dispose();
                closeTimer = null;
            }
        }
        if (!fresh)
        {
            // The backend turns the press away too; the card shows it.
            Busy?.Invoke();
            return PressResult.Busy;
        }
        Post(() =>
        {
            send(VoiceCommands.Press(trial, !trial && dictate, activeProject));
            StartMic();
        });
        return PressResult.Started;
    }

    /// <summary>The shortcut went down: pressed now, and the keys are looked at every 30 ms until they come up (<paramref name="down"/> says whether they are down).</summary>
    public PressResult PressFromKeys(Func<bool> down, bool dictate = false, string? activeProject = null)
    {
        lock (gate)
        {
            // The poll of an earlier press is still going: the same key press, not a new one.
            if (polling) return PressResult.Busy;
            polling = true;
        }
        var result = Press(false, dictate, activeProject);
        var cts = new CancellationTokenSource();
        lock (gate) poll = cts;
        _ = Task.Run(async () =>
        {
            try
            {
                while (!cts.IsCancellationRequested)
                {
                    await Task.Delay(PollEvery, cts.Token).ConfigureAwait(false);
                    if (down()) continue;
                    break;
                }
            }
            catch (OperationCanceledException) { }
            lock (gate) polling = false;
            if (!cts.IsCancellationRequested) Released();
        });
        return result;
    }

    /// <summary>The shortcut came up. In hold mode that ends the recording; in toggle mode the next press does.</summary>
    public void Released()
    {
        bool hold;
        lock (gate) hold = settings.Hold;
        if (hold) Release();
    }

    /// <summary>Ends the recording: the device goes, its last block is sent, then the release.</summary>
    public void Release() => Post(ReleaseNow);

    void ReleaseNow()
    {
        lock (gate) starting = false;
        StopMic();
        send(VoiceCommands.Release);
    }

    /// <summary>Drops the interaction (Esc on the card, a disabled setting, quitting).</summary>
    public void Cancel()
    {
        lock (gate) starting = false;
        Post(() =>
        {
            StopMic();
            send(VoiceCommands.Cancel);
        });
    }

    /// <summary>A command from the card (Start, Retry, an edit, a pick) to the backend, in order with the audio.</summary>
    public void Send(BackendCommand command) => Post(() => send(command));

    /// <summary>Resolves once everything asked of the service so far has been done.</summary>
    public Task FlushAsync()
    {
        var done = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        Post(() => done.TrySetResult());
        return done.Task;
    }

    // MARK: The microphone

    void StartMic()
    {
        StopMic();
        var opened = openMic(Settings.Microphone, OnBlock, OnFailed);
        if (opened.Mic is null)
        {
            // The backend ends the recording with these words, as the Go app's own flow does.
            send(VoiceCommands.AudioError(opened.Error ?? MicDevices.NoneConnected));
            return;
        }
        lock (gate) mic = opened.Mic;
    }

    void StopMic()
    {
        IMicrophone? m;
        lock (gate)
        {
            m = mic;
            mic = null;
        }
        try { m?.Stop(); }
        catch (Exception e) { HostLog.Line($"voice: the microphone did not stop cleanly — {e.Message}"); }
        m?.Dispose();
    }

    void OnBlock(short[] samples)
    {
        foreach (var command in VoiceCommands.Audio(samples)) send(command);
    }

    void OnFailed(string why) => send(VoiceCommands.AudioError(why));

    // MARK: The worker

    void Post(Func<Task> job)
    {
        if (!work.Writer.TryWrite(job)) HostLog.Line("voice: asked after the service ended");
    }

    void Post(Action job) => Post(() => { job(); return Task.CompletedTask; });

    async Task RunAsync()
    {
        await foreach (var job in work.Reader.ReadAllAsync().ConfigureAwait(false))
        {
            try { await job().ConfigureAwait(false); }
            catch (Exception e) { HostLog.Line($"voice: {e.Message}"); }
        }
    }

    public void Dispose()
    {
        lock (gate)
        {
            if (disposed) return;
            disposed = true;
            closeTimer?.Dispose();
            poll?.Cancel();
        }
        StopMic();
        work.Writer.TryComplete();
        worker.Wait(2000);
    }
}
