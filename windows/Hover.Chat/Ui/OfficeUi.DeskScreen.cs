using Hover.Chat.Desk;
using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

public sealed partial class OfficeUi
{
    // The desk's Screen tab (internal/ui/deskpanel.go screen, internal/shell/desk.go deskScreenTick): the desktop with only the windows of
    // the apps the agent opened over it. At rest it is the desktop once; "Watch live" (or the agent testing with computer use) takes a
    // new picture about four times a second. The app makes the pictures (UiEnv.Screen); this draws them and asks for the next.

    sealed class ScreenState
    {
        public bool Watch, Busy, Still;
        /// <summary>The picture shown, an address UiEnv.Images knows.</summary>
        public string? Src;
        /// <summary>Why the last picture could not be taken; replaces the line under the picture until one is.</summary>
        public string Err = "";
        public double NextAt;
    }

    const double ScreenEvery = 250;

    readonly Dictionary<int, ScreenState> screens = [];

    ScreenState ScreenOf(int id)
    {
        if (!screens.TryGetValue(id, out var st)) screens[id] = st = new ScreenState();
        return st;
    }

    /// <summary>Asks the app for the next picture when the tab is in view and one is due; once a frame.</summary>
    void ScreenTick()
    {
        if (deskPanel is not { } id || deskTab != "screen")
        {
            ScreenIdle();
            return;
        }
        if (env.Screen is not { Supported: true } feed || DeskSession(id) is not { } s) return;
        var st = ScreenOf(id);
        var info = ScreenInfo.Of(s);
        var live = st.Watch || info.Testing;
        if (st.Busy)
        {
            Need(100);
            return;
        }
        // At rest the desktop is taken once; live, a picture is due every quarter second.
        if (!live && st.Still) return;
        if (live && now < st.NextAt)
        {
            Need(st.NextAt - now);
            return;
        }
        st.Busy = true;
        if (live) Need(ScreenEvery);
        feed.Take(info.Pids, live, (src, why) =>
        {
            st.Busy = false;
            if (!live) st.Still = true;
            if (why is not null) st.Err = why;
            else if (src is not null)
            {
                st.Err = "";
                var old = st.Src;
                st.Src = src;
                if (old is not null && old != src) feed.Release(old);
            }
            st.NextAt = now + ScreenEvery;
            Dirty = true;
        });
    }

    /// <summary>The tab is not in view: the desktop is taken again when it comes back, and with the panel shut the pictures go.</summary>
    void ScreenIdle()
    {
        if (screens.Count == 0) return;
        foreach (var st in screens.Values) st.Still = false;
        if (deskPanel is not null || env.Screen is not { } feed) return;
        foreach (var st in screens.Values)
        {
            if (st.Src is not { } src) continue;
            feed.Release(src);
            st.Src = null;
        }
    }

    void ScreenToggleWatch()
    {
        if (deskPanel is not { } id) return;
        var st = ScreenOf(id);
        st.Watch = !st.Watch;
        // Stopping takes the desktop alone again, so the picture does not stay a live one under the word "Desktop".
        st.Still = false;
        st.NextAt = 0;
        DeskChanges();
    }

    /// <summary>The tab's body in <paramref name="r"/>: the state and the watch button, the picture in a black box, the line under it.</summary>
    void PaintScreen(SessionInfo s, Rect4 r)
    {
        var supported = env.Screen is { Supported: true };
        var st = ScreenOf(s.Id);
        var info = ScreenInfo.Of(s);
        var live = supported && (st.Watch || info.Testing);
        float x = r.X, y = r.Y, w = r.W, h = r.H;

        var label = live ? "Live" : "Desktop";
        var lf = Pen.F(11.5, 600, live ? Rgba.Of(0xffb3ad) : Pen.Dim);
        var lw = pen.Width(label, lf) + 9 + 9 + 6 + 7;
        pen.Box(x, y, lw, 22, 11, live ? Rgba.Of(0xff453a, 0.16) : Rgba.Of(0xffffff, 0.07));
        pen.Box(x + 9, y + (22 - 7) / 2, 7, 7, 3.5f, live ? Rgba.Of(0xff453a) : Rgba.Of(0xffffff, 0.4));
        pen.Text(label, x + 9 + 7 + 6, y + (22 - pen.LineHeight(lf)) / 2, lf);
        if (supported) DeskButton("sWatch", st.Watch ? "Stop watching" : "Watch live", null, x + w, y + (22 - 26) / 2f, true, false, 0, "", false, true);

        var cy = y + 22 + 8;
        var note = st.Err.Length > 0 ? st.Err : supported ? ScreenInfo.Note(info, live, Bots.Name(s.Bot)) : DeskTiles.ScreenNote;
        var nf = Pen.F(11.5, 400, Pen.Faint);
        var noteH = supported ? pen.WrappedHeight(note, w, nf) + 8 : 0;
        var ah = Math.Max(120, y + h - cy - noteH);
        pen.Box(x, cy, w, ah, 10, Rgba.Of(0x000000));
        pen.Border(x, cy, w, ah, 10, 1, Rgba.Of(0xffffff, 0.08));

        var shown = st.Src is { } src ? env.Images(src) : new ImageState(ImageKind.Pending);
        if (shown.Kind != ImageKind.Ready || shown.W < 1 || shown.H < 1)
        {
            pen.TextIn(supported ? "Reading…" : note, new Rect4(x + 10, cy, w - 20, ah), Pen.F(12, 400, Pen.Faint));
        }
        else
        {
            // Fitted inside the box, whole.
            var k = Math.Min((w - 2) / shown.W, (ah - 2) / shown.H);
            float iw = shown.W * k, ih = shown.H * k;
            pen.Cv.Image(st.Src!, new Rect4(x + (w - iw) / 2, cy + (ah - ih) / 2, iw, ih), 0, false);
        }
        if (supported) pen.Wrapped(note, x, cy + ah + 8, w, nf);
    }
}
