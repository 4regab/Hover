using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text.Json;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Threading;
using HoverAvalonia.Core;
using HoverAvalonia.Ui;

namespace HoverAvalonia;

/// <summary>Port of Hover's <c>--selftest</c> idea: on a real X display, drive the notch with the real pointer and keyboard (XWarpPointer, XTest) and write report.json.
/// Linux/X11 only — it is how the click-through, hover dwell, Esc/Alt+N and focus rules were actually exercised in this prototype.</summary>
static class SelfTest
{
    [DllImport("libX11.so.6")] static extern nint XOpenDisplay(nint n);
    [DllImport("libX11.so.6")] static extern nint XDefaultRootWindow(nint d);
    [DllImport("libX11.so.6")] static extern int XWarpPointer(nint d, nint src, nint dst, int sx, int sy, uint sw, uint sh, int dx, int dy);
    [DllImport("libX11.so.6")] static extern int XQueryPointer(nint d, nint w, out nint root, out nint child, out int rx, out int ry, out int wx, out int wy, out uint mask);
    [DllImport("libX11.so.6")] static extern int XFlush(nint d);
    [DllImport("libX11.so.6")] static extern int XSync(nint d, int discard);
    [DllImport("libX11.so.6")] static extern int XKeysymToKeycode(nint d, nint keysym);
    [DllImport("libXtst.so.6")] static extern int XTestFakeKeyEvent(nint d, uint keycode, int press, ulong delay);
    [DllImport("libXtst.so.6")] static extern int XTestFakeButtonEvent(nint d, uint button, int press, ulong delay);

    static nint dpy, root;
    static void Warp(int x, int y) { XWarpPointer(dpy, 0, root, 0, 0, 0, 0, x, y); XSync(dpy, 0); }
    static nint ChildUnderPointer() { XQueryPointer(dpy, root, out _, out var child, out _, out _, out _, out _, out _); return child; }
    static void Chord(nint[] mods, nint key)
    {
        foreach (var m in mods) XTestFakeKeyEvent(dpy, (uint)XKeysymToKeycode(dpy, m), 1, 0);
        XTestFakeKeyEvent(dpy, (uint)XKeysymToKeycode(dpy, key), 1, 0); XTestFakeKeyEvent(dpy, (uint)XKeysymToKeycode(dpy, key), 0, 0);
        foreach (var m in mods.Reverse()) XTestFakeKeyEvent(dpy, (uint)XKeysymToKeycode(dpy, m), 0, 0);
        XSync(dpy, 0);
    }

    static long RssKb() { foreach (var l in File.ReadLines("/proc/self/status")) if (l.StartsWith("VmRSS:")) return long.Parse(l.Split(' ', StringSplitOptions.RemoveEmptyEntries)[1]); return -1; }
    static double Pct(List<double> v, double p) { if (v.Count == 0) return 0; var s = v.OrderBy(x => x).ToList(); return s[(int)Math.Round((s.Count - 1) * p)]; }

    public static void Start(NotchWindow w, Options o) => _ = Task.Run(() => Run(w, o));
    // The script runs on a pool thread: with software GL the UI thread is busy rendering, and Background-priority continuations starved there.
    static T UI<T>(Func<T> f) => Dispatcher.UIThread.Invoke(f, DispatcherPriority.Send);
    static void UI(Action f) => Dispatcher.UIThread.Invoke(f, DispatcherPriority.Send);

    static void Shot(string dir, string name, PixelRect? crop = null)
    {
        Console.WriteLine($"st: shot {name}");
        var args = crop is { } c ? $"-window root -crop {c.Width}x{c.Height}+{c.X}+{c.Y} +repage {Path.Combine(dir, name)}" : $"-window root {Path.Combine(dir, name)}";
        using var p = Process.Start(new ProcessStartInfo("import", args) { RedirectStandardError = true }); p!.WaitForExit(15000);
    }

    static async Task Run(NotchWindow w, Options o)
    {
        var dir = o.Selftest!; Directory.CreateDirectory(dir);
        var rep = new Dictionary<string, object?>(); var checks = new Dictionary<string, object?>();
        void Save() { try { rep["checks"] = checks; File.WriteAllText(Path.Combine(dir, "report.json"), JsonSerializer.Serialize(rep, new JsonSerializerOptions { WriteIndented = true })); } catch (Exception e) { Console.WriteLine("st: save failed " + e.Message); } }
        int step = 0; async Task Wait(int ms) { Console.WriteLine($"st: wait {++step} {ms}"); await Task.Delay(ms); Save(); }
        try
        {
            dpy = XOpenDisplay(0); root = XDefaultRootWindow(dpy);
            await Wait(2500);
            nint xid = UI(() => w.TryGetPlatformHandle()?.Handle ?? 0);
            rep["platform_adapter"] = w.Plat.Name; rep["os"] = RuntimeInformation.OSDescription; rep["runtime"] = RuntimeInformation.FrameworkDescription;
            rep["screens"] = UI(() => w.Screens.All.Select(s => new { Bounds = s.Bounds.ToString(), WorkingArea = s.WorkingArea.ToString(), s.Scaling, s.IsPrimary }).ToList());
            rep["window_rect"] = w.WinRect.ToString(); rep["window_xid"] = (long)xid;
            int cx = w.WinRect.X + w.WinRect.Width / 2;
            var crop = new PixelRect(w.WinRect.X, w.WinRect.Y, w.WinRect.Width, w.WinRect.Height);

            // 1. Resting: pill drawn, never takes focus, pointer passes through everywhere but the shape.
            Shot(dir, "01-rest.png", crop);
            Warp(cx, 10); await Wait(150); var overPill = ChildUnderPointer();
            Warp(cx, 300); await Wait(150); var belowShape = ChildUnderPointer();
            Warp(w.WinRect.X + 30, 10); await Wait(150); var leftOfShape = ChildUnderPointer();
            checks["pointer_over_pill_hits_notch_window"] = overPill == xid;
            checks["pointer_inside_window_but_outside_shape_clicks_through"] = belowShape != xid;
            checks["pointer_left_of_shape_at_top_clicks_through"] = leftOfShape != xid;
            checks["rest_focus_not_ours"] = !UI(() => w.Plat.ForegroundIsOurs());

            // 2. Hover dwell opens (peek), then settles; frames recorded for 6 s at the demo preset.
            Warp(cx, 3); var t0 = Stopwatch.StartNew();
            while (w.State == NotchGeometry.State.Rest && t0.ElapsedMilliseconds < 3000) await Wait(20);
            rep["hover_open_after_ms"] = w.State == NotchGeometry.State.Rest ? -1 : t0.ElapsedMilliseconds;
            await Wait(1200);
            checks["hover_opened_peek"] = w.State == NotchGeometry.State.Peek; checks["openness_after_open"] = w.OpennessNow;
            rep["gl"] = w.OfficeCtl?.GlInfo; rep["gl_error"] = w.OfficeCtl?.InitError;
            Shot(dir, "02-office-open.png", crop);
            if (w.OfficeCtl != null) { w.OfficeCtl.Intervals.Clear(); w.OfficeCtl.RenderMs.Clear(); }
            Warp(cx, 200); await Wait(6000);   // pointer stays in the panel: it stays open
            checks["stays_open_while_pointer_in_panel"] = w.State != NotchGeometry.State.Rest;
            var oc = w.OfficeCtl!; rep["frames_6s"] = new { count = oc.Intervals.Count, interval_p50 = Pct(oc.Intervals, .5), interval_p95 = Pct(oc.Intervals, .95), interval_p99 = Pct(oc.Intervals, .99),
                render_cpu_ms_p50 = Pct(oc.RenderMs, .5), render_cpu_ms_p95 = Pct(oc.RenderMs, .95), draws = "see gl", rss_kb = RssKb() };

            // 3. Select a bot (click), camera closes in, chat shows.
            var sel = w.Scene.Sessions[0]; UI(() => { w.Scene.Sel = sel.Id; w.OfficeCtl!.RaiseBotClicked(sel.Id); });
            await Wait(1500); Shot(dir, "03-chat.png", crop);
            checks["selected_bot_zoomed"] = w.Scene.Cam[3] > 1.3;
            // Camera movement: drag + wheel.
            var before = (double[])w.Scene.User.Clone();
            UI(() => { w.Scene.Sel = null; w.OfficeCtl!.RaiseBotClicked(null); w.Scene.Drag(120, 40); w.Scene.ZoomBy(1.6, 0, 0); }); await Wait(1200);
            checks["camera_moved"] = w.Scene.User[0] != before[0] || w.Scene.User[1] != before[1]; checks["camera_zoomed"] = w.Scene.User[2] > 1.0;
            Shot(dir, "04-camera.png", crop); UI(() => w.Scene.ResetView()); await Wait(600);

            // 4. Esc / leave closes; Alt+N (real XGrabKey, real XTest key events) toggles.
            Warp(cx, 900); t0.Restart();
            while (w.State != NotchGeometry.State.Rest && t0.ElapsedMilliseconds < 4000) await Wait(20);
            checks["leaving_collapses_after_grace"] = w.State == NotchGeometry.State.Rest; rep["collapse_after_ms"] = t0.ElapsedMilliseconds;
            await Wait(700);
            Chord([0xffe9], 'n'); await Wait(1000);   // Alt_L + n
            checks["alt_n_global_hotkey_opens"] = w.State == NotchGeometry.State.Open;
            Chord([0xffe9], 'n'); await Wait(900);
            checks["alt_n_global_hotkey_closes"] = w.State == NotchGeometry.State.Rest;

            // 5. Repeated open/close: memory after each cycle (managed + RSS).
            var cycles = new List<object>();
            for (int i = 0; i < 4; i++)
            {
                UI(() => w.Expand(false, false)); await Wait(2500);
                UI(() => w.Collapse()); await Wait(Math.Max(2200, o.DropMs + 800));
                GC.Collect(); cycles.Add(new { cycle = i + 1, rss_kb = RssKb(), managed_kb = GC.GetTotalMemory(true) / 1024, office_alive = w.OfficeCtl != null });
            }
            rep["open_close_cycles"] = cycles;
            rep["checks"] = checks; rep["done"] = true;
        }
        catch (Exception e) { rep["error"] = e.ToString(); }
        File.WriteAllText(Path.Combine(dir, "report.json"), JsonSerializer.Serialize(rep, new JsonSerializerOptions { WriteIndented = true }));
        Console.WriteLine("selftest written");
        Dispatcher.UIThread.Post(() => (Avalonia.Application.Current!.ApplicationLifetime as Avalonia.Controls.ApplicationLifetimes.IClassicDesktopStyleApplicationLifetime)!.Shutdown());
    }
}
