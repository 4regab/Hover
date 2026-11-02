using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host.Protocol;

namespace Hover.Settings;

/// <summary>A canvas that draws nothing: a form is laid out once on it to learn its height.</summary>
sealed class NullCanvas : ICanvas
{
    public void PushClip(Rect4 r) { }
    public void PopClip() { }
    public void PushMove(float dx, float dy) { }
    public void PopMove() { }
    public void PushOpacity(float opacity) { }
    public void PopOpacity() { }
    public void Fill(Rect4 r, float[] radius, Rgba c) { }
    public void Stroke(Rect4 r, float[] radius, float width, Rgba c) { }
    public void Line(IReadOnlyList<(float X, float Y)> points, float width, Rgba c) { }
    public void Polygon(IReadOnlyList<(float X, float Y)> points, Rgba c) { }
    public void Glow(float cx, float cy, float radius, Rgba c) { }
    public void Text(IParaLayout layout, IReadOnlyList<StyleRun> runs, float x, float y) { }
    public void Svg(string svg, Rect4 r) { }
    public void Image(string src, Rect4 r, float radius, bool cover) { }
    public void BrokenImage(float x, float y) { }
    public void Path(IReadOnlyList<Hover.Notch.PathCmd> path, Rgba? fill, Rgba? stroke, float strokeWidth, bool evenOdd, float[]? dash) { }
    public void Label(string text, float x, float y, float size, float weight, Rgba c, Align anchor, FontFamily family) { }
}

public sealed partial class SettingsUi
{
    const float MonoLH = 0.928f + 0.236f;

    static readonly NullCanvas Null = new();

    /// <summary>Runs a drawing on the canvas that draws nothing and answers no pointer, and says what it returned.</summary>
    float Measure(Func<float> draw)
    {
        var cv = pen.Cv;
        var count = hits.Count;
        var stopCount = stops.Count;
        var drawn = pen.Drawn.Count;
        pen.Cv = Null;
        try { return draw(); }
        finally
        {
            pen.Cv = cv;
            if (hits.Count > count) hits.RemoveRange(count, hits.Count - count);
            if (stops.Count > stopCount) stops.RemoveRange(stopCount, stops.Count - stopCount);
            if (pen.Drawn.Count > drawn) pen.Drawn.RemoveRange(drawn, pen.Drawn.Count - drawn);
        }
    }

    /// <summary>The MCP section's small icon button (edit, remove, drop a row).</summary>
    void MiniIcon(string action, string what, string icon, Rgba hotColour, float x, float y)
    {
        var hover = Hot(TK.Mini, action, what.GetHashCode());
        if (hover) Box(x, y, 28, 28, 7, C(P.RowHover));
        Icon(icon, x + 7, y + 7, 14, hover ? hotColour : Dim);
        Add(new Rect4(x, y, 28, 28), TK.Mini, action, what.GetHashCode(), what);
    }

    float McpFieldH(string value, bool multi)
    {
        if (!multi) return 32;
        var n = Math.Max(1, value.Count(c => c == '\n') + 1);
        return Math.Max(58, n * 12 * MonoLH + 14);
    }

    float McpFieldAt(string key, string field, string value, string placeholder, bool bad, bool multi, float x, float y, float width)
    {
        var fh = McpFieldH(ctlText(key, value), multi);
        DrawField(key, new Rect4(x, y, width, fh), value, placeholder, false, true, 8, field, mono: true, multi: multi, bad: bad);
        return fh;
    }

    string ctlText(string key, string value) => fields.TryGetValue(key, out var f) && FocusId == key ? f.Box.Text : value;

    float Labelled(string label, string? problem, float x, float y, float width, Func<float, float> field)
    {
        pen.Text(label, x, y, F(12, 400, Dim));
        var hh = Tl(12) + 5;
        hh += field(y + hh);
        if (!string.IsNullOrEmpty(problem))
        {
            var eh = pen.Wrapped(problem, x, y + hh + 5, width, F(11.5f, 400, RedInk));
            hh += 5 + eh;
        }
        return hh;
    }

    PillSpec Small(string text) => new(text, "", 12, 4, 12, 500, Ink, C(P.Fill), true);

    float McpForm(McpForm f, float x, float y, float width)
    {
        var k = f.Serial.ToString();
        float ix = x + 14, iw = width - 28;
        var cy = y + 14;
        cy += Labelled("Name", f.NameProblem, ix, cy, iw, fy => McpFieldAt($"mcpf:{k}:name", "name", f.Name, "github", f.NameProblem is not null, false, ix, fy, iw)) + 12;
        string[] kinds = ["A command on this computer", "A URL"];
        var (_, sh) = SegSize(2, kinds[0]);
        DrawSegments(ix, cy, kinds, kinds[0], f.Remote ? 1 : 0, true, "McpKind");
        cy += sh + 12;
        if (f.Remote)
            cy += Labelled("URL", f.UrlProblem, ix, cy, iw, fy => McpFieldAt($"mcpf:{k}:url", "url", f.Url, "https://example.com/mcp", f.UrlProblem is not null, false, ix, fy, iw)) + 12;
        else
        {
            var hw = (iw - 8) / 2;
            var h1 = Labelled("Command", f.CommandProblem, ix, cy, hw, fy => McpFieldAt($"mcpf:{k}:command", "command", f.Command, "npx", f.CommandProblem is not null, false, ix, fy, hw));
            var h2 = Labelled("Arguments", null, ix + hw + 8, cy, hw, fy => McpFieldAt($"mcpf:{k}:args", "args", f.Args, "One per line", false, true, ix + hw + 8, fy, hw));
            cy += Math.Max(h1, h2) + 12;
        }
        pen.Text(f.Remote ? "Headers" : "Environment variables", ix, cy, F(11.5f, 400, Dim));
        cy += Tl(11.5f);
        var kw = (iw - 6 - 6 - 28) / 2.4f;
        var vw = kw * 1.4f;
        for (var i = 0; i < f.Pairs.Count; i++)
        {
            cy += 6;
            var pair = f.Pairs[i];
            // The name goes red as it is typed, if Kiro could not use it.
            var fh = McpFieldAt($"mcpf:{k}:k{i}", $"k{i}", pair.Key, f.Remote ? "Authorization" : "API_KEY", f.PairBad(pair.Key, pair.Value), false, ix, cy, kw);
            McpFieldAt($"mcpf:{k}:v{i}", $"v{i}", pair.Value, "value", false, false, ix + kw + 6, cy, vw);
            MiniIcon("McpPairDel", i.ToString(), "close", Ink, ix + kw + 6 + vw + 6, cy + (fh - 28) / 2);
            cy += fh;
        }
        if (f.PairsProblem is { } pp)
        {
            var eh = pen.Wrapped(pp, ix, cy + 6, iw, F(11.5f, 400, RedInk));
            cy += 6 + eh;
        }
        var add = new PillSpec("Add one", "add", 10, 3, 12, 500, Ink, C(P.Fill), true);
        var (_, ah) = DrawPill(ix, cy + 6, 0, add, TK.Act, "McpPairAdd");
        cy += 6 + ah + 12;
        var nh = pen.Wrapped("Saved in Kiro's file as plain text, as Kiro keeps it.", ix, cy, iw, F(11.5f, 400, Dim));
        cy += nh + 12;
        var saving = Small(f.Editing is not null ? "Save" : "Add server") with { Fg = Rgba.Of(0x0c0b0e), Bg = Rgba.Of(0xf6f2ff) };
        var cancel = Small("Cancel");
        var (sw, bh) = PillSize(saving);
        var (cw, _) = PillSize(cancel);
        DrawPill(ix + iw - sw - 8 - cw, cy, 0, cancel, TK.Act, "McpCancel");
        DrawPill(ix + iw - sw, cy, 0, saving, TK.Act, "McpSave");
        return cy + bh + 14 - y;
    }

    string ServerLine(McpServer s) => s.Remote ? s.Url : string.Join(" ", new[] { s.Command }.Concat(s.Args));

    float McpRow(McpView m, McpServer srv, float x, float y, float width)
    {
        var on = !srv.Disabled;
        var warn = m.Warn(srv);
        var kind = srv.Remote ? "Remote" : "Local";
        var kw = TextW(kind, 11, 500) + 16;
        var tx = x + 12 + 24 + 2 + 8;
        var right = 8 + kw + 8 + 28 + 8 + 28 + 8 + SwitchW;
        var tw = x + width - 12 - right - tx;
        var colH = Tl(13) + 2 + 11 * MonoLH;
        if (warn != "") colH += 2 + pen.HeightWhenWrapped(warn, tw, F(12));
        var rh = Math.Max(46, 18 + Math.Max(colH, 28));
        float Mid(float ch) => y + 9 + (rh - 18 - ch) / 2;
        DrawTile(x + 12, Mid(24), srv.Remote ? "globe" : "terminal", srv.Remote ? Tint.Blue : Tint.Gray);
        var ty = Mid(colH);
        Opacity(on ? 1 : 0.55f, () =>
        {
            pen.Text(srv.Name, tx, ty, F(13), tw);
            pen.Text(ServerLine(srv), tx, ty + Tl(13) + 2, Mono(11, Dim), tw);
        });
        if (warn != "") pen.Wrapped(warn, tx, ty + Tl(13) + 2 + 11 * MonoLH + 2, tw, F(12, 400, AmberInk));
        var rx = x + width - 12 - right + 8;
        Box(rx, Mid(20), kw, 20, 10, A(Ink, 0.05f));
        pen.TextIn(kind, new Rect4(rx, Mid(20), kw, 20), F(11, 500, Dim));
        rx += kw + 8;
        MiniIcon("McpEdit", srv.Name, "rename", Ink, rx, Mid(28));
        rx += 28 + 8;
        MiniIcon("McpRemove", srv.Name, "delete", RedInk, rx, Mid(28));
        rx += 28 + 8;
        DrawSwitch(rx, Mid(SwitchH), on, true, "Mcp:" + srv.Name);
        return rh;
    }

    /// <summary>Remove asks first, and says the Kiro IDE uses the same list.</summary>
    float McpAsk(string name, float x, float y, float width)
    {
        var cancel = Small("Cancel");
        var remove = Small("Remove") with { Bg = redFill, Fg = White };
        var (cw, bh) = PillSize(cancel);
        var (rw, _) = PillSize(remove);
        var text = "Remove " + name + " from Kiro? The Kiro IDE uses this list too.";
        var tw = width - 24 - 10 - cw - 10 - rw;
        var th = pen.HeightWhenWrapped(text, tw, F(12.5f));
        var rh = 24 + Math.Max(th, bh);
        Box(x, y, width, rh, 0, A(C(P.Red), 0.07f));
        pen.Wrapped(text, x + 12, y + (rh - th) / 2, tw, F(12.5f, 400, Dim));
        DrawPill(x + 12 + tw + 10, y + (rh - bh) / 2, 0, cancel, TK.Act, "McpRemoveNo");
        DrawPill(x + 12 + tw + 10 + cw + 10, y + (rh - bh) / 2, 0, remove, TK.Act, "McpRemoveYes", 0, name);
        return rh;
    }

    /// <summary>The MCP section: its heading and count, the card, its line and the file's link.</summary>
    float Mcp(McpView m, float x, float y, float width)
    {
        var title = m.Title;
        var cut = title.IndexOf(" · ", StringComparison.Ordinal);
        var head = cut >= 0 ? title[..cut] : title;
        var count = cut >= 0 ? title[(cut + 3)..] : "";
        var hw = pen.Text(head.ToUpperInvariant(), x + 12, y + 10, F(10.5f, 600, Dim));
        if (count != "") pen.Text("· " + count, x + 12 + hw + 6, y + 10, F(10.5f, 400, Dim));
        var top = y + 10 + Tl(10.5f) + 6;
        var editing = m.Form?.Editing ?? "";
        var host = -1;
        for (var i = 0; i < m.Servers.Count; i++) if (editing != "" && m.Servers[i].Name == editing) host = i;
        var adding = m.Form is not null && host < 0;
        var showAdd = m.Error is null && !adding;

        // The card's height comes from drawing it once on a canvas that draws nothing, then the background goes under it.
        float Body()
        {
            var cy = top;
            if (m.Error is { } err)
            {
                var eh = pen.Wrapped(err, x + 12, cy + 12, width - 24, F(12.5f, 400, AmberInk));
                cy += 24 + eh;
            }
            for (var i = 0; i < m.Servers.Count; i++)
            {
                if (i > 0)
                {
                    Box(x + 12, cy, width - 12, 1, 0, C(P.Separator));
                    cy++;
                }
                if (m.Confirm is { } c && c == m.Servers[i].Name) cy += McpAsk(m.Servers[i].Name, x, cy, width);
                else if (host == i && m.Form is { } form) cy += McpForm(form, x, cy, width);
                else cy += McpRow(m, m.Servers[i], x, cy, width);
            }
            if (m.Servers.Count > 0 && (adding || showAdd))
            {
                Box(x + 12, cy, width - 12, 1, 0, C(P.Separator));
                cy++;
            }
            if (adding && m.Form is { } nf) cy += McpForm(nf, x, cy, width);
            if (showAdd)
            {
                if (Hot(TK.Act, "McpAdd")) Box(x, cy, width, 46, 0, C(P.RowHover));
                Icon("add", x + 14, cy + 15, 16, Dim);
                pen.TextIn("Add MCP server", new Rect4(x + 14 + 16 + 10, cy, 300, 46), F(13, 400, Dim), left: true);
                Add(new Rect4(x, cy, width, 46), TK.Act, "McpAdd");
                cy += 46;
            }
            if (m.Notice is { } note)
            {
                var nh = pen.Wrapped(note, x + 12, cy + 12, width - 24, F(12, 400, RedInk));
                cy += 24 + nh;
            }
            return cy - top;
        }
        var cardH = Measure(Body);
        Box(x, top, width, cardH, 12, C(P.Surface));
        Clipped(new Rect4(x, top, width, cardH), () => Body());
        var ny = top + cardH + 6;
        ny += Sub(McpView.Note, x + 12, ny, width - 24);
        if (m.HasFile)
        {
            var spec = new PillSpec("Open mcp.json in your editor", "", 6, 3, 12, 500, LinkInk, Rgba.Clear, true);
            var (_, bh) = DrawPill(x + 6, ny, 0, spec, TK.Act, "McpOpen");
            ny += bh;
        }
        return ny - y + 6;
    }
}
