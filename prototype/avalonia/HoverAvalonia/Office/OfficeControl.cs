using System.Diagnostics;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.OpenGL;
using Avalonia.OpenGL.Controls;
using Avalonia.Threading;
using Silk.NET.OpenGL;

namespace HoverAvalonia.Office;

/// <summary>The office as an Avalonia control: Avalonia's OpenGlControlBase gives a GL context shared with its compositor, this renders into the
/// provided framebuffer, and Avalonia composites the result on the GPU (ANGLE/D3D11 on Windows, GLX or EGL on Linux, CGL on macOS). No readback.
/// Pacing follows Hover: ~30 fps while a bot walks or works, 10 fps idle, nothing at all while hidden (the timer stops).</summary>
public sealed class OfficeControl : OpenGlControlBase
{
    public OfficeScene Scene { get; private set; } = null!; // set by SetScene before the control is attached (a second scene would clobber the shared textures)
    public void SetScene(OfficeScene s) => Scene = s;
    GlRenderer? renderer;
    readonly DispatcherTimer timer = new();
    readonly Stopwatch clock = Stopwatch.StartNew();
    double lastMs;
    public event Action? Stepped;
    public string GlInfo = "(no GL context yet)";
    public string? InitError;
    /// <summary>Wall-clock ms between presented frames and CPU ms spent in each render call (GL work itself is async unless HOVER_GLFINISH=1).</summary>
    public readonly List<double> Intervals = [], RenderMs = [];
    double lastRender;
    public ulong Rendered;
    public readonly List<double> Stamps = [];
    public event Action? FirstFrame; bool firstDone;
    readonly bool finish = Environment.GetEnvironmentVariable("HOVER_GLFINISH") == "1";
    public int FpsLimit = 30, IdleFps = 10;
    bool dragging; Point dragFrom;
    public event Action<int?>? BotClicked;
    public event Action<int>? PropClicked;
    public void RaiseBotClicked(int? id) => BotClicked?.Invoke(id);

    public OfficeControl()
    {
        timer.Tick += (_, _) => Tick();
        timer.Interval = TimeSpan.FromMilliseconds(1000.0 / FpsLimit);
        Focusable = true;
    }

    public void Run(bool on) { if (on) { lastMs = clock.Elapsed.TotalMilliseconds; timer.Start(); } else timer.Stop(); }

    void Tick()
    {
        double now = clock.Elapsed.TotalMilliseconds, dt = now - lastMs; lastMs = now;
        Scene.Resize(Math.Max(1, Bounds.Width), Math.Max(1, Bounds.Height));
        // Several sim steps may be needed when the timer is slower than the 60 Hz step; one call advances by the accumulated dt.
        if (Scene.Frame(now, dt))
        {
            Stepped?.Invoke();
            RequestNextFrameRendering();
        }
        int fps = Scene.Lively ? FpsLimit : IdleFps;
        var want = TimeSpan.FromMilliseconds(1000.0 / fps);
        if (timer.Interval != want) timer.Interval = want;
    }

    protected override void OnOpenGlInit(GlInterface gi)
    {
        try
        {
            var gl = GL.GetApi(gi.GetProcAddress);
            renderer = new GlRenderer(gl, GlVersion.Type == GlProfileType.OpenGLES);
            GlInfo = $"{renderer.GlInfo} [{GlVersion.Type} {GlVersion.Major}.{GlVersion.Minor}]"; Console.WriteLine("gl: " + GlInfo);
        }
        catch (Exception e) { InitError = e.ToString(); Console.Error.WriteLine("GL init failed: " + e); }
    }

    protected override void OnOpenGlDeinit(GlInterface gi) { renderer?.Dispose(); renderer = null; }

    protected override void OnOpenGlRender(GlInterface gi, int fb)
    {
        if (renderer == null) return;
        var scale = TopLevel.GetTopLevel(this)?.RenderScaling ?? 1;
        int w = Math.Max(1, (int)(Bounds.Width * scale)), h = Math.Max(1, (int)(Bounds.Height * scale));
        var t0 = Stopwatch.GetTimestamp();
        try { renderer.Render(Scene, fb, w, h); }
        catch (Exception e) { InitError ??= e.ToString(); Console.Error.WriteLine("render failed: " + e); return; }
        if (finish) GL.GetApi(gi.GetProcAddress).Finish();
        double ms = Stopwatch.GetElapsedTime(t0).TotalMilliseconds, now = clock.Elapsed.TotalMilliseconds;
        if (lastRender > 0) Intervals.Add(now - lastRender);
        lastRender = now; RenderMs.Add(ms); Rendered++; Stamps.Add(now);
        if (Stamps.Count > 20000) Stamps.RemoveRange(0, 10000);
        if (!firstDone) { firstDone = true; FirstFrame?.Invoke(); }
        if (Environment.GetEnvironmentVariable("HOVER_STATS") == "1" && Rendered % 10 == 0) Console.WriteLine($"stats: frames={Rendered} last_render_cpu_ms={ms:F1} interval_ms={(Intervals.Count > 0 ? Intervals[^1] : 0):F1} time={Scene.Time} draws={renderer.Draws} shadow_draws={renderer.ShadowDraws}");
        if (Intervals.Count > 20000) { Intervals.RemoveRange(0, 10000); RenderMs.RemoveRange(0, 10000); }
    }

    // MARK: camera movement + bot selection (drag pans, wheel zooms toward the pointer, click picks)
    protected override void OnPointerPressed(PointerPressedEventArgs e)
    {
        var p = e.GetPosition(this); dragging = true; dragFrom = p; moved = 0; Scene.Pointer = (p.X, p.Y); e.Pointer.Capture(this);
    }
    double moved;
    protected override void OnPointerMoved(PointerEventArgs e)
    {
        var p = e.GetPosition(this); Scene.Pointer = (p.X, p.Y);
        if (dragging)
        {
            var d = p - dragFrom; moved += Math.Abs(d.X) + Math.Abs(d.Y);
            if (moved > 4) { Scene.Dragging = true; Scene.Drag(d.X, d.Y); dragFrom = p; }
        }
        RequestNextFrameRendering();
    }
    protected override void OnPointerReleased(PointerReleasedEventArgs e)
    {
        bool click = dragging && moved <= 4; dragging = false; Scene.Dragging = false; e.Pointer.Capture(null);
        if (!click) return;
        var p = e.GetPosition(this);
        var id = Scene.PickAt(p.X, p.Y, out int prop);
        if (id is int sid) { Scene.Sel = sid; BotClicked?.Invoke(sid); }
        else if (prop == 3) { Scene.ApplyTime(Scene.Time == TimeOfDay.Day ? TimeOfDay.Night : TimeOfDay.Day); } // the window toggles day/night, as in Hover
        else if (prop >= 0) PropClicked?.Invoke(prop);
        else { Scene.Sel = null; BotClicked?.Invoke(null); }
    }
    protected override void OnPointerExited(PointerEventArgs e) { Scene.Pointer = null; }
    protected override void OnPointerWheelChanged(PointerWheelEventArgs e)
    {
        var p = e.GetPosition(this);
        Scene.ZoomBy(Math.Pow(1.0015, e.Delta.Y * 100), p.X - Bounds.Width / 2, -(p.Y - Bounds.Height / 2));
    }
}
