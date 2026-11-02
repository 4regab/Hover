using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

public sealed partial class OfficeUi
{
    /// <summary>A row of a menu. <paramref name="Pin"/>: the leading and trailing pinned rows stay in view while the rows between them scroll.</summary>
    sealed record MenuRow(string Label, string Id = "", string Note = "", bool On = false, bool Header = false, bool Rule = false, string? Mark = null, bool Disabled = false, bool Pin = false);

    sealed class Menu
    {
        public string Kind = "", Anchor = "";
        public Rect4 At;
        public List<MenuRow> Rows = [];
        public bool Up;
        public float W = 250;
        /// <summary>Makes the rows again each frame, for a menu whose contents arrive from the backend while it is open.</summary>
        public Func<List<MenuRow>>? Build;
        /// <summary>The scroll of the rows between the pinned ones.</summary>
        public readonly MenuScroll Scroll = new();
        /// <summary>The menu has opened on its picked row (or has none to open on).</summary>
        public bool Centered;
        /// <summary>The row the Up and Down keys are on, or -1.</summary>
        public int Hot = -1;
        public bool RevealHot;
        /// <summary>Where the last frame drew the box, and the part of it that scrolls.</summary>
        public Rect4 Box, List;
    }

    Menu? menu;
    bool askedProjects;

    void OpenMenu(string kind, Rect4 at, List<MenuRow> rows, bool up, float width = 250, string anchor = "", Func<List<MenuRow>>? build = null)
    {
        menu = new Menu { Kind = kind, At = at, Rows = rows, Up = up, W = width, Anchor = anchor.Length > 0 ? anchor : kind, Build = build };
        Dirty = true;
    }

    Rect4 AnchorOf(string id) => targets.LastOrDefault(t => t.Id == id).R;

    // MARK: Painting the overlays

    void PaintOverlays()
    {
        if (menu is not null) PaintMenu(menu);
        if (confirm is not null) PaintConfirm();
        if (ToastText is { } toast)
        {
            var f = Pen.F(12.5, 600, Pen.Ink);
            var tw = pen.Width(toast, f);
            var gw = Math.Min(tw + 28, w - 24);
            var gh = pen.LineHeight(f) + 16;
            var gx = (w - gw) / 2;
            var gy = h <= 620 ? 10f : 18f;
            pen.Box(gx, gy, gw, gh, 12, Rgba.Of(0x0a060e, 0.94));
            pen.Border(gx, gy, gw, gh, 12, 1, Rgba.Of(0xffffff, 0.12));
            pen.TextIn(toast, new Rect4(gx, gy, gw, gh), f);
            Block(new Rect4(gx, gy, gw, gh));
        }
    }

    static float RowHeight(Pen pen, MenuRow r, float w)
    {
        if (r.Rule) return 9;
        if (r.Header) return 24;
        if (r.Note.Length == 0) return 30;
        var lh = pen.LineHeight(Pen.F(12.5, 600, Pen.Ink));
        return 7 + lh + 1 + pen.HeightWhenWrapped(r.Note, w - 12 - 16 - (r.On ? 20 : 0), Pen.F(11, 400, Pen.Faint)) + 7;
    }

    /// <summary>
    /// The menu of the HUD's button (internal/ui/officeview.go hud): the time of day as three icons (follow the clock, night, day), then
    /// Chill beats, Session history and Settings. Picking the time or the beats leaves the menu open; the other rows close it.
    /// </summary>
    void PaintTopMenu(Menu m)
    {
        var compact = h <= 620;
        const float mw = 184;
        var bh = 5 + 30 + 4 + m.Rows.Count * 32 + 5;
        var box = new Rect4(Math.Max(w - mw - (compact ? 10 : 16), 8), Math.Clamp(compact ? 48 : 64, 8, Math.Max(h - 8 - bh, 8)), mw, bh);
        m.Box = box;
        PaintGlass(box, 13);
        Add(box, "menuBox");
        var white = Rgba.Of(0xffffff);
        var quiet = new Rgba(0xf6, 0xf2, 0xff, 0x9e);
        float tx = box.X + 5, ty = box.Y + 5;
        pen.Box(tx, ty, mw - 10, 30, 9, new Rgba(0xff, 0xff, 0xff, 0x0d));
        var cw = (mw - 10 - 4 - 2 * 2) / 3;
        var mode = env.TimeMode();
        string[] times = [Glyphs.Auto, Glyphs.Night, Glyphs.Sun];
        for (var i = 0; i < 3; i++)
        {
            var x = tx + 2 + i * (cw + 2);
            var cell = new Rect4(x, ty + 2, cw, 26);
            if (mode == i) pen.Box(cell, 7, new Rgba(0xff, 0xff, 0xff, 0x24));
            pen.Icon(times[i], x + (cw - 15) / 2, ty + (30 - 15) / 2, 15, mode == i || Over(cell) ? white : quiet);
            Add(cell, "menuTime", i);
        }
        var gold = Rgba.Of(0xffd27a);
        var lf = Pen.F(12.5, 500, Rgba.Of(0xf6f2ff));
        var ry = ty + 30 + 4;
        for (var i = 0; i < m.Rows.Count; i++)
        {
            var row = new Rect4(box.X + 5, ry, mw - 10, 32);
            if (Over(row) || m.Hot == i) pen.Box(row, 8, new Rgba(0xff, 0xff, 0xff, 0x17));
            var on = m.Rows[i].Id == "beats" && env.Beats();
            var icon = m.Rows[i].Id switch { "beats" => Glyphs.Notes, "history" => Glyphs.History, _ => Glyphs.Cog };
            pen.Icon(icon, row.X + 8, ry + (32 - 15) / 2, 15, on ? gold : quiet);
            pen.Text(m.Rows[i].Label, row.X + 8 + 15 + 9, ry + (32 - pen.LineHeight(lf)) / 2, lf);
            if (on)
            {
                var onLook = Pen.F(11, 600, gold);
                pen.Text("On", row.Right - 8 - pen.Width("On", onLook), ry + (32 - pen.LineHeight(onLook)) / 2, onLook);
            }
            Add(row, "menu", i, m.Rows[i].Label);
            ry += 32;
        }
        overBlocker = false;
    }

    void PaintMenu(Menu m)
    {
        if (m.Kind == "top")
        {
            PaintTopMenu(m);
            return;
        }
        if (m.Build is not null) m.Rows = m.Build();
        var rows = m.Rows;
        var n = rows.Count;
        const float pad = 6;
        var mw = Math.Max(Math.Min(m.W, w - 16), 0);
        var hs = new float[n];
        for (var i = 0; i < n; i++) hs[i] = RowHeight(pen, rows[i], mw);
        // The pinned rows at the top and the bottom (a heading, the effort levels, a note) stay in view; the rows between them scroll.
        int head = 0, foot = 0;
        while (head < n && rows[head].Pin) head++;
        while (foot < n - head && rows[n - 1 - foot].Pin) foot++;
        float sumHead = 0, sumFoot = 0, sumList = 0;
        for (var i = 0; i < n; i++)
        {
            if (i < head) sumHead += hs[i];
            else if (i >= n - foot) sumFoot += hs[i];
            else sumList += hs[i];
        }
        // A window too short for the pinned rows lets everything scroll.
        var avail = Math.Max(h - 16, 0);
        if (2 * pad + sumHead + sumFoot > avail)
        {
            sumList += sumHead + sumFoot;
            sumHead = sumFoot = 0;
            head = foot = 0;
        }
        var natural = 2 * pad + sumHead + sumList + sumFoot;
        // The menu opens on the side of its pill that was asked for; when it does not fit there it takes the side with more room.
        var above = m.At.Y - 6 - 8;
        var below = h - 8 - (m.At.Bottom + 6);
        var up = m.Up;
        var room = up ? above : below;
        if (natural > room && (up ? below : above) > room)
        {
            up = !up;
            room = up ? above : below;
        }
        // Never taller than the room, nor the window less 16; at least the pinned rows and a few rows of the list (84, as the Go menu).
        var boxH = Math.Min(Math.Min(natural, Math.Max(room, 2 * pad + sumHead + sumFoot + Math.Min(sumList, 84))), Math.Max(avail, 2 * pad));
        var listH = Math.Max(boxH - 2 * pad - sumHead - sumFoot, 0);
        var x = Math.Clamp(m.At.X, 8, Math.Max(w - mw - 8, 8));
        var y = Math.Clamp(up ? m.At.Y - 6 - boxH : m.At.Bottom + 6, 8, Math.Max(h - 8 - boxH, 8));
        var box = new Rect4(x, y, mw, boxH);
        m.Box = box;
        for (var i = 0; i < 4; i++) pen.Box(x - i * 2, y + 6 - i, mw + i * 4, boxH + i * 2, 14 + i * 2, Rgba.Of(0x000000, 0.06));
        pen.Box(box, 14, MenuPanel);
        pen.Border(x, y, mw, boxH, 14, 1, Rgba.Of(0xffffff, 0.12));
        Add(box, "menuBox");

        var listTop = y + pad + sumHead;
        var list = new Rect4(x, listTop, mw, listH);
        m.List = list;
        m.Scroll.Set(sumList, listH);
        var tops = new float[n];
        float at = 0;
        for (var i = head; i < n - foot; i++)
        {
            tops[i] = at;
            at += hs[i];
        }
        // It opens on the picked row; a list that fills in later (the repositories) does so when its picked row arrives, unless the user scrolled.
        if (!m.Centered && !m.Scroll.Touched)
        {
            for (var i = head; i < n - foot; i++)
                if (rows[i].On)
                {
                    m.Scroll.Center(tops[i], tops[i] + hs[i]);
                    m.Centered = true;
                    break;
                }
            if (m.Build is null) m.Centered = true;
        }
        if (m.RevealHot)
        {
            m.RevealHot = false;
            if (m.Hot >= head && m.Hot < n - foot) m.Scroll.Reveal(tops[m.Hot], tops[m.Hot] + hs[m.Hot]);
        }

        var cy = y + pad;
        for (var i = 0; i < head; i++)
        {
            PaintMenuRow(m, i, x, cy, mw, hs[i], null);
            cy += hs[i];
        }
        pen.Cv.PushClip(list);
        for (var i = head; i < n - foot; i++)
        {
            var ry = listTop - m.Scroll.Off + tops[i];
            if (ry + hs[i] <= listTop || ry >= list.Bottom) continue;
            PaintMenuRow(m, i, x, ry, mw, hs[i], list);
        }
        pen.Cv.PopClip();
        m.Scroll.PaintFade(pen.Cv, list, MenuPanel);
        cy = list.Bottom;
        for (var i = n - foot; i < n; i++)
        {
            PaintMenuRow(m, i, x, cy, mw, hs[i], null);
            cy += hs[i];
        }
        if (m.Scroll.Max > 0)
        {
            var strip = MenuScroll.Strip(list);
            Add(strip, "menuBar");
            m.Scroll.PaintBar(pen.Cv, list, Over(strip));
        }
        overBlocker = false;
    }

    /// <summary>The colour of a menu's panel, which the fade at the edge of a scrolling list runs into.</summary>
    static readonly Rgba MenuPanel = Rgba.Of(0x16121c, 0.985);

    static Rect4 Cut(Rect4 a, Rect4 b)
    {
        float x = Math.Max(a.X, b.X), y = Math.Max(a.Y, b.Y);
        return new Rect4(x, y, Math.Max(Math.Min(a.Right, b.Right) - x, 0), Math.Max(Math.Min(a.Bottom, b.Bottom) - y, 0));
    }

    /// <summary>One row of a menu at (x, cy). A row of the scrolling part is cut to <paramref name="clip"/>: only what shows answers the pointer.</summary>
    void PaintMenuRow(Menu m, int i, float x, float cy, float mw, float rh, Rect4? clip)
    {
        var r = m.Rows[i];
        if (r.Rule) { pen.Box(x + 10, cy + 4, mw - 20, 1, 0, Rgba.Of(0xffffff, 0.06)); return; }
        if (r.Header)
        {
            pen.Text(r.Label.ToUpperInvariant(), x + 14, cy + (rh - pen.LineHeight(Pen.F(10.5, 600, Pen.Faint))) / 2, Pen.F(10.5, 600, Pen.Faint));
            return;
        }
        var rr = new Rect4(x + 6, cy, mw - 12, rh);
        var seen = clip is { } c ? Cut(rr, c) : rr;
        var shown = seen.W > 0 && seen.H > 0;
        var hov = !r.Disabled && ((shown && Over(seen)) || m.Hot == i);
        if (hov) pen.Box(rr, 8, Rgba.Of(0xffffff, 0.09));
        var tx = rr.X + 8;
        var lf = Pen.F(12.5, r.Note.Length > 0 ? 600 : 500, r.Disabled ? Pen.Faint : Pen.Ink);
        if (r.Mark is not null)
        {
            pen.Mark(r.Mark, tx, cy + (rh - 18) / 2, 18);
            tx += 26;
        }
        var labelW = rr.Right - tx - 8 - (r.On ? 20 : 0);
        if (r.Note.Length == 0) pen.Text(r.Label, tx, cy + (rh - pen.LineHeight(lf)) / 2, lf, labelW);
        else
        {
            pen.Text(r.Label, tx, cy + 7, lf, labelW);
            pen.Wrapped(r.Note, tx, cy + 7 + pen.LineHeight(lf) + 1, rr.Right - tx - 8 - (r.On ? 20 : 0), Pen.F(11, 400, Pen.Faint));
        }
        if (r.On) pen.Icon(Icons.Check, rr.Right - 8 - 12, cy + (r.Note.Length > 0 ? 10 : (rh - 12) / 2), 12, Pen.Lilac);
        if (!r.Disabled && shown) Add(seen, "menu", i, r.Label);
    }

    // MARK: Actions

    void Activate(Target t)
    {
        clicksNow = Math.Max(clicksNow, 1);
        if (DeskActivate(t) || ListsActivate(t)) return;
        if (t.Id != "askOpt") Trace($"click {t.Id} {t.N} {t.S}".TrimEnd());
        var s = Current();
        switch (t.Id)
        {
            case "view":
                ChatView = t.N == 1;
                menu = null;
                break;
            case "topMenu":
                OpenMenu("top", t.R, env.OpenSettings is null
                    ? [new MenuRow("Chill beats", "beats"), new MenuRow("Session history", "history")]
                    : [new MenuRow("Chill beats", "beats"), new MenuRow("Session history", "history"), new MenuRow("Settings", "settings")], false, 184);
                break;
            case "menuTime":
                env.SetTime?.Invoke(t.N);
                break;
            case "newChat":
                CloseChat();
                ChatView = true;
                focus = Focus.Task;
                break;
            case "row":
                OpenRow(t.N);
                break;
            case "showSide":
                sideClosed = false;
                break;
            case "hideSide":
                sideClosed = true;
                break;
            case "fab":
                taskOpen = true;
                focus = Focus.Task;
                break;
            case "taskFold":
                taskOpen = false;
                focus = Focus.None;
                break;
            case "taskText":
            case "taskBox":
                focus = Focus.Task;
                if (t.Id == "taskText" && boxLayout is not null && editArea.W > 0)
                {
                    task.Press(boxLayout, px - editArea.X, py - editArea.Y + taskScroll, clicksNow, (modsNow & UiMods.Shift) != 0, now);
                    editDragging = true;
                }
                break;
            case "startTask":
                StartTask();
                break;
            case "startOff":
                focus = Focus.Task;
                break;
            case "attach":
                Attach(true);
                break;
            case "cloud":
                taskCloud = !taskCloud;
                if (taskCloud) env.Send(new CloudReposCommand());
                break;
            case "pillAgent":
                {
                    List<MenuRow> Rows() => [.. (state?.Tools ?? []).Select(x => new MenuRow(ToolDisplay(x), x.Id, x.Ready ? "" : x.Hint, x.Id == TaskPicks().Tool?.Id, Mark: x.Id))];
                    OpenMenu("agent", t.R, Rows(), true, build: Rows);
                    break;
                }
            case "pillWhere":
                {
                    var rows = new List<MenuRow> { new("This computer", "here", On: !taskCloud) };
                    if (ToolSupportsCloud(TaskPicks().Tool)) rows.Add(new MenuRow("Kiro Web", "cloud", "Runs in Kiro's cloud on a repository.", taskCloud));
                    OpenMenu("where", t.R, rows, true);
                    break;
                }
            case "pillRepo":
                OpenRepoMenu(t.R);
                break;
            case "pillFolder":
                OpenFolderMenu(t.R);
                break;
            case "pillAccess":
                {
                    var cur = TaskPicks().Access;
                    var tool = TaskPicks().Tool;
                    var rows = new List<MenuRow> { new((tool is null ? "The agent" : ToolDisplay(tool)) + " may", Header: true, Pin: true) };
                    foreach (var (id, label, note) in AccessChoices)
                        rows.Add(new MenuRow(label, id, id == "risky" && tool?.Id == "codex" ? "Asks to write outside the folder or go online. Codex runs the rest." : note, id == cur));
                    rows.Add(new MenuRow("", Note: "For this task only. Settings keeps the default.", Disabled: true, Pin: true));
                    OpenMenu("access", t.R, rows, true, 270);
                    break;
                }
            case "pillModel":
                OpenModelMenu(t.R);
                break;
            case "menu":
                if (menu is { } m && t.N >= 0 && t.N < m.Rows.Count) MenuPick(m, m.Rows[t.N]);
                break;
            case "close":
                CloseChat();
                break;
            case "more":
                OpenMoreMenu(t.R);
                break;
            case "rename":
                if (s is not null && OpenId is not null)
                {
                    renaming = true;
                    title.SetText(s.Title);
                    title.SelectAll();
                    focus = Focus.Title;
                }
                break;
            case "titleBox":
                focus = Focus.Title;
                break;
            case "botTag":
                OpenChat(t.N);
                break;
            case "openCloud":
                if (s is not null && HasWebPage(s)) env.OpenUrl?.Invoke("https://app.kiro.dev/session/" + Uri.EscapeDataString(KiroIdOf(s)));
                break;
            case "compose":
                compose = true;
                focus = Focus.Reply;
                break;
            case "composer":
            case "replyText":
                focus = Focus.Reply;
                if (t.Id == "replyText" && boxLayout is not null && replyArea.W > 0)
                {
                    reply.Press(boxLayout, px - replyArea.X, py - replyArea.Y + replyScroll, clicksNow, (modsNow & UiMods.Shift) != 0, now);
                    editArea = replyArea;
                    editDragging = true;
                }
                break;
            case "send":
                SendReply();
                break;
            case "pause":
                SendReply();
                break;
            case "sendOff":
                focus = Focus.Reply;
                break;
            case "resume":
                if (OpenId is { } rid) env.Send(new QueueResumeCommand(rid));
                break;
            case "latest":
                follow = true;
                smooth = new Smooth(scroll, MaxScroll, now);
                break;
            case "vbar":
                VerticalBarDown(t);
                break;
            case "askDeny":
                WithAsk(t.S, (sess, a) => Answer(sess, a, "deny"));
                break;
            case "askTrust":
                WithAsk(t.S, (sess, a) => Answer(sess, a, "trust"));
                break;
            case "askAllow":
                WithAsk(t.S, (sess, a) => Answer(sess, a, "allow"));
                break;
            case "askOpen":
                WithAsk(t.S, (sess, a) => OpenChat(sess.Id));
                break;
            case "askAnswer":
                WithAsk(t.S, SendQuestion);
                break;
            case "askOpt":
                WithAsk(AskIdOfFrame(), (sess, a) => PickOption(a, t.N, t.S));
                break;
            case "askCustom":
                WithAsk(t.S, (sess, a) =>
                {
                    var st = AskOf(a);
                    customBox = st.Custom[t.N];
                    customQ = t.N;
                    focus = Focus.Custom;
                });
                break;
            case "confirmNo":
                confirm = null;
                break;
            case "confirmYes":
                ConfirmYes();
                break;
        }
        Dirty = true;
    }

    static readonly (string Id, string Label, string Note)[] AccessChoices =
    [
        ("full", "Full access", "Never asks. Edits, runs commands and goes online on its own."),
        ("risky", "Ask first", "Asks before commands, deletes, the network and anything outside the folder."),
        ("always", "Ask always", "Asks before every change and every command."),
        ("read", "Read only", "Reads and searches. Changes nothing."),
    ];

    /// <summary>Finds the session that has the question with this id and runs the action on both.</summary>
    void WithAsk(string askId, Action<SessionInfo, AskInfo> act)
    {
        if (state is null) return;
        foreach (var s in state.Sessions)
            if (s.Ask is { } a && (askId.Length == 0 || a.Id == askId))
            {
                act(s, a);
                return;
            }
    }

    string AskIdOfFrame() => Current()?.Ask?.Id ?? "";

    void PickOption(AskInfo a, int q, string label)
    {
        var st = AskOf(a);
        var question = a.Questions![q];
        var set = st.Sel[q];
        if (question.Multiple)
        {
            if (!set.Remove(label)) set.Add(label);
        }
        else
        {
            var was = set.Contains(label);
            set.Clear();
            if (!was) set.Add(label);
        }
    }

    void ConfirmYes()
    {
        if (confirm is not { } c) return;
        confirm = null;
        if (c.Kind == "deleteKey")
        {
            env.Send(new DeleteCommand(null, confirmKey));
            return;
        }
        if (OpenId is not { } id) return;
        switch (c.Kind)
        {
            case "restore": env.Send(new RestoreCommand(id, c.Turn)); break;
            case "again": env.Send(new AgainCommand(id, c.Turn)); follow = true; break;
            case "delete":
                env.Send(new DeleteCommand(id));
                CloseChat();
                break;
        }
    }

    void VerticalBarDown(Target t)
    {
        var bar = new Bar(true, t.R.X, t.R.Y, t.R.H, thread.Height, threadRect.H, scroll);
        if (bar.Along(px, py) is not { } along) return;
        var part = bar.PartAt(along);
        if (part == Bar.Part.Thumb)
        {
            barDrag = (true, along - bar.Thumb().Start);
            return;
        }
        ScrollTo(bar.Step(part, along));
    }

    // MARK: Menus

    void OpenFolderMenu(Rect4 at)
    {
        var up = !(ChatView && OpenId is null && openKey is null);
        List<MenuRow> Rows() => FolderRows();
        OpenMenu("project", at, Rows(), up, 300, build: Rows);
    }

    List<MenuRow> FolderRows()
    {
        var rows = new List<MenuRow>();
        var cur = TaskPicks().Folder;
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var p in projects)
            if (p.Folder.Length > 0 && seen.Add(p.Folder)) rows.Add(new MenuRow(p.Name.Length > 0 ? p.Name : LastPart(p.Folder), p.Folder, p.Folder, p.Folder == cur));
        foreach (var hrow in state?.History ?? [])
            if (hrow.Folder.Length > 0 && seen.Add(hrow.Folder) && rows.Count < 12) rows.Add(new MenuRow(LastPart(hrow.Folder), hrow.Folder, hrow.Folder, hrow.Folder == cur));
        if (state?.Folder is { Length: > 0 } sf && seen.Add(sf)) rows.Insert(0, new MenuRow(LastPart(sf), sf, sf, sf == cur));
        if (rows.Count == 0) rows.Add(new MenuRow("No folders yet", Disabled: true));
        return rows;
    }

    void OpenRepoMenu(Rect4 at)
    {
        List<MenuRow> Rows()
        {
            var rows = new List<MenuRow> { new("Empty workspace", "", "Starts with no repository.", string.IsNullOrEmpty(taskRepo)) };
            foreach (var r in repos) rows.Add(new MenuRow(r, r, On: r == taskRepo));
            if (reposNote.Length > 0) rows.Add(new MenuRow(reposNote, Disabled: true));
            return rows;
        }
        OpenMenu("repo", at, Rows(), true, 300, build: Rows);
    }

    void OpenModelMenu(Rect4 at)
    {
        var (tool, _, _, model, effort) = TaskPicks();
        var rows = new List<MenuRow> { new("Model", Header: true) };
        rows.Add(new MenuRow("Default", "model:", On: model.Length == 0));
        foreach (var m in tool?.Models ?? []) rows.Add(new MenuRow(m.Name.Length > 0 ? m.Name : m.Id, "model:" + m.Id, On: m.Id == model));
        var levels = tool?.Models.FirstOrDefault(m => m.Id == model)?.Levels ?? tool?.Efforts;
        if (levels is { Count: > 0 })
        {
            rows.Add(new MenuRow("", Rule: true));
            rows.Add(new MenuRow(tool?.EffortLabel is { Length: > 0 } el ? el : "Effort", Header: true));
            foreach (var l in levels) rows.Add(new MenuRow(l, "effort:" + l, On: l == effort));
        }
        rows.AddRange(ModelFooter(tool));
        OpenMenu("model", at, PinModelEnds(rows), true, 240);
    }

    /// <summary>
    /// A model menu scrolls its models inside the box and keeps the heading, the effort levels and the last row in view
    /// (internal/ui/officemenus.go modelMenu): the heading is pinned, and so is everything from the rule after the models.
    /// </summary>
    static List<MenuRow> PinModelEnds(List<MenuRow> rows)
    {
        var tail = rows.FindIndex(1, r => r.Rule);
        for (var i = 0; i < rows.Count; i++)
            if (i == 0 || (tail > 0 && i >= tail)) rows[i] = rows[i] with { Pin = true };
        return rows;
    }

    /// <summary>A Kiro Web task whose id at Kiro the backend has sent (<c>kiroId</c>, once the first turn has told it): the chip that opens its page needs both.</summary>
    static bool HasWebPage(SessionInfo s) => s.Cloud is not null && KiroIdOf(s).Length > 0;

    /// <summary>The end of a model menu (internal/shell/newtask.go): Kiro's row says when it compacts and opens its Settings; the others keep a note.</summary>
    IEnumerable<MenuRow> ModelFooter(ToolInfo? tool)
    {
        yield return new MenuRow("", Rule: true);
        if (tool?.Id == "kiro") yield return new MenuRow(kiroAutoCompact ? $"Auto compact at {kiroCompactAt}%" : "Auto compact is off", "compact");
        else yield return new MenuRow("Used by " + (tool is null ? "the agent" : ToolDisplay(tool)) + " from its next turn.", Disabled: true);
    }

    void OpenMoreMenu(Rect4 at)
    {
        var s = Current();
        if (s is null) return;
        var rows = new List<MenuRow>();
        if (OpenId is not null)
        {
            AskEditors();
            if (!ChatView) rows.Add(new MenuRow("Open in chat view", "expand"));
            if (editorChoices.Count > 0)
            {
                rows.Add(new MenuRow("Open the folder in", Header: true));
                foreach (var ed in editorChoices) rows.Add(new MenuRow(ed.Name, "editor:" + ed.Id));
                rows.Add(new MenuRow("", Rule: true));
            }
            rows.Add(new MenuRow("Terminal", "terminal"));
            rows.Add(new MenuRow("Files & changes", "files"));
            rows.Add(new MenuRow("", Rule: true));
            rows.Add(new MenuRow("Rename", "rename"));
            rows.Add(new MenuRow("Fork this chat", "fork"));
            var others = (state?.Tools ?? []).Where(t => t.Ready && t.Id != s.Tool).ToList();
            if (others.Count > 0)
            {
                rows.Add(new MenuRow("", Rule: true));
                rows.Add(new MenuRow("Switch agent", Header: true));
                foreach (var t in others) rows.Add(new MenuRow(ToolDisplay(t), "switch:" + t.Id, Mark: t.Id));
            }
            if (s.Turns.Any(x => x.SwitchTo is not null)) rows.Add(new MenuRow("Bring back messages", "bringBack"));
            rows.Add(new MenuRow("", Rule: true));
            rows.Add(new MenuRow("Delete chat", "delete"));
        }
        else rows.Add(new MenuRow("This chat is saved. Open it in the office to change it.", Disabled: true));
        OpenMenu("more", at, rows, false, 230);
    }

    /// <summary>The Up and Down keys (and Home and End) move a lit row over the rows that can be picked, Enter picks it.</summary>
    void MenuKey(Menu m, UiKey key)
    {
        var n = m.Rows.Count;
        bool Pickable(int i) => m.Rows[i] is { Header: false, Rule: false, Disabled: false };
        int Next(int from, int dir)
        {
            for (var i = from + dir; i >= 0 && i < n; i += dir)
                if (Pickable(i)) return i;
            return -1;
        }
        var to = key switch
        {
            UiKey.Down => Next(m.Hot < 0 ? -1 : m.Hot, 1),
            UiKey.Up => Next(m.Hot < 0 ? n : m.Hot, -1),
            UiKey.Home => Next(-1, 1),
            UiKey.End => Next(n, -1),
            _ => -1,
        };
        if (to >= 0)
        {
            m.Hot = to;
            m.RevealHot = true;
        }
        else if (key == UiKey.Enter && m.Hot >= 0 && m.Hot < n && Pickable(m.Hot)) MenuPick(m, m.Rows[m.Hot]);
        Dirty = true;
    }

    void MenuPick(Menu m, MenuRow row)
    {
        if (m.Kind == "top" && row.Id == "beats")
        {
            // The switch is turned and the menu stays open, so the row shows the change.
            env.SetBeats?.Invoke(!env.Beats());
            Dirty = true;
            return;
        }
        menu = null;
        var (tool, _, _, _, _) = TaskPicks();
        switch (m.Kind)
        {
            case "top":
                if (row.Id == "history") OpenOfficePanel(OfficePanel == "history" ? "" : "history");
                else if (row.Id == "settings") env.OpenSettings?.Invoke();
                break;
            case "project":
                taskFolder = row.Id;
                break;
            case "agent":
                taskTool = row.Id;
                // The new tool's own access, model and effort until the user picks.
                taskAccess = taskModel = taskEffort = null;
                if (taskCloud && row.Id != "kiro") taskCloud = false;
                break;
            case "where":
                taskCloud = row.Id == "cloud";
                if (taskCloud) env.Send(new CloudReposCommand());
                break;
            case "repo":
                taskRepo = row.Id.Length > 0 ? row.Id : null;
                break;
            case "access":
                if (row.Id.Length > 0) taskAccess = row.Id;
                break;
            case "model":
                if (row.Id == "compact") env.ShowSettingsPage?.Invoke("kiro");
                else if (row.Id.StartsWith("model:", StringComparison.Ordinal))
                {
                    taskModel = row.Id[6..].Length > 0 ? row.Id[6..] : null;
                    taskEffort = null;
                }
                else if (row.Id.StartsWith("effort:", StringComparison.Ordinal)) taskEffort = row.Id[7..];
                _ = tool;
                break;
            case "more":
                MoreAct(row.Id);
                break;
            case "rmodel":
                ReplyModelPick(row);
                break;
        }
    }

    void MoreAct(string id)
    {
        if (OpenId is not { } sid) return;
        if (id == "expand") ChatView = true;
        else if (id == "terminal") DeskOpenTab(sid, "terminal");
        else if (id == "files") DeskDetails(sid);
        else if (id.StartsWith("editor:", StringComparison.Ordinal)) env.Send(new OpenInCommand(sid) { Editor = id[7..] });
        else if (id == "rename")
        {
            Activate(new Target(default, "rename"));
        }
        else if (id == "fork") env.Send(new ForkCommand(sid, Math.Max(turns.Count - 1, 0)));
        else if (id == "bringBack") env.Send(new BringBackCommand(sid));
        else if (id == "delete") confirm = (0, "delete");
        else if (id.StartsWith("switch:", StringComparison.Ordinal)) env.Send(new SwitchProviderCommand(sid, id[7..]));
    }
}
