using System.Globalization;

namespace Hover.Office;

// The wall canvases (main.js's drawSky, drawTV, drawBoard, drawClock and the beam and patch
// textures), drawn on the CPU and handed to the renderer as textures when they change.
// The rest of internal/office/office.go.

public sealed partial class OfficeModel
{
    int Count(params Stage[] st) => Sessions.Count(s => st.Contains(s.Last().Stage));

    Canvas NewCanvas(int i, int w, int h)
    {
        var c = new Canvas(w, h, text);
        Canvases[i] = c;
        return c;
    }

    static string HexCss((byte R, byte G, byte B) c) => $"#{c.R:x2}{c.G:x2}{c.B:x2}";

    void DrawTextures()
    {
        // glowTex (the sprites' own texture is made by the renderer), beamTex, patchTex.
        var c = Canvases[Layout.TexBeam];
        c.GradientV(0, 64, [new Stop(0, 1, 1, 1, 0.9), new Stop(1, 1, 1, 1, 0)], 0, 0, 4, 64);
        c = Canvases[Layout.TexPatch];
        c.Blur = 2;
        c.Style("#fff");
        foreach (var (px, py) in new[] { (4.0, 4.0), (34.0, 4.0), (4.0, 34.0), (34.0, 34.0) }) c.Rect(px, py, 26, 26);
        ApplyTime(TimeOfDay.Night);
        DrawBoard();
        DrawClock();
    }

    public void DrawSky()
    {
        var night = Time == TimeOfDay.Night;
        var x = NewCanvas(Layout.TexSky, 128, 96);
        var rr = new Rng(3);
        var stops = new[] { Css.StopOf(0, "#5eb0ff"), Css.StopOf(1, "#cfe8ff") };
        if (night) stops = [Css.StopOf(0, "#070a24"), Css.StopOf(1, "#2a2458")];
        x.GradientV(0, 96, stops, 0, 0, 128, 96);
        if (night)
        {
            for (var i = 0; i < 40; i++)
            {
                var c = "#9aa6ff";
                if (rr.Next() < 0.3) c = "#fff";
                x.Style(c);
                double a = Math.Truncate(rr.Next() * 128), b = Math.Truncate(rr.Next() * 55);
                x.Rect(a, b, 1, 1);
            }
            x.Style("#fff2cc");
            x.Rect(92, 12, 12, 12);
            x.Rect(90, 14, 16, 8);
            x.Style("#e6d6a8");
            x.Rect(96, 16, 3, 3);
            x.Rect(100, 20, 2, 2);
        }
        else
        {
            x.Style("#fff6d8");
            x.Rect(96, 10, 12, 12);
            x.Style("#fff");
            foreach (var c in new[] { (14.0, 20.0, 26.0), (58.0, 12.0, 20.0), (70.0, 34.0, 30.0) })
            {
                x.Rect(c.Item1, c.Item2, c.Item3, 5);
                x.Rect(c.Item1 + 4, c.Item2 - 3, c.Item3 - 10, 3);
            }
        }
        for (double bx = 0; bx < 128;)
        {
            var w = 8 + Math.Truncate(rr.Next() * 14);
            var h = 18 + Math.Truncate(rr.Next() * 36);
            x.Style(night ? "#120e2a" : "#8fb2d6");
            x.Rect(bx, 96 - h, w, h);
            for (var wy = 96 - h + 3; wy < 94; wy += 4)
                for (var wx = bx + 2; wx < bx + w - 2; wx += 3)
                {
                    var p = night ? 0.35 : 0.2;
                    if (rr.Next() < p)
                    {
                        var c = "#dbe9f8";
                        if (night)
                        {
                            c = "#b99bff";
                            if (rr.Next() < 0.8) c = "#ffd27a";
                        }
                        x.Style(c);
                        x.Rect(wx, wy, 1, 2);
                    }
                }
            bx += w + 1;
        }
        Dirty[Layout.TexSky] = true;
    }

    /// <summary>The session the TV follows: the open one, else the first at work.</summary>
    Session? TvSession()
    {
        if (HasSel && DrawerOpen)
            foreach (var s in Sessions)
                if (s.Id == Sel) return s;
        return Sessions.FirstOrDefault(s => s.Busy);
    }

    public void DrawTV(double t)
    {
        var rows = new (string Label, int N, string Color)[]
        {
            ("Working", Count(Stage.Waking, Stage.Working, Stage.Waiting), "#c4a2ff"), ("Done", Count(Stage.Done), "#4ade80"),
            ("Failed", Count(Stage.Failed), "#ff6b62"), ("Stopped", Count(Stage.Stopped), "#8a8fa0"),
        };
        string name = "", what = "";
        (byte R, byte G, byte B) css = default;
        double ctx = 0;
        var hasCtx = false;
        var cur = TvSession();
        if (cur is not null)
        {
            what = cur.Last().Stage == Stage.Working ? (cur.Act + " " + Short(cur.File)).Trim() : cur.Last().Stage.Word();
            (name, css, ctx, hasCtx) = (cur.B.Name, cur.B.Css, cur.Ctx, cur.HasCtx);
        }
        var x = NewCanvas(Layout.TexTV, 208, 118);
        x.Style("#061022");
        x.Rect(0, 0, 208, 118);
        x.Style("#0b1b36");
        for (double y = 0; y < 118; y += 3) x.Rect(0, y, 208, 1);
        x.Font = CanvasFont.Pixel(11, true);
        x.Baseline = Baseline.Top;
        x.Style("#9ad2ff");
        x.Text("AGENT OFFICE", 10, 8);
        x.Style("#2f5a8a");
        x.Rect(10, 22, 188, 1);
        for (var i = 0; i < rows.Length; i++)
        {
            var rw = rows[i];
            double yy = 30 + i * 14;
            x.Style("#6fa8d8");
            x.Text(rw.Label, 10, yy);
            x.Style(rw.Color);
            x.Text(rw.N.ToString(CultureInfo.InvariantCulture), 70, yy);
            for (var k = 0; k < rw.N; k++) x.Rect(86 + k * 8, 33 + i * 14, 6, 6);
        }
        if (cur is not null)
        {
            x.Style(HexCss(css));
            x.Text(name, 10, 90);
            x.Style("#cfe6ff");
            var shown = what;
            if (Utf16Len(what) > 24) shown = Utf16Take(what, 23) + "…";
            x.Text(shown, 46, 90);
            if (hasCtx)
            {
                x.Style("#1c3458");
                x.Rect(10, 105, 150, 5);
                x.Style("#9ad2ff");
                x.Rect(10, 105, 1.5 * ctx, 5);
                x.Style("#6fa8d8");
                x.Text(ctx.ToString(CultureInfo.InvariantCulture) + "%", 166, 101);
            }
        }
        else
        {
            x.Style("#6fa8d8");
            x.Text("No sessions yet", 10, 90);
        }
        if (Js.FloorI(t * 2) % 2 != 0)
        {
            x.Style("#9ad2ff");
            x.Rect(190, 8, 6, 10);
        }
        Dirty[Layout.TexTV] = true;
    }

    static int Utf16Len(string s) => s.Length;

    /// <summary>The first n UTF-16 units of s (a pair cut in half is dropped, as from_utf16_lossy does).</summary>
    static string Utf16Take(string s, int n)
    {
        if (s.Length <= n) return s;
        var cut = s[..n];
        if (char.IsHighSurrogate(cut[^1])) cut = cut[..^1] + "\ufffd";
        return cut;
    }

    public void DrawBoard()
    {
        var cols = new (string Head, string Color, Stage[] Stages)[]
        {
            ("WAKING", "#f5b83d", [Stage.Waking]), ("DOING", "#9b6bff", [Stage.Working, Stage.Waiting]), ("FINISHED", "#2fae66", [Stage.Done, Stage.Failed, Stage.Stopped]),
        };
        var notes = Sessions.Select(s => (Stage: s.Last().Stage, Name: s.B.Name, Css: s.B.Css, s.Title)).ToList();
        var x = NewCanvas(Layout.TexBoard, 480, 280);
        x.Scale = 2;
        x.Style("#e9e3d6");
        x.Rect(0, 0, 240, 140);
        x.Style("#d6cebd");
        x.Rect(0, 132, 240, 8);
        x.Baseline = Baseline.Top;
        for (var i = 0; i < cols.Length; i++)
        {
            var c = cols[i];
            double cx = 8 + i * 78;
            x.Font = CanvasFont.Pixel(11, true);
            x.Style(c.Color);
            x.Rect(cx, 7, 70, 14);
            x.Style("#fff");
            x.Text(c.Head, cx + 5, 8);
            if (i > 0 && notes.Count > 0)
            {
                x.Style("#cfc6b3");
                x.Rect(cx - 5, 8, 1, 118);
            }
            var list = notes.Where(n => c.Stages.Contains(n.Stage)).ToList();
            var room = list.Count > 3 ? 2 : 3;
            for (var k = 0; k < list.Count; k++)
            {
                if (k >= room) break;
                var n = list[k];
                double ny = 27 + k * 34;
                x.Style("rgba(0,0,0,.14)");
                x.Rect(cx + 1.5, ny + 1.5, 68, 31);
                x.Style("#fbf8f1");
                x.Rect(cx, ny, 68, 31);
                x.Style(HexCss(n.Css));
                x.Rect(cx, ny, 3, 31);
                x.Font = CanvasFont.Pixel(8, true);
                x.Style("#2a2233");
                x.Text(n.Name, cx + 6, ny + 3);
                var mark = n.Stage switch { Stage.Failed => "#ff453a", Stage.Stopped => "#8e8a96", Stage.Done => "#2fae66", _ => "" };
                if (mark != "")
                {
                    x.Style(mark);
                    x.Rect(cx + 60, ny + 4, 5, 5);
                }
                // The title on up to two lines.
                x.Font = CanvasFont.Inter(7);
                x.Style("#5a5263");
                var words = n.Title.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);
                string line = "";
                var row = 0;
                for (var j = 0; j < words.Length && row < 2;)
                {
                    var next = words[j];
                    if (line != "") next = line + " " + words[j];
                    if (x.Measure(next) <= 58 || line == "")
                    {
                        line = next;
                        j++;
                        continue;
                    }
                    if (row == 1)
                    {
                        line = next;
                        break;
                    }
                    x.Text(Fit(x, line, 58), cx + 6, ny + 13);
                    row = 1;
                    line = words[j];
                    j++;
                }
                if (line != "") x.Text(Fit(x, line, 58), cx + 6, ny + 13 + row * 8.5);
            }
            if (list.Count > room)
            {
                x.Font = CanvasFont.Inter(7);
                x.Style("#7a7282");
                x.Text($"+{list.Count - room} more", cx + 3, 29 + room * 34);
            }
        }
        if (notes.Count == 0)
        {
            x.Align = Align.Center;
            x.Style("#3a3044");
            x.Font = CanvasFont.Pixel(16, true);
            x.Text("The office is quiet", 120, 46);
            x.Style("#5e5666");
            x.Font = CanvasFont.Inter(10);
            x.Text("Give Kiro, Codex, Cursor, OpenCode", 120, 72);
            x.Text("or Claude Code a task, and a bot", 120, 86);
            x.Text("walks in to do it.", 120, 100);
            x.Align = Align.Start;
        }
        Dirty[Layout.TexBoard] = true;
    }

    public void DrawClock()
    {
        var (ms, off) = WallClock();
        var local = (long)Math.Floor(ms / 1000) + off * 60L;
        static long Mod(long a, long b) => (a % b + b) % b;
        long h = Mod(local, 86400) / 3600, m = Mod(local, 3600) / 60, s = Mod(local, 60);
        var x = NewCanvas(Layout.TexClock, 96, 44);
        x.Style("#0f0d12");
        x.Rect(0, 0, 96, 44);
        x.Font = CanvasFont.Pixel(30, true);
        x.Baseline = Baseline.Middle;
        x.Align = Align.Center;
        x.Style("#3a1a0c");
        x.Text("88 88", 48, 23);
        // The colon blinks with the real seconds.
        var sep = s % 2 != 0 ? " " : ":";
        x.Style("#ff8a3a");
        x.Text($"{h:00}{sep}{m:00}", 48, 23);
        Dirty[Layout.TexClock] = true;
    }

    /// <summary>Cuts text with an ellipsis to fit the width.</summary>
    static string Fit(Canvas x, string s, double w)
    {
        if (x.Measure(s) <= w) return s;
        var t = s.EnumerateRunes().ToList();
        string Join() => string.Concat(t.Select(r => r.ToString()));
        while (t.Count > 0 && x.Measure(Join() + "…") > w) t.RemoveAt(t.Count - 1);
        return Join().TrimEnd(' ', '\t', '\n') + "…";
    }
}
