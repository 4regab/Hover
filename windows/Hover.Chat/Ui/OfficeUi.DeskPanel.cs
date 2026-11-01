using Hover.Chat.Desk;
using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

public sealed partial class OfficeUi
{
    // The desk panel (desk.slint, internal/ui/deskpanel.go and deskrows.go): the surfaces as tabs along the top, and the one shown.

    float deskScroll, tabOff, listW = 640, lastTotal, lastListH;
    bool deskFocusPending;
    Laid? panelLaid;
    (int Ver, string Tab, int W, int Id, bool Prefs)? laidSig;
    ChatThread? docThread;
    readonly ThreadPainter docPainter = new();
    Rect4 deskListRect;
    (float X, float Y) docOrigin;
    int formOwner = -1;

    /// <summary>Where the panel is in the last frame, and the list inside it.</summary>
    public Rect4 DeskPanelRect { get; private set; }

    public Rect4 DeskListRect => deskListRect;

    /// <summary>The panel's tabs as the last frame drew them (surface ids).</summary>
    public IReadOnlyList<string> DeskTabs { get; private set; } = [];

    /// <summary>How many rows the open tab has.</summary>
    public int DeskRowCount => panelLaid?.Rows.Count ?? 0;

    ChatThread Doc() => docThread ??= new ChatThread(pen.Sh, cache, "Kiro");

    static readonly Rgba Ok = Rgba.Of(0x5de37a), Bad = Rgba.Of(0xff7b72), Amber = Rgba.Of(0xffc46b);

    float Tv(string text, float x, float y, float h, Look look, float maxW = 0)
    {
        if (text.Length == 0) return 0;
        return pen.Text(text, x, y + (h - pen.LineHeight(look)) / 2, look, maxW);
    }

    static Rgba Tone(int t) => t switch
    {
        1 => Ok,
        2 => Bad,
        3 => Amber,
        4 => Rgba.Of(0x7cb7ff),
        5 => Rgba.Of(0xc4a2ff),
        6 => Pen.Dim,
        7 => Pen.Faint,
        _ => Pen.Ink,
    };

    // MARK: The body of a tab

    bool DocWaiting;

    /// <summary>The tab's list, from the data it has (Reading… until the first read is back).</summary>
    Laid DeskBody(SessionInfo s, string tab, float w)
    {
        var id = s.Id;
        var p = Prefs(id);
        var bot = Bots.Name(s.Bot);
        var mono = DeskRows.Cols(w, 48, DeskRows.CharMono);
        Laid Failed(string e) => Laid.Nothing("diff", "Couldn’t read that", e);
        switch (tab)
        {
            case "terminal":
                {
                    var cols = DeskRows.Cols(w, 42, DeskRows.CharTerm);
                    if (p.TermTab == 1) return DeskRows.TerminalRows(Got(id, "terminal"), cols, bot);
                    return DeskRows.MineRows(Terminal(id).Entries, cols);
                }
            case "files":
                {
                    if (p.HasFile)
                    {
                        var f = Got(id, "file");
                        if (!f.Has || deskGotArg.GetValueOrDefault((id, "file")) != p.File) return Laid.Wait;
                        string? md = null;
                        var mdH = 0f;
                        if (f["text"].StrOrNull() is { } text && DeskRows.IsMd(p.File) && p.FileMode == 0 && !p.Editing)
                        {
                            md = text.Replace("\r\n", "\n");
                            if (md.Length > 8000) md = md[..8000];
                            DocWaiting = Doc().SetDocument(md, Math.Max(w - 18, 120));
                            mdH = Doc().Height;
                        }
                        return DeskRows.FileRows(f, md, mdH);
                    }
                    var files = Got(id, "files");
                    if (!files.Has) return Laid.Wait;
                    if (files["error"].Has) return Failed(files["error"].Str());
                    if (p.Find.Length > 0) return DeskRows.FindRows(files["tree"].Strings(), p.Find);
                    return DeskRows.FilesRows(files, p.Open, p.Saved);
                }
            case "diff":
                {
                    var diff = Got(id, "diff");
                    if (!diff.Has) return Laid.Wait;
                    if (diff["error"].Has) return Laid.Nothing("diff", "Couldn’t read the diff", diff["error"].Str());
                    if (diff["files"].Count == 0) return Laid.Nothing("diff", "No changes", diff["git"].Bool() ? "The working tree matches the last commit." : "Nothing edited yet.");
                    var prTile = Tiles(s).FirstOrDefault(t => t.Id == "pr");
                    var prOk = prTile is { Enabled: true } && Got(id, "probe") is { Has: true } probe && !probe["pr"].IsObject;
                    return DeskRows.DiffRows(diff, p.DiffOpen, prOk);
                }
            case "pr":
                {
                    var pr = Got(id, "pr");
                    if (!pr.Has) return Laid.Wait;
                    if (pr["number"].Has)
                    {
                        float? mdH = null;
                        var body = pr["body"].Str().Trim();
                        if (body.Length > 0)
                        {
                            if (body.Length > 8000) body = body[..8000];
                            DocWaiting = Doc().SetDocument(body, Math.Max(w - 18, 120));
                            mdH = Doc().Height;
                        }
                        return DeskRows.PrRows(pr, w, mdH);
                    }
                    if (pr["error"].Has && !pr["setup"].Has && !pr["none"].Has) return Laid.Nothing("pr", "No pull request", pr["error"].Str());
                    return new Laid([]);
                }
            case "linked":
                {
                    var l = Got(id, "linked");
                    if (!l.Has) return Laid.Wait;
                    if (l["prs"].Count == 0) return Laid.Nothing("linked", "No linked pull requests", "Pull requests this session mentions show here.");
                    return DeskRows.LinkedRows(l);
                }
            case "agents":
                {
                    var a = Got(id, "agents");
                    if (!a.Has) return Laid.Wait;
                    if (a["agents"].Count == 0) return Laid.Nothing("agents", "No subagents", "Work this session hands to subagents shows here.");
                    return DeskRows.AgentRows(a, p.AgentOpen, mono);
                }
        }
        return new Laid([]);
    }

    // MARK: The panel

    void PaintDeskPanel(Rect4 r, float radius, bool attached)
    {
        if (deskPanel is not { } id || DeskSession(id) is not { } s) return;
        DeskPanelRect = r;
        var p = Prefs(id);
        var tiles = Tiles(s);
        if (!attached)
            for (var i = 0; i < 4; i++) pen.Box(r.X - i * 3, r.Y + 12 - i, r.W + i * 6, r.H + i * 3, radius + i * 3, Rgba.Of(0x000000, 0.1));
        pen.Box(r, radius, Rgba.Of(0x131116, 0.985));
        if (!attached) pen.Border(r.X, r.Y, r.W, r.H, radius, 1, Rgba.Of(0xffffff, 0.09));
        else pen.Box(r.X, r.Y, 1, r.H, 0, Rgba.Of(0xffffff, 0.06));
        Block(r);
        Add(r, "deskPanel");
        pen.Cv.PushClip(r);
        PaintTabs(r, s, tiles);
        var by = r.Y + 52;
        var bh = r.H - 52;
        // The list's width, for the rows' wrapping.
        var lw = deskTab == "terminal" ? r.W : r.W - 10 - 4;
        if (Math.Abs(lw - listW) > 8)
        {
            listW = lw;
            deskVersion++;
        }
        var sig = (deskVersion, deskTab, (int)listW, id, p.Editing);
        if (laidSig != sig || panelLaid is null)
        {
            laidSig = sig;
            var before = deskTab;
            panelLaid = DeskBody(s, deskTab, listW);
            if (before != deskTab) deskScroll = 0;
        }
        var pr = Got(id, "pr");
        switch (deskTab)
        {
            case "terminal":
                PaintTerminal(s, new Rect4(r.X, by, r.W, bh));
                break;
            case "screen":
                PaintScreen(s, new Rect4(r.X + 10, by, r.W - 20, bh - 12));
                break;
            case "pr" when pr["setup"].Has:
                PaintGhSetup(s, new Rect4(r.X + 10, by, r.W - 20, bh - 12));
                break;
            case "pr" when pr["none"].Bool():
                PaintPrForm(s, new Rect4(r.X + 10, by, r.W - 10, bh - 12));
                break;
            default:
                PaintLists(s, new Rect4(r.X, by, r.W, bh), p);
                break;
        }
        if (deskMenuOpen) PaintOpenMenu(r);
        pen.Cv.PopClip();
        if (deskFocusPending && deskTab == "terminal" && fields.TryGetValue("term", out var tf) && tf.Layout is not null)
        {
            deskFocusPending = false;
            FocusField(tf, false);
        }
    }

    void PaintTabs(Rect4 r, SessionInfo s, List<DeskTile> tiles)
    {
        var shown = new List<DeskTile>();
        foreach (var tid in DeskTiles.TabOrder)
        {
            var t = tiles.FirstOrDefault(x => x.Id == tid);
            if (t is null) continue;
            if (t.Enabled || tid == deskTab || DeskTiles.EmptyTab(tid)) shown.Add(t);
        }
        DeskTabs = [.. shown.Select(t => t.Id)];
        var tf = Pen.F(12.5, 400, Pen.Ink);
        var avail = r.W - 10 - 6 - 28 - 10;
        var widths = new List<float>();
        float total = 0;
        foreach (var t in shown)
        {
            var title = t.Id == "linked" ? "Linked PRs" : t.Title;
            var tw = 9 + 15 + 6 + pen.Width(title, tf) + 9;
            if (t.Badge > 0) tw += 6 + Math.Max(14, pen.Width(t.Badge.ToString(), Pen.F(9, 600, Pen.Ink)) + 6);
            widths.Add(tw);
            total += tw + 2;
        }
        total = Math.Max(total - 2, 0);
        float left = 0;
        for (var i = 0; i < shown.Count; i++)
        {
            if (shown[i].Id == deskTab)
            {
                if (left < tabOff) tabOff = left;
                else if (left + widths[i] > tabOff + avail) tabOff = left + widths[i] - avail;
            }
            left += widths[i] + 2;
        }
        tabOff = Math.Clamp(tabOff, 0, Math.Max(0, total - avail));
        pen.Cv.PushClip(new Rect4(r.X + 10, r.Y, avail, 52));
        var cx = r.X + 10 - tabOff;
        for (var i = 0; i < shown.Count; i++)
        {
            var t = shown[i];
            var on = t.Id == deskTab;
            var tr = new Rect4(cx, r.Y + 11, widths[i], 30);
            var hov = Over(tr);
            if (on) pen.Box(tr, 7, Rgba.Of(0xffffff, 0.08));
            else if (hov) pen.Box(tr, 7, Rgba.Of(0xffffff, 0.05));
            var col = on ? Pen.Ink : hov ? Pen.Dim : Pen.Faint;
            pen.Icon(DeskIcons.Of(t.Id), cx + 9, r.Y + 11 + 7.5f, 15, col);
            var title = t.Id == "linked" ? "Linked PRs" : t.Title;
            var tx = cx + 9 + 15 + 6;
            pen.Text(title, tx, r.Y + 11 + (30 - pen.LineHeight(tf)) / 2, tf with { Color = col });
            tx += pen.Width(title, tf) + 6;
            if (t.Badge > 0)
            {
                var bw = Math.Max(14, pen.Width(t.Badge.ToString(), Pen.F(9, 600, Pen.Ink)) + 6);
                pen.Box(tx, r.Y + 11 + 8, bw, 14, 7, Rgba.Of(0x3b82f6));
                pen.TextIn(t.Badge.ToString(), new Rect4(tx, r.Y + 11 + 8, bw, 14), Pen.F(9, 600, Rgba.Of(0xffffff)));
            }
            Add(tr, "deskTab", i, t.Id);
            cx += widths[i] + 2;
        }
        pen.Cv.PopClip();
        var close = new Rect4(r.Right - 10 - 28, r.Y + 12, 28, 28);
        if (Over(close)) pen.Box(close, 8, Rgba.Of(0xffffff, 0.08));
        pen.Icon(DeskIcons.Close, close.X + 7, close.Y + 7, 14, Pen.Dim);
        Add(close, "deskClose");
        pen.Box(r.X, r.Y + 51, r.W, 1, 0, Rgba.Of(0xffffff, 0.06));
    }

    // MARK: Lists

    void PaintLists(SessionInfo s, Rect4 area, DeskPrefs p)
    {
        var files = deskTab == "files";
        float lp = 10;
        if (files)
        {
            lp = 0;
            if (p.HasFile && p.FileMode == 0 && DeskRows.IsMd(p.File)) lp = 14;
        }
        var rp = files ? 0f : 4f;
        float lx = area.X + lp, lw = area.W - lp - rp;
        float ly = area.Y, lh = area.H - 12;
        if (files && p.HasFile)
        {
            var fh = PaintFileBar(s, p, new Rect4(area.X + (p.HasFile ? 0 : lp), ly, area.W, 44));
            ly += fh;
            lh -= fh;
            if (p.Editing && Busy(s))
            {
                var wh = PaintFileWarning(Bots.Name(s.Bot) + " is working in this folder. If it changes this file too, saving keeps your version.", area.X, ly, area.W);
                ly += wh;
                lh -= wh;
            }
        }
        if (files && !p.HasFile)
        {
            // The search box.
            var bx = area.X + 12;
            var by = ly + 6;
            var bw = area.W - 24;
            pen.Box(bx, by, bw, 32, 8, Rgba.Of(0xffffff, 0.03));
            var foc = focus == Focus.Desk && deskField?.Key == "find";
            pen.Border(bx, by, bw, 32, 8, 1, foc ? Rgba.Of(0xc4a2ff, 0.3) : Rgba.Of(0xffffff, 0.06));
            pen.Icon(DeskIcons.Search, bx + 10, by + 9, 14, Pen.Faint);
            var f = PaintField("find", false, new Rect4(bx + 32, by + (32 - pen.LineHeight(Pen.F(12.5, 400, Pen.Ink))) / 2, bw - 42, 18), Pen.F(12.5, 400, Pen.Ink), "Find a file");
            if (f.Box.Text != p.Find)
            {
                p.Find = f.Box.Text;
                deskScroll = 0;
                DeskChanges();
            }
            ly += 44;
            lh -= 44;
        }
        if (files && p.HasFile && p.Editing)
        {
            PaintFileEditor(new Rect4(area.X, ly, area.W, lh + 12));
            return;
        }
        var laid = panelLaid;
        if (laid is not null && laid.Rows.Count > 0) PaintDeskList(laid, new Rect4(lx, ly, lw, lh), false);
        else PaintEmpty(laid, new Rect4(lx, ly, lw, lh));
    }

    void PaintEmpty(Laid? laid, Rect4 r)
    {
        deskListRect = r;
        if (laid is null || laid.Loading)
        {
            pen.TextIn("Reading…", r, Pen.F(12, 400, Pen.Faint));
            return;
        }
        var e = laid.Empty;
        if (e is null) return;
        var tf = Pen.F(13, 600, Pen.Dim);
        var bf = Pen.F(12, 400, Pen.Faint);
        var th = pen.HeightWhenWrapped(e.Title, r.W - 40, tf);
        var bh = e.Text.Length > 0 ? pen.HeightWhenWrapped(e.Text, Math.Min(340, r.W - 40), bf) : 0;
        var total = 26 + 6 + th + (bh > 0 ? 6 + bh : 0);
        var cy = r.Y + (r.H - total) / 2;
        pen.Icon(DeskIcons.Of(e.Icon), r.X + (r.W - 26) / 2, cy, 26, Pen.Faint);
        cy += 26 + 6;
        var tw = pen.Shape(e.Title, tf, r.W - 40).Layout.Width;
        pen.Wrapped(e.Title, r.X + (r.W - tw) / 2, cy, r.W - 40, tf);
        cy += th + 6;
        if (bh > 0)
        {
            var bw = Math.Min(340, r.W - 40);
            var shaped = pen.Shape(e.Text, bf, bw);
            pen.Wrapped(e.Text, r.X + (r.W - shaped.Layout.Width) / 2, cy, bw, bf);
        }
    }

    /// <summary>The rows in view, each at its own y in a viewport as tall as all of them, and a thin bar beside it.</summary>
    void PaintDeskList(Laid laid, Rect4 area, bool follow)
    {
        deskListRect = area;
        var end = Math.Max(0, laid.Total - area.H);
        if (follow && (laid.Total != lastTotal || area.H != lastListH)) deskScroll = end;
        lastTotal = laid.Total;
        lastListH = area.H;
        deskScroll = Math.Clamp(deskScroll, 0, end);
        pen.Cv.PushClip(area);
        var (from, to) = laid.Window(deskScroll, area.H);
        for (var i = from; i < to; i++)
        {
            var ry = area.Y + laid.Ys[i] - deskScroll;
            if (ry + laid.Rows[i].H < area.Y || ry > area.Bottom) continue;
            PaintRow(laid.Rows[i], i, area.X, ry, area.W - 10);
        }
        if (laid.Total > area.H + 1)
        {
            var th = Math.Max(20, area.H * (area.H / Math.Max(1, laid.Total)));
            var ty = area.Y + (area.H - th) * (deskScroll / Math.Max(1, laid.Total - area.H));
            pen.Box(area.Right - 4, ty, 3, th, 1.5f, Rgba.Of(0xffffff, 0.22));
        }
        pen.Cv.PopClip();
    }

    /// <summary>The wheel over the list.</summary>
    bool DeskWheel(float x, float y, float delta)
    {
        if (deskPanel is null) return false;
        if (!DeskPanelRect.Contains(x, y)) return false;
        if (deskListRect.Contains(x, y) || deskTab is "pr" or "terminal")
        {
            var max = deskTab == "pr" && Got(deskPanel.Value, "pr")["none"].Bool() ? Math.Max(0, prContent - prView) : Math.Max(0, (panelLaid?.Total ?? 0) - deskListRect.H);
            deskScroll = Math.Clamp(deskScroll - delta, 0, max);
            Dirty = true;
        }
        return true;
    }

    float prContent, prView;

    // MARK: A row

    static readonly Rgba RowHover = Rgba.Of(0xffffff, 0.05);

    void PaintRow(DRow r, int idx, float x, float y, float w)
    {
        var h = r.H;
        var mono11 = Pen.Mono(11, Pen.Ink);
        var mono115 = Pen.Mono(11.5, Pen.Ink);
        var mono125 = Pen.Mono(12.5, Pen.Ink);
        var rect = new Rect4(x, y, w, h);
        switch (r.Kind)
        {
            case 1:
                {
                    var f = Pen.F(10.5, 600, Pen.Faint);
                    var tw = Tv(r.Text, x + 6, y + 14, h - 14, f);
                    if (r.Right.Length > 0) Tv(r.Right, x + 6 + tw + 6, y + 14, h - 14, Pen.F(10.5, 400, Pen.Faint));
                    break;
                }
            case 2:
                pen.Box(x + 4, y, w - 8, h - 8, 8, Amber with { A = 20 });
                pen.Wrapped(r.Text, x + 14, y + 7, w - 28, Pen.F(11.5, 400, Rgba.Of(0xffd59a)));
                break;
            case 3:
                pen.Wrapped(r.Text, x + 6, y + (h - pen.HeightWhenWrapped(r.Text, w - 12, Pen.F(12, 400, Pen.Faint))) / 2, w - 12, Pen.F(12, 400, Pen.Faint));
                break;
            case 4:
                {
                    if (r.Act.Length > 0) Add(rect, "drow", idx, r.Act);
                    if (Over(rect)) pen.Box(rect, 8, RowHover);
                    var cx = x + 8;
                    pen.Icon(DeskIcons.File, cx, y + (h - 14) / 2, 14, Pen.Faint);
                    cx += 14 + 8;
                    var rx = x + w - 8;
                    var nf = Pen.F(12.5, 500, r.Flag == 4 ? Pen.Dim : Pen.Ink);
                    var nw = Math.Min(pen.Width(r.Text, nf), rx - cx);
                    Tv(r.Text, cx, y, h, nf, nw);
                    if (r.Sub.Length > 0 && rx - cx - nw - 7 > 0) Tv(r.Sub, cx + nw + 7, y, h, mono11 with { Color = Pen.Faint }, rx - cx - nw - 7);
                    break;
                }
            case 5:
                {
                    if (r.Act.Length > 0) Add(rect, "drow", idx, r.Act);
                    var hov = Over(rect);
                    if (hov) pen.Box(rect, 8, RowHover);
                    var dir = r.Act.StartsWith("dir:", StringComparison.Ordinal);
                    var cx = x + 12 + r.Depth * 16;
                    if (dir) pen.Icon(r.Flag % 2 == 1 ? DeskIcons.ChevronDown : DeskIcons.ChevronRight, cx, y + (h - 12) / 2, 12, Pen.Faint);
                    cx += 12 + 8;
                    pen.Icon(dir ? DeskIcons.Folder : DeskIcons.File, cx, y + (h - 14) / 2, 14, Pen.Faint);
                    cx += 14 + 8;
                    var rx = x + w - 12;
                    if (r.Badge.Length > 0)
                    {
                        rx -= 17 + 8;
                        StatusBadge(r.Badge, rx + 8, y + (h - 17) / 2);
                    }
                    var col = r.Flag >= 2 ? Amber : hov ? Pen.Ink : Pen.Dim;
                    Tv(r.Text, cx, y, h, Pen.F(12.5, 400, col), Math.Max(rx - cx, 0));
                    break;
                }
            case 7:
                {
                    pen.Box(rect, 0, Rgba.Of(0x0b0a0d));
                    var hov = Over(rect);
                    Tv(r.Num, x, y, h, mono115 with { Color = Rgba.Of(0xffffff, 0.22) }, 0);
                    var nw = pen.Width(r.Num, mono115);
                    if (nw < 44) { }
                    Tv(r.Text, x + 56, y, h, mono115 with { Color = Rgba.Of(0xd6d3dc) }, 0);
                    if (r.Act.Length > 0 && hov) OpenLineButton(r.Act, x + w - 86, y, h, false);
                    break;
                }
            case 8:
                {
                    if (r.Act.Length > 0) Add(rect, "drow", idx, r.Act);
                    pen.Box(x, y, w, h, 10, Rgba.Of(0xffffff, Over(rect) ? 0.07 : 0.035));
                    var cx = x + 10;
                    pen.Icon(r.Flag == 1 ? DeskIcons.ChevronDown : DeskIcons.ChevronRight, cx, y + (h - 12) / 2, 12, Pen.Faint);
                    cx += 12 + 8;
                    StatusBadge(r.Badge, cx, y + (h - 17) / 2);
                    cx += 17 + 8;
                    var rx = x + w - 10;
                    if (r.Tag2.Length > 0)
                    {
                        var bw = DeskButton("attachDiff", "Attach", DeskIcons.Add, rx, y + (h - 26) / 2, true, false, idx, r.Tag2, rightAligned: true);
                        rx -= bw + 8;
                    }
                    if (r.Del.Length > 0) { var dw = pen.Width(r.Del, mono11); rx -= dw; Tv(r.Del, rx, y, h, mono11 with { Color = Bad }); rx -= 8; }
                    if (r.Add.Length > 0) { var aw = pen.Width(r.Add, mono11); rx -= aw; Tv(r.Add, rx, y, h, mono11 with { Color = Ok }); rx -= 8; }
                    Tv(r.Text, cx, y, h, Pen.Mono(12, Pen.Ink), Math.Max(rx - cx, 0));
                    break;
                }
            case 9:
                pen.Box(rect, 0, Rgba.Of(0x7cb7ff, 0.07));
                Tv(r.Text, x + 10, y, h, mono115 with { Color = Rgba.Of(0x8fb6ff) }, w - 20);
                break;
            case 10:
                {
                    var bg = r.Tone == 1 ? Rgba.Of(0x2ea043, 0.15) : r.Tone == 2 ? Rgba.Of(0xf85149, 0.13) : Rgba.Of(0x0f0e11);
                    pen.Box(rect, 0, bg);
                    var hov = Over(rect);
                    var nl = mono11 with { Color = Rgba.Of(0xffffff, 0.22) };
                    if (r.Num.Length > 0) Tv(r.Num, x + 38 - pen.Width(r.Num, nl), y, h, nl);
                    if (r.Num2.Length > 0) Tv(r.Num2, x + 40 + 38 - pen.Width(r.Num2, nl), y, h, nl);
                    var col = r.Tone == 1 ? Rgba.Of(0xaff5b4) : r.Tone == 2 ? Rgba.Of(0xffc1bc) : Rgba.Of(0xd6d3dc);
                    Tv(r.Text, x + 88, y, h, mono115 with { Color = col });
                    if (r.Act.Length > 0 && hov) OpenLineButton(r.Act, x + w - 86, y, h, true);
                    break;
                }
            case 11:
                pen.Box(rect, 0, Rgba.Of(0x0f0e11));
                Tv(r.Text, x + 10, y, h, Pen.F(11.5, 400, Pen.Faint), w - 20);
                break;
            case 27:
                {
                    var pw = PromptWidth(r.Sub, mono125);
                    Tv(r.Sub, x + 16, y, h, mono125 with { Color = Rgba.Of(0xf6f2ff, 0.88) });
                    var rf = Pen.F(11.5, 400, Tone(r.Tone));
                    var rw = pen.Width(r.Right, rf);
                    Tv(r.Text, x + 16 + pw, y, h, mono125 with { Color = Rgba.Of(0xf6f2ff, 0.88) }, Math.Max(w - 32 - pw - rw, 0));
                    if (r.Flag != 1) Tv(r.Right, x + w - 16 - rw, y, h, rf);
                    else
                    {
                        var k = (float)Math.Abs(((now % 1600) / 1600.0) * 2 - 1);
                        Tv(r.Right, x + w - 16 - rw, y, h, rf with { Color = Mix(Pen.Faint with { A = 255 }, Rgba.Of(0xffffff), k) });
                        Need(100);
                    }
                    break;
                }
            case 28:
                {
                    var col = r.Tone switch { 2 => Rgba.Of(0xff9a92), 7 => Pen.Faint, _ => Rgba.Of(0xf6f2ff, 0.7) };
                    Tv(r.Text, x + 16, y, h, mono125 with { Color = col });
                    break;
                }
            case 13:
                pen.Box(rect, 0, Rgba.Of(0x0b0a0d));
                Tv(r.Text, x + 10, y, h, mono115 with { Color = r.Tone == 7 ? Rgba.Of(0xffffff, 0.3) : Rgba.Of(0xd6d3dc) });
                break;
            case 14:
                {
                    if (r.Act.Length > 0) Add(rect, "drow", idx, r.Act);
                    pen.Box(rect, 10, Rgba.Of(0xffffff, Over(rect) ? 0.06 : 0.03));
                    var cx = x + 10;
                    pen.Box(cx, y + (h - 8) / 2, 8, 8, 4, Tone(r.Tone));
                    cx += 8 + 10;
                    var rx = x + w - 10;
                    pen.Icon(r.Flag == 1 ? DeskIcons.ChevronDown : DeskIcons.ChevronRight, rx - 12, y + (h - 12) / 2, 12, Pen.Faint);
                    rx -= 12 + 10;
                    var rw = pen.Width(r.Right, mono11);
                    Tv(r.Right, rx - rw, y, h, mono11 with { Color = Pen.Faint });
                    rx -= rw + 10;
                    var tf = Pen.F(13, 500, Pen.Ink);
                    var sf = Pen.F(11.5, 400, Pen.Faint);
                    var colH = pen.LineHeight(tf) + 2 + pen.LineHeight(sf);
                    var ty = y + (h - colH) / 2;
                    pen.Text(r.Text, cx, ty, tf, Math.Max(rx - cx, 0));
                    pen.Text(r.Sub, cx, ty + pen.LineHeight(tf) + 2, sf, Math.Max(rx - cx, 0));
                    break;
                }
            case 15:
                {
                    var f = Pen.F(10.5, 600, Pen.Faint);
                    pen.Text(r.Text, x + 28, y + h - pen.LineHeight(f) - 2, f);
                    break;
                }
            case 16:
                {
                    var (fg, bg) = PrTone(r.Tone);
                    var pw = Pill(r.Text, fg, bg, x + 4, y + (h - 22) / 2);
                    Tv(r.Sub, x + 4 + pw + 8, y, h, Pen.Mono(12, Pen.Faint));
                    break;
                }
            case 17:
                pen.Wrapped(r.Text, x + 4, y + 4, w - 8, Pen.F(15, 600, Pen.Ink));
                break;
            case 18:
                pen.Wrapped(r.Text, x + 4, y + 3, w - 8, Pen.F(11.5, 400, Pen.Dim));
                break;
            case 19:
                DeskButton("drowBtn", r.Text, DeskIcons.External, x + 4, y + (h - 26) / 2, false, false, idx, r.Act);
                break;
            case 20:
                {
                    var cx = x + 4;
                    foreach (var (text, col, weight) in new[] { ("Checks", Pen.Ink, 600), (r.Del, Bad, 400), (r.Sub, Amber, 400), (r.Add, Ok, 400), (r.Tag1, Pen.Dim, 400) })
                    {
                        if (text.Length == 0) continue;
                        cx += Tv(text, cx, y, h, Pen.F(12, weight, col)) + 12;
                    }
                    break;
                }
            case 21:
                {
                    if (r.Act.Length > 0) { Add(rect, "drow", idx, r.Act); if (Over(rect)) pen.Box(rect, 7, RowHover); }
                    pen.Box(x + 8, y + (h - 8) / 2, 8, 8, 4, Tone(r.Tone));
                    Tv(r.Text, x + 8 + 8 + 8, y, h, Pen.F(12, 400, Rgba.Of(0xdcd8e4)), w - 32);
                    break;
                }
            case 22:
                Tv(r.Text, x + 6, y, h, Pen.F(13, 400, Rgba.Of(0xececf0)));
                break;
            case 25:
                PaintDocRow(r, x, y, w, h);
                break;
            case 23:
                {
                    if (r.Act.Length > 0) Add(rect, "drow", idx, r.Act);
                    if (Over(rect)) pen.Box(rect, 10, RowHover);
                    var cx = x + 10;
                    if (r.Badge.Length > 0)
                    {
                        var (fg, bg) = PrTone(r.Tone);
                        cx += Pill(r.Badge, fg, bg, cx, y + (h - 22) / 2) + 10;
                    }
                    var rx = x + w - 10;
                    pen.Icon(DeskIcons.External, rx - 13, y + (h - 13) / 2, 13, Pen.Faint);
                    rx -= 13 + 10;
                    foreach (var (text, col) in new[] { (r.Del, Bad), (r.Add, Ok) })
                    {
                        if (text.Length == 0) continue;
                        rx -= pen.Width(text, mono11);
                        Tv(text, rx, y, h, mono11 with { Color = col });
                        rx -= 10;
                    }
                    var tf = Pen.F(13, 500, Pen.Ink);
                    var sf = Pen.F(11.5, 400, Pen.Faint);
                    var ty = y + (h - (pen.LineHeight(tf) + 2 + pen.LineHeight(sf))) / 2;
                    pen.Text(r.Text, cx, ty, tf, Math.Max(rx - cx, 0));
                    pen.Text(r.Sub, cx, ty + pen.LineHeight(tf) + 2, sf, Math.Max(rx - cx, 0));
                    break;
                }
            case 24:
                {
                    var cx = x + 4;
                    cx += Tv(r.Text, cx, y, h, Pen.F(12, 600, Pen.Ink)) + 8;
                    foreach (var (text, col) in new[] { (r.Add, Ok), (r.Del, Bad) })
                        if (text.Length > 0) cx += Tv(text, cx, y, h, mono11 with { Color = col }) + 8;
                    if (r.Sub.Length > 0)
                    {
                        var f = Pen.Mono(10.5, Rgba.Of(0xf6f2ff, 0.6));
                        var bw = pen.Width(r.Sub, f) + 16;
                        pen.Box(cx, y + (h - 19) / 2, bw, 19, 6, Rgba.Of(0xffffff, 0.055));
                        pen.TextIn(r.Sub, new Rect4(cx, y + (h - 19) / 2, bw, 19), f);
                    }
                    if (r.Act.Length > 0) DeskButton("drowBtn", "Create PR", null, x + w - 6, y + (h - 26) / 2, false, true, idx, r.Act, rightAligned: true);
                    break;
                }
        }
    }

    static (Rgba Fg, Rgba Bg) PrTone(int t) => t switch
    {
        1 => (Ok, Rgba.Of(0x2ea043, 0.2)),
        5 => (Rgba.Of(0xc9a8ff), Rgba.Of(0xa371f7, 0.2)),
        2 => (Bad, Rgba.Of(0xf85149, 0.19)),
        _ => (Rgba.Of(0xc9c5d1), Rgba.Of(0xffffff, 0.1)),
    };

    float Pill(string text, Rgba fg, Rgba bg, float x, float y)
    {
        var f = Pen.F(11.5, 600, fg);
        var w = pen.Width(text, f) + 20;
        pen.Box(x, y, w, 22, 11, bg);
        pen.TextIn(text, new Rect4(x, y, w, 22), f);
        return w;
    }

    void StatusBadge(string letter, float x, float y)
    {
        var col = letter switch { "A" or "U" => Ok, "D" => Bad, "R" => Rgba.Of(0x7cb7ff), _ => Amber };
        pen.Box(x, y, 17, 17, 5, col with { A = 38 });
        pen.TextIn(letter, new Rect4(x, y, 17, 17), Pen.F(10, 700, col));
    }

    void OpenLineButton(string act, float x, float y, float h, bool diff)
    {
        var r = new Rect4(x, y + 1, 80, h - 2);
        var hov = Over(r);
        pen.Box(r, 6, Rgba.Of(0xffffff, diff ? (hov ? 0.19 : 0.11) : (hov ? 0.15 : 0.08)));
        pen.TextIn("Open ↗", r, Pen.F(10.5, 400, Rgba.Of(0xf6f2ff, 0.77)));
        Add(r, "drowBtn", 0, act);
    }

    /// <summary>A small bordered button (DButton). Returns its width; it keeps DButton's empty 24 px at the right.</summary>
    float DeskButton(string id, string text, string? icon, float x, float y, bool small, bool primary, int n, string s, bool disabled = false, bool rightAligned = false)
    {
        var look = Pen.F(small ? 11.5 : 12, 600, primary ? Rgba.Of(0x0b0a0f) : Pen.Ink);
        var tw = pen.Width(text, look);
        var rw = tw + 24 + (icon is not null ? 13 + 7 : 0);
        var w = rw + 24;
        var hgt = small ? 26f : 30f;
        if (rightAligned) x -= w;
        if (small && !primary && id == "drowBtn") hgt = 26;
        var r = new Rect4(x, y, w, hgt);
        var hov = Over(r) && !disabled;
        var bg = primary ? (hov ? Rgba.Of(0xffffff) : Rgba.Of(0xf5f5f7)) : Rgba.Of(0xffffff, hov ? 0.12 : 0.06);
        if (disabled) pen.Cv.PushOpacity(0.45f);
        pen.Box(r, 9, bg);
        if (!primary) pen.Border(x, y, w, hgt, 9, 1, Rgba.Of(0xffffff, 0.12));
        var cx = x + 12;
        if (icon is not null)
        {
            pen.Icon(icon, cx, y + (hgt - 13) / 2, 13, look.Color);
            cx += 13 + 7;
        }
        Tv(text, cx, y, hgt, look);
        if (disabled) pen.Cv.PopOpacity();
        if (!disabled) Add(r, id, n, s);
        return w;
    }

    void PaintDocRow(DRow r, float x, float y, float w, float h)
    {
        var doc = Doc();
        docOrigin = (x - Palette.ThreadPad[3] + 4, y);
        pen.Cv.PushClip(new Rect4(x, y, w + 8, h));
        pen.Cv.PushMove(docOrigin.X, docOrigin.Y);
        docPainter.Paint(doc, pen.Cv, 0, h);
        pen.Cv.PopMove();
        pen.Cv.PopClip();
        Add(new Rect4(x, y, w, h), "docRow", 0, "");
    }

    /// <summary>A click in a Markdown document of the panel: a link opens, a code block's Copy copies.</summary>
    void DocClick(float x, float y)
    {
        var hit = Doc().Hit(x - docOrigin.X, y - docOrigin.Y);
        switch (hit.Kind)
        {
            case HitKind.Link:
                if (hit.Link.StartsWith("http://", StringComparison.OrdinalIgnoreCase) || hit.Link.StartsWith("https://", StringComparison.OrdinalIgnoreCase)) env.OpenUrl?.Invoke(hit.Link);
                else OpenRelative(hit.Link);
                break;
            case HitKind.Act when hit.Act.Kind == ActKind.Copy:
                env.Clipboard.Set(hit.Act.Text);
                Say("Copied.");
                break;
        }
    }

    /// <summary>A link in a Markdown file that names another file of the folder (AGENTS.md, docs/a.md#top) opens that file here.</summary>
    void OpenRelative(string url)
    {
        if (deskPanel is not { } id) return;
        var p = Prefs(id);
        if (!p.HasFile) return;
        var link = url;
        var cut = link.IndexOfAny(['#', '?']);
        if (cut >= 0) link = link[..cut];
        if (link.Length == 0 || link.Contains(':')) return;
        var dir = DeskRows.DirOf(p.File);
        var joined = link.TrimStart('/');
        if (dir.Length > 0 && !link.StartsWith('/')) joined = dir + "/" + link;
        var parts = new List<string>();
        foreach (var part in joined.Split('/'))
        {
            if (part is "" or ".") continue;
            if (part == "..")
            {
                if (parts.Count == 0) return;
                parts.RemoveAt(parts.Count - 1);
            }
            else parts.Add(part);
        }
        DeskAct("file:" + string.Join('/', parts));
    }

    // MARK: The open file

    float PaintFileBar(SessionInfo s, DeskPrefs p, Rect4 r)
    {
        var f = Got(s.Id, "file");
        var md = DeskRows.IsMd(p.File);
        var fileOk = f.Has && deskGotArg.GetValueOrDefault((s.Id, "file")) == p.File;
        var can = fileOk && f["text"].Has && !f["truncated"].Bool() && !f["text"].Str().Contains('\uFFFD');
        var editing = p.Editing;
        var cx = r.X + (editing ? 16 : 8);
        var rx = r.Right - 12;
        var y = r.Y;
        if (editing)
        {
            var sw = DeskButton("fSave", p.Saving ? "Saving…" : "Save  Ctrl+S", null, rx, y + 9, true, true, 0, "", p.Saving, true);
            rx -= sw + 8;
            var cw = DeskButton("fCancel", "Cancel", null, rx, y + 9, true, false, 0, "", false, true);
            rx -= cw + 8;
            var ef = Pen.F(11.5, 500, Amber);
            var ew = pen.Width("Editing", ef);
            rx -= ew;
            Tv("Editing", rx, y, 44, ef);
            rx -= 8;
        }
        else
        {
            var pw = 7f + 4 + 12 + 6 + (editorChoices.Count > 0 ? 15 + 4 : 0);
            rx -= pw;
            var pr = new Rect4(rx, y + 9, pw, 26);
            if (deskMenuOpen) pen.Box(pr, 8, Rgba.Of(0xffffff, 0.1));
            else if (Over(pr)) pen.Box(pr, 8, Rgba.Of(0xffffff, 0.08));
            var ix = rx + 7;
            if (editorChoices.Count > 0)
            {
                EditorMark(editorChoices[0], ix, y + (44 - 15) / 2);
                ix += 15 + 4;
            }
            pen.Icon(DeskIcons.ChevronDown, ix, y + (44 - 12) / 2, 12, Pen.Faint);
            Add(pr, "openPill");
            rx -= 8;
            var eb = new Rect4(rx - 28, y + 8, 28, 28);
            pen.Cv.PushOpacity(can ? 1 : 0.3f);
            if (Over(eb) && can) pen.Box(eb, 8, Rgba.Of(0xffffff, 0.08));
            pen.Icon(DeskIcons.Rename, eb.X + 7, eb.Y + 7, 14, Pen.Dim);
            pen.Cv.PopOpacity();
            if (can) Add(eb, "fEdit");
            rx -= 28 + 8;
            if (md)
            {
                var seg = Segment("Preview", "Markdown", p.FileMode, rx, y + 9);
                rx -= seg + 8;
            }
            var bb = new Rect4(cx, y + 8, 28, 28);
            if (Over(bb)) pen.Box(bb, 8, Rgba.Of(0xffffff, 0.08));
            pen.Icon(DeskIcons.Back, bb.X + 7, bb.Y + 7, 14, Pen.Dim);
            Add(bb, "fileBack");
            cx += 28 + 8;
        }
        Tv(p.File, cx, y, 44, Pen.Mono(12, Pen.Ink), Math.Max(rx - cx, 0));
        pen.Box(r.X, y + 43, r.W, 1, 0, Rgba.Of(0xffffff, 0.06));
        return 44;
    }

    float Segment(string a, string b, int value, float rx, float y)
    {
        var f = Pen.F(11.5, 400, Pen.Ink);
        var wa = pen.Width(a, f) + 14;
        var wb = pen.Width(b, f) + 14;
        var total = wa + wb + 4;
        var x = rx - total;
        pen.Box(x, y, total, 26, 7, Rgba.Of(0xffffff, 0.03));
        var cx = x + 2;
        for (var i = 0; i < 2; i++)
        {
            var cw = i == 0 ? wa : wb;
            var cell = new Rect4(cx, y + 2, cw, 22);
            if (value == i) pen.Box(cell, 5, Rgba.Of(0xffffff, 0.09));
            var col = value == i ? Pen.Ink : Over(cell) ? Pen.Dim : Pen.Faint;
            pen.TextIn(i == 0 ? a : b, cell, f with { Color = col });
            Add(cell, "fMode", i);
            cx += cw;
        }
        return total;
    }

    void EditorMark(EditorChoice c, float x, float y)
    {
        if (c.Id == "fm")
        {
            pen.Icon(DeskIcons.Folder, x, y, 14, Pen.Dim);
            return;
        }
        var (letter, tint) = c.Id switch
        {
            "vscode" => ("V", 0x0e7fd6u), "cursor" => ("C", 0x2a2a30u), "kiro" => ("K", 0x9046ffu), "zed" => ("Z", 0x2a2a30u),
            _ => (c.Name.Length > 0 ? c.Name[..1].ToUpperInvariant() : "?", 0x3a3744u),
        };
        pen.Box(x, y, 15, 15, 4, Rgba.Of(tint));
        pen.Border(x, y, 15, 15, 4, 1, Rgba.Of(0xffffff, 0.12));
        pen.TextIn(letter, new Rect4(x, y, 15, 15), Pen.F(9, 700, Rgba.Of(0xffffff)));
    }

    float PaintFileWarning(string text, float x, float y, float w)
    {
        var f = Pen.F(12, 400, Amber);
        var th = pen.HeightWhenWrapped(text, w - 32 - 14 - 8, f);
        var h = th + 16;
        pen.Box(x, y, w, h, 0, Amber with { A = 13 });
        pen.Icon(DeskIcons.Warning, x + 16, y + (h - 14) / 2, 14, Amber);
        pen.Wrapped(text, x + 16 + 14 + 8, y + 8, w - 32 - 14 - 8, f);
        pen.Box(x, y + h - 1, w, 1, 0, Rgba.Of(0xffffff, 0.06));
        return h;
    }

    void PaintFileEditor(Rect4 r)
    {
        pen.Box(r, 0, Rgba.Of(0x08070a));
        pen.Cv.PushClip(r);
        var area = new Rect4(r.X + 16, r.Y + 10, r.W - 32, r.H - 10);
        PaintField("fileEdit", true, area, Pen.Mono(12, Pen.Ink), "");
        pen.Cv.PopClip();
        deskListRect = r;
    }

    void PaintOpenMenu(Rect4 r)
    {
        var away = new Rect4(r.X, r.Y, r.W, r.H);
        Add(away, "openAway");
        var choices = editorChoices;
        var mh = 12f + 24;
        for (var i = 0; i < choices.Count; i++)
        {
            if (choices[i].Id == "fm" && i > 0) mh += 9;
            mh += 30;
        }
        var mx = r.Right - 214;
        var my = r.Y + 52 + 44 - 6;
        pen.Box(mx, my, 204, mh, 10, Rgba.Of(0x1b181e));
        pen.Border(mx, my, 204, mh, 10, 1, Rgba.Of(0xffffff, 0.1));
        Block(new Rect4(mx, my, 204, mh));
        Add(new Rect4(mx, my, 204, mh), "openMenuBox");
        Tv("Open file in", mx + 14, my + 6, 24, Pen.F(11, 400, Pen.Faint));
        var cy = my + 6 + 24;
        for (var i = 0; i < choices.Count; i++)
        {
            var c = choices[i];
            if (c.Id == "fm" && i > 0)
            {
                pen.Box(mx + 6, cy + 4, 192, 1, 0, Rgba.Of(0xffffff, 0.06));
                cy += 9;
            }
            var row = new Rect4(mx + 6, cy, 192, 30);
            var hov = Over(row);
            if (hov) pen.Box(row, 7, Rgba.Of(0xffffff, 0.05));
            EditorMark(c, mx + 14, cy + 7.5f);
            Tv(c.Name, mx + 6 + 8 + 15 + 10, cy, 30, Pen.F(13, 400, hov ? Pen.Ink : Pen.Dim));
            if (c.Last)
            {
                var lf = Pen.F(11, 400, Pen.Faint);
                Tv("Last used", mx + 204 - 14 - pen.Width("Last used", lf), cy, 30, lf);
            }
            Add(row, "openIn", i, c.Id);
            cy += 30;
        }
    }

    // MARK: The terminal

    void PaintTerminal(SessionInfo s, Rect4 r)
    {
        var p = Prefs(s.Id);
        pen.Box(r, 0, Rgba.Of(0x08070a));
        var mine = p.TermTab == 0;
        var tx = r.X + 8;
        var live = false;
        foreach (var c in Got(s.Id, "terminal")["commands"].Items()) live |= c["status"].Str() == "in_progress";
        for (var i = 0; i < 2; i++)
        {
            var label = i == 0 ? "My commands" : Bots.Name(s.Bot);
            var f = Pen.F(12, 400, Pen.Ink);
            var tw = pen.Width(label, f);
            var liveDot = i == 1 && live;
            var bw = 10 + tw + 10 + (liveDot ? 12 : 0);
            var tr = new Rect4(tx, r.Y + 6, bw, 26);
            var on = (i == 0) == mine;
            if (on) pen.Box(tr, 6, Rgba.Of(0xffffff, 0.06));
            else if (Over(tr)) pen.Box(tr, 6, Rgba.Of(0xffffff, 0.04));
            Tv(label, tx + 10, r.Y + 6, 26, f with { Color = on ? Pen.Ink : Over(tr) ? Pen.Dim : Pen.Faint });
            if (liveDot) pen.Box(tx + 10 + tw + 6, r.Y + 6 + 10, 6, 6, 3, Rgba.Of(0x9b6bff));
            Add(tr, "termTab", i);
            tx += bw + 2;
        }
        pen.Box(r.X, r.Y + 37, r.W, 1, 0, Rgba.Of(0xffffff, 0.06));
        var ay = r.Y + 38;
        var ah = r.H - 38;
        const float ph = 24;
        var laid = panelLaid ?? new Laid([]);
        var listH = mine ? Math.Max(0, Math.Min(laid.Total, ah - ph - 8)) : ah;
        PaintDeskList(laid, new Rect4(r.X, ay, r.W, listH), true);
        var term = Terminal(s.Id);
        if (!mine || term.Running) return;
        var py = ay + listH;
        var pf = Pen.Mono(12.5, Rgba.Of(0xf6f2ff, 0.88));
        var prompt = DeskRows.Prompt(term.Cwd.Length > 0 ? term.Cwd : s.Folder);
        var pw = PromptWidth(prompt, pf);
        Tv(prompt, r.X + 16, py, ph, pf);
        var f2 = PaintField("term", false, new Rect4(r.X + 16 + pw, py + (ph - pen.LineHeight(pf)) / 2, r.W - 32 - pw, pen.LineHeight(pf)), pf, "");
        _ = f2;
        // The box under the last line is where the next command is typed: a click on the list reaches it too.
        Add(new Rect4(r.X, py, r.W, ph), "termBox");
    }

    /// <summary>The width of a prompt including the space after its ">", which a text's own width leaves out.</summary>
    float PromptWidth(string prompt, Look look) => pen.Width(prompt + "|", look) - pen.Width("|", look);
}
