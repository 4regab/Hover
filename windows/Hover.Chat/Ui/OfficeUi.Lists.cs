using System.Globalization;
using Hover.Chat.Desk;
using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

public sealed partial class OfficeUi
{
    // The side panels over the office (the board, the overview, the history) and the Kiro Web and Kiro CLI sessions the
    // history and the session list show beside Hover's own (internal/ui/officepanel.go, internal/shell/panels.go).

    /// <summary>The panel over the office now: board, tv or history; empty for none.</summary>
    public string OfficePanel { get; private set; } = "";

    /// <summary>The office has a panel open or closed: its name, or empty. The office moves its camera to that prop.</summary>
    public event Action<string>? OfficePanelChanged;

    sealed class WebList
    {
        public bool Listing, HasErr;
        public List<ListedSession> List = [];
        public string Err = "", Note = "", Opening = "";
        public bool Asked;
    }

    sealed class CliList
    {
        public bool Listing;
        public List<ListedSession> List = [];
        public string Opening = "";
        public double At = -1e9;
    }

    readonly WebList web = new();
    readonly CliList cli = new();
    float panelScroll;
    List<PanelRow> panelRows = [];

    public IReadOnlyList<ListedSession> WebSessions => web.List;

    public IReadOnlyList<ListedSession> CliSessions => cli.List;

    /// <summary>Opens the board, the overview or the history over the office (name "" for none).</summary>
    public void OpenOfficePanel(string name)
    {
        if (name.Length > 0)
        {
            DeskLeave();
            if (OpenId is not null || openKey is not null) CloseChat();
            taskOpen = false;
            ChatView = false;
            panelScroll = 0;
        }
        OfficePanel = name;
        if (name == "history")
        {
            AskWeb();
            AskCli(true);
        }
        OfficePanelChanged?.Invoke(name);
        Dirty = true;
    }

    void AskWeb()
    {
        if (web.Listing) return;
        web.Listing = true;
        env.Send(new WebSessionsCommand());
    }

    void AskCli(bool force)
    {
        if (cli.Listing || (!force && now - cli.At < 120_000)) return;
        cli.Listing = true;
        cli.At = now;
        env.Send(new CliSessionsCommand());
    }

    /// <summary>The session list asks Kiro for this computer's sessions when it shows, at most every two minutes.</summary>
    void ListsTick()
    {
        if (ChatView && !sideClosed && w >= 760 && state is not null) AskCli(false);
    }

    bool ListsAccept(BackendEvent e)
    {
        switch (e)
        {
            case WebSessionsEvent w:
                web.Listing = false;
                web.Asked = true;
                // Another account's list never stays: a failure shows none.
                if (w.Ok)
                {
                    web.List = w.Sessions;
                    web.Note = w.Note ?? "";
                    web.Err = "";
                    web.HasErr = false;
                }
                else
                {
                    web.List = [];
                    web.Note = "";
                    web.Err = w.Error ?? "Kiro could not list them.";
                    web.HasErr = true;
                }
                return true;
            case CliSessionsEvent c:
                cli.Listing = false;
                // A failure keeps the last list: it is still what this computer has.
                if (c.Ok) cli.List = c.Sessions;
                return true;
            case AdoptWebEvent a:
                web.Opening = "";
                Adopted(a, web.List);
                return true;
            case AdoptCliEvent a:
                cli.Opening = "";
                Adopted(a, cli.List);
                return true;
        }
        return false;
    }

    void Adopted(AdoptEvent a, List<ListedSession> list)
    {
        if (!a.Ok)
        {
            Say(a.Error ?? "Could not open it.");
            return;
        }
        list.RemoveAll(x => x.Id == a.Id);
        if (a.SessionId is { } sid)
        {
            OfficePanel = "";
            OfficePanelChanged?.Invoke("");
            OpenChat(sid);
        }
    }

    // MARK: Rows

    sealed record PanelRow(int Kind)
    {
        public string Text { get; init; } = "";
        public string Sub { get; init; } = "";
        public string Meta { get; init; } = "";
        public string Count { get; init; } = "";
        public Rgba Color { get; init; }
        public int Stage { get; init; }
        public float Pct { get; init; }
        public string S1 { get; init; } = "";
        public string S2 { get; init; } = "";
        public string S3 { get; init; } = "";
        public string S4 { get; init; } = "";
        /// <summary>1 a Kiro Web session, 2 a Kiro CLI one, 0 Hover's own.</summary>
        public int Open { get; init; }
        public bool Desk { get; init; }
        public int? SessionId { get; init; }
        public string Key { get; init; } = "";
    }

    static string AgoLong(double ms)
    {
        var m = (long)Math.Round(ms / 60e3, MidpointRounding.AwayFromZero);
        if (m < 1) return "now";
        if (m < 60) return $"{m} min ago";
        if (m < 24 * 60) return $"{(long)Math.Round(m / 60.0, MidpointRounding.AwayFromZero)} h ago";
        return $"{(long)Math.Round(m / 1440.0, MidpointRounding.AwayFromZero)} d ago";
    }

    static readonly string[] Months = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

    static double LocalOffsetMs(double ms) => TimeZoneInfo.Local.GetUtcOffset(DateTimeOffset.FromUnixTimeMilliseconds((long)ms)).TotalMilliseconds;

    /// <summary>Today, Yesterday, This week, else the month and year.</summary>
    public static string DayWord(double nowMs, double ms)
    {
        var off = LocalOffsetMs(nowMs);
        var today = Math.Floor((nowMs + off) / 864e5) * 864e5 - off;
        var k = (today + 864e5 - ms) / 864e5;
        if (ms >= today) return "Today";
        if (k < 2) return "Yesterday";
        if (k < 7) return "This week";
        var d = DateTimeOffset.FromUnixTimeMilliseconds((long)ms).ToOffset(TimeSpan.FromMilliseconds(LocalOffsetMs(ms)));
        return Months[d.Month - 1] + " " + d.Year.ToString(CultureInfo.InvariantCulture);
    }

    /// <summary>A history row's date: the time today and yesterday, the weekday this week, the date before that.</summary>
    public static string StampWord(double nowMs, double ms)
    {
        long Day(double t) => (long)Math.Floor((t + LocalOffsetMs(t)) / 864e5);
        long d = Day(ms), n = Day(nowMs);
        var k = n - d;
        var local = DateTimeOffset.FromUnixTimeMilliseconds((long)ms).ToOffset(TimeSpan.FromMilliseconds(LocalOffsetMs(ms)));
        if (k < 2) return Turns.Hm(ms, (long)(LocalOffsetMs(ms) / 60e3));
        if (k < 7) return local.DayOfWeek.ToString()[..3];
        var m = Months[local.Month - 1][..3];
        return local.Year == DateTimeOffset.FromUnixTimeMilliseconds((long)nowMs).ToOffset(TimeSpan.FromMilliseconds(LocalOffsetMs(nowMs))).Year
            ? $"{m} {local.Day}" : $"{m} {local.Day}, {local.Year}";
    }

    double NowMs => new DateTimeOffset(DateTime.SpecifyKind(env.Utc(), DateTimeKind.Utc)).ToUnixTimeMilliseconds();

    (string Title, string Sub, List<PanelRow> Rows) BuildPanel()
    {
        var sessions = state?.Sessions ?? [];
        var rows = new List<PanelRow>();
        var nowMs = NowMs;
        switch (OfficePanel)
        {
            case "board":
                foreach (var (head, color, stages) in new[]
                {
                    ("Waking", 0xf5b83du, new[] { 0 }),
                    ("Doing", 0x9b6bffu, new[] { 1, 5 }),
                    ("Finished", 0x2fae66u, new[] { 2, 3, 4 }),
                })
                {
                    var list = sessions.Where(x => stages.Contains(StageNumber(x.Stage))).ToList();
                    rows.Add(new PanelRow(0) { Text = head.ToUpperInvariant(), Color = Rgba.Of(color), Count = list.Count.ToString(CultureInfo.InvariantCulture) });
                    foreach (var x in list)
                    {
                        var stage = StageNumber(x.Stage);
                        var turn = x.Turns.LastOrDefault(t => !t.Queued);
                        var steps = turn?.Steps.Count ?? 0;
                        var started = turn is { T0: > 0 } ? turn.T0 : nowMs;
                        var meta = AgoLong(nowMs - started);
                        if (steps > 0) meta += $" · {steps} step{(steps == 1 ? "" : "s")}";
                        rows.Add(new PanelRow(1)
                        {
                            Sub = $"{Bots.Name(x.Bot)} · {ToolName(x.Tool)} · {StageWord(stage)}",
                            Text = x.Title.Length > 0 ? x.Title : "New chat", Meta = meta, Color = Bots.Color(x.Bot), Stage = stage, SessionId = x.Id,
                        });
                    }
                    if (list.Count == 0) rows.Add(new PanelRow(4) { Text = "Nobody here" });
                }
                return ("Session board", $"{sessions.Count} of 6 desks in use", rows);
            case "tv":
                {
                    string N(params int[] st) => sessions.Count(x => st.Contains(StageNumber(x.Stage))).ToString(CultureInfo.InvariantCulture);
                    rows.Add(new PanelRow(2) { S1 = N(0, 1, 5), S2 = N(2), S3 = N(3), S4 = N(4) });
                    rows.Add(new PanelRow(0) { Text = "CONTEXT USED" });
                    foreach (var x in sessions)
                    {
                        rows.Add(new PanelRow(3)
                        {
                            Sub = Bots.Name(x.Bot), Text = x.Title.Length > 0 ? x.Title : "New chat", Color = Bots.Color(x.Bot), Stage = -1, SessionId = x.Id,
                            Count = x.Ctx is { } c ? c + "%" : "—", Pct = x.Ctx ?? 0,
                        });
                    }
                    if (sessions.Count == 0) rows.Add(new PanelRow(4) { Text = "No sessions yet. Press + to give an agent a task." });
                    return ("Office overview", "Up to 3 tasks run at once, across Kiro, Codex, Cursor, OpenCode, Claude Code and Antigravity", rows);
                }
            case "history":
                {
                    var find = Fld("histFind", false).Box.Text.ToLowerInvariant();
                    var history = state?.History ?? [];
                    var all = history.Where(h => find.Length == 0 || (h.Title + " " + h.Folder + " " + h.Tool).Contains(find, StringComparison.OrdinalIgnoreCase)).ToList();
                    var haveKiro = sessions.Select(KiroIdOf).Where(k => k.Length > 0).ToHashSet();
                    var clouds = web.List.Where(c => !haveKiro.Contains(c.Id) && (find.Length == 0 || (c.Title + " kiro web").Contains(find, StringComparison.OrdinalIgnoreCase))).ToList();
                    var clis = cli.List.Where(c => !haveKiro.Contains(c.Id) && (find.Length == 0 || (c.Title + " " + c.Folder + " kiro cli").Contains(find, StringComparison.OrdinalIgnoreCase))).ToList();
                    var items = new List<(double Ms, HistoryRow? Here, ListedSession? Web, ListedSession? Cli)>();
                    items.AddRange(all.Select(h => ((double)h.At, (HistoryRow?)h, (ListedSession?)null, (ListedSession?)null)));
                    items.AddRange(clis.Select(c => ((double)(c.Updated ?? 0), (HistoryRow?)null, (ListedSession?)null, (ListedSession?)c)));
                    items.AddRange(clouds.Select(c => ((double)(c.Updated ?? 0), (HistoryRow?)null, (ListedSession?)c, (ListedSession?)null)));
                    items = [.. items.OrderByDescending(i => i.Ms)];
                    // Why there are no Kiro Web sessions, in words at the top, not in the small print.
                    if (web.HasErr) rows.Add(new PanelRow(4) { Text = "Kiro Web sessions couldn’t be listed. " + web.Err });
                    else if (!web.Listing && web.List.Count == 0 && web.Note.Length > 0) rows.Add(new PanelRow(4) { Text = "No Kiro Web sessions to add. " + web.Note });
                    var at = "";
                    foreach (var it in items)
                    {
                        var d = it.Ms > 0 ? DayWord(nowMs, it.Ms) : "Earlier";
                        if (d != at)
                        {
                            at = d;
                            rows.Add(new PanelRow(0) { Text = d.ToUpperInvariant() });
                        }
                        if (it.Web is { } wc)
                        {
                            rows.Add(new PanelRow(6)
                            {
                                Sub = "kiro", Text = wc.Title.Length > 0 ? wc.Title : "Kiro Web session", Meta = web.Opening == wc.Id ? "Opening…" : "Not opened yet",
                                S1 = it.Ms > 0 ? StampWord(nowMs, it.Ms) : "", Count = "click to read it", Stage = 4, Open = 1, Key = "web:" + wc.Id,
                            });
                            continue;
                        }
                        if (it.Cli is { } cc)
                        {
                            rows.Add(new PanelRow(6)
                            {
                                Sub = "kiro", Text = cc.Title.Length > 0 ? cc.Title : "Kiro CLI session", Meta = cli.Opening == cc.Id ? "Opening…" : "Not opened yet",
                                S1 = it.Ms > 0 ? StampWord(nowMs, it.Ms) : "", Count = LastPart(cc.Folder ?? ""), Stage = 4, Open = 2, Key = "cli:" + cc.Id,
                            });
                            continue;
                        }
                        var h = it.Here!;
                        var live = sessions.FirstOrDefault(x => x.Key == h.Key);
                        var stage = StageNumber(h.Stage);
                        // The saved entry holds the last finished turn's state; a session at a desk that is running says what it is doing.
                        if (live is not null && Busy(live)) stage = StageNumber(live.Stage);
                        rows.Add(new PanelRow(6)
                        {
                            Sub = h.Tool, Text = h.Title.Length > 0 ? h.Title : "New chat", Meta = StageWord(stage), S1 = StampWord(nowMs, it.Ms),
                            Count = LastPart(h.Folder), Stage = stage, Key = h.Key, Desk = live is not null, SessionId = live?.Id,
                        });
                    }
                    if (items.Count == 0)
                        rows.Add(new PanelRow(4) { Text = history.Count == 0 && web.List.Count == 0 ? "Sessions you start are kept here. Open one to read it, reply to carry on." : "Nothing matches." });
                    var sub = $"{history.Count} session{(history.Count == 1 ? "" : "s")}, kept until you delete them";
                    if (web.Listing) sub += " · looking up Kiro Web…";
                    return ("Session history", sub, rows);
                }
        }
        return ("", "", rows);
    }

    static string KiroIdOf(SessionInfo s) => s.Extra is { } ex && ex.TryGetValue("kiroId", out var v) && v.ValueKind == System.Text.Json.JsonValueKind.String ? v.GetString() ?? "" : "";

    // MARK: Painting the panel

    float PanelRowH(PanelRow r, float lw)
    {
        switch (r.Kind)
        {
            case 0: return 6 + pen.LineHeight(Pen.F(12, 600, Pen.Ink)) + 8;
            case 1: case 3: return CardParts(r, lw).CardH + 6;
            case 2: return 10 + pen.LineHeight(Pen.F(22, 600, Pen.Ink)) + pen.LineHeight(Pen.F(11, 400, Pen.Ink)) + 10 + 16;
            case 4: return pen.WrappedHeight(r.Text, lw, Pen.F(12.5, 400, Pen.Ink));
            case 6: return 48;
        }
        return 0;
    }

    readonly record struct CardPart(float TitleW, float ColW, float SubH, float TitleH, float MetaH, float ColH, float CardH, float CountW, int Lines);

    CardPart CardParts(PanelRow r, float lw)
    {
        var subF = Pen.F(11.5, 600, Pen.Ink);
        var titleF = Pen.F(12.5, 400, Pen.Ink);
        var metaF = Pen.F(11, 400, Pen.Ink);
        var titleW = lw - 51;
        float countW = 0;
        if (r.Kind == 3)
        {
            titleW -= 44;
            countW = pen.Width(r.Count, metaF);
        }
        var colW = lw - 10 - 9 - 22 - 10;
        if (r.Kind == 3) colW -= 10 + countW;
        var lh = pen.LineHeight(titleF);
        var maxLines = Math.Max(1, (int)(34 / lh));
        var subH = pen.LineHeight(subF);
        var (cut, th) = CutLines(r.Text, titleF, titleW, maxLines);
        _ = cut;
        var colH = subH + 1 + th;
        if (r.Kind == 3) colH += 1 + 5;
        float metaH = 0;
        if (r.Meta.Length > 0)
        {
            metaH = pen.LineHeight(metaF);
            colH += 1 + metaH;
        }
        return new CardPart(titleW, colW, subH, th, metaH, colH, 9 + Math.Max(colH, 20) + 9, countW, maxLines);
    }

    static Rgba StageDot(int stage) => stage switch
    {
        2 => Rgba.Of(0x30d158),
        3 => Rgba.Of(0xff453a),
        5 => Rgba.Of(0xffb340),
        <= 1 => Rgba.Of(0xb48cff),
        _ => Rgba.Of(0x8e8e93),
    };

    /// <summary>The side panel (office.slint, "A side panel"): a header (the title, a note, Close), the history's find box, and the rows in a list that scrolls.</summary>
    void PaintOfficePanel(bool compact)
    {
        if (OfficePanel.Length == 0) return;
        var gap = compact ? 8f : 12f;
        var sw = Math.Min(compact ? 360f : 400f, w - 2 * gap);
        float x = w - sw - gap, y = gap, pw = sw, ph = h - 2 * gap;
        var rad = compact ? 16f : 20f;
        var (title, sub, rows) = BuildPanel();
        panelRows = rows;
        for (var i = 0; i < 4; i++) pen.Box(x - i * 3, y + 12 - i, pw + i * 6, ph + i * 3, rad + i * 3, Rgba.Of(0x000000, 0.1));
        pen.Box(x, y, pw, ph, rad, Rgba.Of(0x14101a, 0.97));
        pen.Border(x, y, pw, ph, rad, 1, Rgba.Of(0xffffff, 0.09));
        var area = new Rect4(x, y, pw, ph);
        Block(area);
        Add(area, "officePanel");
        pen.Cv.PushClip(area);
        var tf = Pen.F(14, 600, Pen.Ink);
        var sf = Pen.F(11.5, 400, Pen.Faint);
        var xs = compact ? 26f : 32f;
        float th = pen.LineHeight(tf), sh = pen.LineHeight(sf);
        var content = Math.Max(th + sh, xs);
        var tw = pw - 14 - 10 - xs;
        var ty = y + 12 + (content - th - sh) / 2;
        pen.Text(title, x + 14, ty, tf, tw);
        pen.Text(sub, x + 14, ty + th, sf, tw);
        var close = new Rect4(x + pw - 10 - xs, y + 12 + (content - xs) / 2, xs, xs);
        if (Over(close)) pen.Box(close, 8, Rgba.Of(0xffffff, 0.08));
        pen.Icon(DeskIcons.Close, close.X + (xs - 14) / 2, close.Y + (xs - 14) / 2, 14, Pen.Dim);
        Add(close, "panelClose");
        var ly = y + 12 + content + 11;
        pen.Box(x, ly, pw, 1, 0, Rgba.Of(0xffffff, 0.06));
        ly++;
        if (OfficePanel == "history")
        {
            float bx = x + 12, by = ly + 12, bw = pw - 24;
            var foc = focus == Focus.Desk && deskField?.Key == "histFind";
            pen.Box(bx, by, bw, 34, 10, Rgba.Of(0x1b181e));
            pen.Border(bx, by, bw, 34, 10, 1, foc ? Rgba.Of(0xffffff, 0.22) : Rgba.Of(0xffffff, 0.07));
            pen.Icon(DeskIcons.Search, bx + 10, by + 10, 14, Pen.Faint);
            var fl = Pen.F(12.5, 400, Pen.Ink);
            PaintField("histFind", false, new Rect4(bx + 32, by + (34 - pen.LineHeight(fl)) / 2, bw - 42, pen.LineHeight(fl)), fl, "Find a session");
            ly = by + 34;
        }
        var lh = y + ph - ly;
        var lw = pw - 24;
        var hs = rows.Select(r => PanelRowH(r, lw)).ToList();
        var total = 14 + hs.Sum() + 12;
        panelScroll = Math.Clamp(panelScroll, 0, Math.Max(0, total - lh));
        pen.Cv.PushClip(new Rect4(x, ly, pw, lh));
        panelListRect = new Rect4(x, ly, pw, lh);
        var ry = ly + 14 - panelScroll;
        for (var i = 0; i < rows.Count; i++)
        {
            if (ry + hs[i] > ly && ry < ly + lh) PaintPanelRow(rows[i], i, x + 12, ry, lw);
            ry += hs[i];
        }
        pen.Cv.PopClip();
        pen.Cv.PopClip();
    }

    Rect4 panelListRect;

    void PaintPanelRow(PanelRow r, int idx, float x, float y, float lw)
    {
        switch (r.Kind)
        {
            case 0:
                {
                    var f = Pen.F(12, 600, Pen.Dim);
                    var lh = pen.LineHeight(f);
                    var tx = x + 4;
                    if (r.Color.A > 0)
                    {
                        pen.Box(x + 4, y + 6 + (lh - 8) / 2, 8, 8, 2, r.Color);
                        tx += 16;
                    }
                    pen.Text(r.Text, tx, y + 6, f);
                    var cw = pen.Width(r.Count, f);
                    pen.Text(r.Count, x + lw - 4 - cw, y + 6, f with { Color = Pen.Faint });
                    break;
                }
            case 1:
            case 3:
                {
                    var q = CardParts(r, lw);
                    var rect = new Rect4(x, y, lw, q.CardH);
                    var hov = Over(rect);
                    pen.Box(rect, 12, Rgba.Of(0xffffff, hov ? 0.09 : 0.04));
                    pen.Border(x, y, lw, q.CardH, 12, 1, hov ? Rgba.Of(0xc4a2ff, 0.35) : Rgba.Of(0xffffff, 0.09));
                    BotFace(r.Color, x + 10, y + (q.CardH - 20) / 2);
                    var tx = x + 10 + 22 + 10;
                    var ty = y + 9 + (q.CardH - 18 - q.ColH) / 2;
                    var subCol = r.Stage == 2 ? Rgba.Of(0x30d158) : r.Stage == 3 ? Rgba.Of(0xff453a) : r.Stage <= 1 && r.Stage >= 0 ? Pen.Lilac : Pen.Dim;
                    pen.Text(r.Sub, tx, ty, Pen.F(11.5, 600, subCol), q.ColW);
                    ty += q.SubH + 1;
                    var titleF = Pen.F(12.5, 400, Pen.Ink);
                    var (cut, th) = CutLines(r.Text, titleF, q.TitleW, q.Lines);
                    pen.Wrapped(cut, tx, ty, q.TitleW, titleF);
                    ty += th;
                    if (r.Kind == 3)
                    {
                        ty += 1;
                        pen.Box(tx, ty, q.ColW, 5, 3, Rgba.Of(0xffffff, 0.1));
                        pen.Box(tx, ty, q.ColW * Math.Clamp(r.Pct / 100, 0, 1), 5, 3, Pen.Lilac);
                        ty += 5;
                    }
                    if (r.Meta.Length > 0) pen.Text(r.Meta, tx, ty + 1, Pen.F(11, 400, Pen.Faint), q.ColW);
                    if (r.Kind == 3)
                    {
                        var mf = Pen.F(11, 400, Pen.Faint);
                        Tv(r.Count, x + lw - 9 - q.CountW, y, q.CardH, mf);
                    }
                    if (r.SessionId is not null) Add(rect, "panelRow", idx);
                    break;
                }
            case 2:
                {
                    var bw = (lw - 3 * 8) / 4;
                    var numF = Pen.F(22, 600, Pen.Ink);
                    var lblF = Pen.F(11, 400, Pen.Dim);
                    var bh = 10 + pen.LineHeight(numF) + pen.LineHeight(lblF) + 10;
                    var cols = new[] { Pen.Lilac, Rgba.Of(0x30d158), Rgba.Of(0xff453a), Rgba.Of(0x8e8e93) };
                    var nums = new[] { r.S1, r.S2, r.S3, r.S4 };
                    var names = new[] { "Working", "Done", "Failed", "Stopped" };
                    for (var i = 0; i < 4; i++)
                    {
                        var bx = x + i * (bw + 8);
                        pen.Box(bx, y, bw, bh, 12, Rgba.Of(0xffffff, 0.04));
                        pen.Border(bx, y, bw, bh, 12, 1, Rgba.Of(0xffffff, 0.09));
                        pen.TextIn(nums[i], new Rect4(bx, y + 10, bw, pen.LineHeight(numF)), numF with { Color = cols[i] });
                        pen.TextIn(names[i], new Rect4(bx, y + 10 + pen.LineHeight(numF), bw, pen.LineHeight(lblF)), lblF);
                    }
                    break;
                }
            case 4:
                pen.Wrapped(r.Text, x, y, lw, Pen.F(12.5, 400, Pen.Faint));
                break;
            case 6:
                PaintHistoryRow(r, idx, x, y, lw);
                break;
        }
    }

    void BotFace(Rgba col, float x, float y)
    {
        pen.Box(x, y, 22, 20, 6, col);
        pen.Box(x + 4, y + 6, 14, 8, 3, Rgba.Of(0x121018));
        pen.Box(x + 4 + 3, y + 6 + 2, 2, 4, 0, Rgba.Of(0xaaf6ff));
        pen.Box(x + 4 + 14 - 5, y + 6 + 2, 2, 4, 0, Rgba.Of(0xaaf6ff));
    }

    void PaintHistoryRow(PanelRow r, int idx, float x, float y, float lw)
    {
        var card = new Rect4(x, y, lw, 48);
        var del = new Rect4(x + lw - 32, y + 5, 26, 24);
        var hov = Over(card) || Over(del);
        if (Over(card)) pen.Box(card, 10, Rgba.Of(0xffffff, 0.05));
        pen.Mark(r.Sub.Length > 0 ? r.Sub : "kiro", x + 8, y + (48 - 30) / 2, 30);
        var tf = Pen.F(13, 500, Pen.Ink);
        var mf = Pen.F(11.5, 400, Pen.Dim);
        var chipF = Pen.F(10.5, 500, Pen.Dim);
        var row1 = pen.LineHeight(tf);
        var row2 = Math.Max(17, pen.LineHeight(mf));
        var colH = row1 + 3 + row2;
        var cx = x + 8 + 30 + 11;
        var cr = x + lw - 10;
        var cy = y + 8 + (32 - colH) / 2;
        var dw = pen.Width(r.S1, mf);
        if (r.Open != 0 || !hov) pen.Text(r.S1, cr - dw, cy + (row1 - pen.LineHeight(mf)) / 2, mf with { Color = Pen.Faint });
        pen.Text(r.Text, cx, cy, tf, Math.Max(cr - dw - 8 - cx, 0));
        var ry = cy + row1 + 3;
        pen.Box(cx, ry + (row2 - 6) / 2, 6, 6, 3, StageDot(r.Stage));
        var ex = cx + 6 + 5;
        var mw = pen.Width(r.Meta, mf);
        pen.Text(r.Meta, ex, ry + (row2 - pen.LineHeight(mf)) / 2, mf);
        ex += mw + 5;
        float tail = 0, dkW = 0, wkW = 0;
        if (r.Desk) { dkW = pen.Width("At a desk", chipF) + 12; tail += 5 + dkW; }
        if (r.Open == 1) { wkW = pen.Width("Kiro Web", chipF) + 24; tail += 5 + wkW; }
        if (r.Open == 2) { wkW = pen.Width("CLI", chipF) + 12; tail += 5 + wkW; }
        pen.Text("· " + r.Count, ex, ry + (row2 - pen.LineHeight(mf)) / 2, mf with { Color = Pen.Faint }, Math.Max(cr - ex - tail, 0));
        var chx = cr - tail + 5;
        if (r.Desk)
        {
            pen.Box(chx, ry + (row2 - 17) / 2, dkW, 17, 5, Rgba.Of(0xb48cff, 0.14));
            pen.TextIn("At a desk", new Rect4(chx, ry + (row2 - 17) / 2, dkW, 17), chipF with { Color = Rgba.Of(0xd8c6ff) });
            chx += dkW + 5;
        }
        if (r.Open == 1)
        {
            var cyy = ry + (row2 - 17) / 2;
            pen.Box(chx, cyy, wkW, 17, 5, Rgba.Of(0x6ba8ff, 0.14));
            pen.Icon(DeskIcons.Cloud, chx + 5, cyy + 3, 11, Rgba.Of(0x8fbcff));
            pen.Text("Kiro Web", chx + 19, cyy + (17 - pen.LineHeight(chipF)) / 2, chipF with { Color = Rgba.Of(0xb8d4ff) });
        }
        if (r.Open == 2)
        {
            var cyy = ry + (row2 - 17) / 2;
            pen.Box(chx, cyy, wkW, 17, 5, Rgba.Of(0xffffff, 0.07));
            pen.TextIn("CLI", new Rect4(chx, cyy, wkW, 17), chipF);
        }
        Add(card, "panelRow", idx);
        if (r.Open == 0)
        {
            var dh = Over(del);
            if (hov)
            {
                pen.Box(del, 8, dh ? Rgba.Of(0xff453a, 0.14) : Rgba.Clear);
                pen.Icon(DeskIcons.Trash, del.X + 6, del.Y + 5, 14, dh ? Bad : Pen.Dim);
            }
            Add(del, "panelDelete", idx);
        }
    }

    // MARK: Clicks

    bool ListsActivate(Target t)
    {
        switch (t.Id)
        {
            case "officePanel":
                return true;
            case "panelClose":
                OpenOfficePanel("");
                return true;
            case "panelRow":
                if (t.N >= 0 && t.N < panelRows.Count) OpenPanelRow(panelRows[t.N]);
                return true;
            case "panelDelete":
                if (t.N >= 0 && t.N < panelRows.Count && panelRows[t.N] is { Open: 0, Key.Length: > 0 } r)
                {
                    confirm = (0, "deleteKey");
                    confirmKey = r.Key;
                }
                return true;
        }
        return false;
    }

    string confirmKey = "";

    void OpenPanelRow(PanelRow r)
    {
        if (r.Open == 1 && r.Key.StartsWith("web:", StringComparison.Ordinal))
        {
            if (web.Opening.Length > 0) return;
            var id = r.Key[4..];
            // One that a desk already holds opens that desk.
            if (state?.Sessions.FirstOrDefault(x => KiroIdOf(x) == id) is { } held) { OpenOfficePanel(""); OpenChat(held.Id); return; }
            web.Opening = id;
            Say("Opening it from Kiro Web…");
            env.Send(new AdoptWebCommand(id));
        }
        else if (r.Open == 2 && r.Key.StartsWith("cli:", StringComparison.Ordinal))
        {
            if (cli.Opening.Length > 0) return;
            var id = r.Key[4..];
            if (state?.Sessions.FirstOrDefault(x => KiroIdOf(x) == id) is { } held) { OpenOfficePanel(""); OpenChat(held.Id); return; }
            cli.Opening = id;
            Say("Opening it from Kiro CLI…");
            env.Send(new AdoptCliCommand(id));
        }
        else if (r.SessionId is { } sid)
        {
            OpenOfficePanel("");
            OpenChat(sid);
        }
        else if (r.Key.Length > 0)
        {
            OpenOfficePanel("");
            OpenSaved(r.Key);
        }
    }

    /// <summary>The wheel over the panel.</summary>
    bool PanelWheel(float x, float y, float delta)
    {
        if (OfficePanel.Length == 0 || !panelListRect.Contains(x, y)) return false;
        panelScroll = Math.Max(0, panelScroll - delta);
        Dirty = true;
        return true;
    }

    // MARK: Small things the keys and edits need

    void AfterEdit()
    {
        if (focus == Focus.Reply) ReplyEdited();
        else if (focus == Focus.Desk && deskField is { } f)
        {
            if (f.Key == "card" && deskCard is { } id) cardDrafts[id] = f.Box.Text;
            if (f.Key == "histFind") panelScroll = 0;
        }
    }

    void DeskEnter(bool shift)
    {
        if (deskField is not { } f) return;
        switch (f.Key)
        {
            case "term":
                DeskTermRun(f.Box.Text);
                f.Box.Clear();
                break;
            case "fileEdit":
            case "prBody":
                f.Box.Insert("\n", now);
                break;
        }
    }

    /// <summary>The reply box's list takes the Up and Down keys, Tab and Enter while it has something to pick, and Escape.</summary>
    bool ReplyListKey(UiKey key, bool shift)
    {
        if (pop is null) return false;
        if (key == UiKey.Escape)
        {
            PopClose();
            return true;
        }
        if (pop.Items.Count == 0) return false;
        switch (key)
        {
            case UiKey.Up: PopMove(-1); return true;
            case UiKey.Down: PopMove(1); return true;
            case UiKey.Tab: PopPick(-1); return true;
            case UiKey.Enter when !shift: PopPick(-1); return true;
        }
        return false;
    }
}
