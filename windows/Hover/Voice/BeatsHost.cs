using Hover.Host;
using Hover.Voice;
using Hover.Windowing;
using Microsoft.UI.Dispatching;

namespace Hover.Speaking;

/// <summary>
/// The office beats in the app: plays the loop while the office's switch is on and an office is in
/// view, fades it in and out, and lets the sound device go while it is silent. The switch is
/// <see cref="OfficeHost.Beats"/> (office.json's <c>beats</c>); this follows <see cref="OfficeHost.BeatsChanged"/>.
/// A port of the beats parts of <c>internal/shell/office.go</c>.
/// </summary>
sealed class BeatsHost : IDisposable
{
    readonly OfficeHost office;
    readonly Beats beats;
    readonly Func<bool> inView;
    readonly DispatcherQueueTimer fade, watch;
    bool seen = true, disposed;

    public Beats Beats => beats;

    /// <param name="inView">The open notch, or an app window that is shown.</param>
    public BeatsHost(DispatcherQueue queue, OfficeHost office, Func<bool> inView, AudioOpener? open = null)
    {
        this.office = office;
        this.inView = inView;
        beats = new Beats(office.Beats, open ?? AudioOutput.Open, Beats.LoopFile);
        // The page's ramp: a step each animation frame, until it arrives.
        fade = queue.CreateTimer();
        fade.Interval = TimeSpan.FromMilliseconds(16);
        fade.Tick += (_, _) => Guard(() => { if (!beats.Frame()) fade.Stop(); });
        // Whether an office is in view changes with the notch and the window; looked at a few times a second.
        watch = queue.CreateTimer();
        watch.Interval = TimeSpan.FromMilliseconds(150);
        watch.Tick += (_, _) => Guard(Look);
        office.BeatsChanged += OnChanged;
        watch.Start();
        Look();
    }

    void Guard(Action action)
    {
        if (disposed) return;
        try { action(); }
        catch (Exception e) { HostLog.Line($"beats: {e.Message}"); }
    }

    void Look()
    {
        var now = inView();
        if (now == seen && beats.Seen == now) return;
        seen = now;
        beats.Follow(now);
        Fade();
    }

    void Fade()
    {
        if (!fade.IsRunning) fade.Start();
    }

    /// <summary>The switch was turned on or off. With no sound output the switch goes back off, as the Go app's button does.</summary>
    void OnChanged(bool on)
    {
        Guard(() =>
        {
            var worked = beats.Toggle(on);
            if (on && !worked)
            {
                HostLog.Line("beats: no sound output, so the switch is off again");
                office.Beats = false;
            }
            Fade();
        });
    }

    public void Dispose()
    {
        if (disposed) return;
        disposed = true;
        office.BeatsChanged -= OnChanged;
        watch.Stop();
        fade.Stop();
        beats.Dispose();
    }
}
