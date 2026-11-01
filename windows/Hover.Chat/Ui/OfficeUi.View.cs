using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

public sealed partial class OfficeUi
{
    bool sideClosed, sideShown, listAllowed;
    bool taskOpen;
    int clicksNow = 1;
    UiMods modsNow;
    Rect4 editArea;
    bool editDragging;
    HashSet<int>? idsBeforeStart;

    // Of the new task: what the pills show until the user changes them.
    string? taskTool, taskFolder, taskAccess, taskModel, taskEffort, taskRepo;
    bool taskCloud;
    List<string> repos = [];
    string reposNote = "";
    List<ProjectInfo> projects = [];
    readonly EditBox repoFind = new();

    // MARK: The office

    void PaintOffice(bool compact)
    {
        var sideGap = compact ? 8f : 12f;
        var sideW = Math.Min(compact ? 360f : 400f, w - 2 * sideGap);
        sideShown = false;
        listAllowed = false;
        threadRect = default;
        chatRect = default;
        // The questions over the bots first, so a chat's own card is above them.
        PaintViewSwitch(12, compact ? 8 : 10, false, compact);
        var drawer = OpenId is not null || openKey is not null;
        if (!drawer) PaintMenuButton(compact);
        PaintTags();
        PaintAsksOver(compact);
        PaintTaskFab(compact);
        if (drawer) PaintChatPane(w - sideW - sideGap, sideGap, sideW, h - 2 * sideGap, compact ? 16 : 20, false, compact);
        PaintOfficePanel(compact);
        // The desk: its panel on the right, its card where its desk is.
        if (deskPanel is not null)
        {
            var deskW = Math.Min(720f, w - 2 * sideGap);
            PaintDeskPanel(new Rect4(w - deskW - sideGap, sideGap, deskW, h - 2 * sideGap), compact ? 16 : 20, false);
        }
        PaintDeskCardLayer();
        PaintTip();
    }

    /// <summary>The questions that wait, each over its bot's head (or stacked at the top when the office does not show it).</summary>
    void PaintAsksOver(bool compact)
    {
        if (state is null) return;
        var cw = 262f;
        var fallbackY = compact ? 52f : 58f;
        foreach (var s in state.Sessions)
        {
            if (s.Ask is not { } a || !AskOver(s)) continue;
            var ah = AskCard(a, s, true, cw, 0, 0, true);
            float ax, ay;
            // The card stands on the bot's name tag, over its chip (internal/ui/officeview.go tag); without a tag, over the bot.
            if (TagOf(s.Id) is { } tag)
            {
                ax = Math.Clamp(tag.X - cw / 2, 8, w - cw - 8);
                ay = Math.Max(tag.Y - TagH - 4 - ah, 8);
            }
            else if (env.BotRect(s.Id) is { } bot)
            {
                ax = Math.Clamp(bot.X + bot.W / 2 - cw / 2, 8, w - cw - 8);
                ay = bot.Y - ah - 14;
                if (ay < 8) ay = bot.Y + bot.H + 8;
            }
            else
            {
                ax = Math.Clamp(w / 2 - cw / 2, 8, w - cw - 8);
                ay = fallbackY;
                fallbackY += ah + 10;
            }
            AskFrame(a, true, ax, ay, cw, ah);
            AskCard(a, s, true, cw, ax, ay, false);
        }
    }

    void PaintMenuButton(bool compact)
    {
        var r = new Rect4(w - 12 - 36, compact ? 8 : 10, 36, 36);
        var hov = Over(r) || menu?.Kind == "top";
        pen.Box(r, 11, Rgba.Of(0x0a060e, 0.8));
        pen.Border(r.X, r.Y, r.W, r.H, 11, 1, Rgba.Of(0xffffff, hov ? 0.2 : 0.1));
        pen.Icon(Glyphs.Menu, r.X + 10, r.Y + 10, 16, hov ? Pen.Ink : Pen.Dim);
        Add(r, "topMenu");
    }

    /// <summary>The switch between the office and the chat view: two icons in a pill.</summary>
    void PaintViewSwitch(float x, float y, bool glass, bool compact)
    {
        float vw = compact ? 68 : 76, vh = compact ? 28 : 32;
        pen.Box(x, y, vw, vh, vh / 2, Rgba.Of(0x0a060e, ChatView ? 0.0 : 0.72));
        if (ChatView) pen.Box(x, y, vw, vh, vh / 2, Rgba.Of(0xffffff, 0.05));
        pen.Border(x, y, vw, vh, vh / 2, 1, Rgba.Of(0xffffff, 0.1));
        var half = vw / 2;
        for (var i = 0; i < 2; i++)
        {
            var on = (i == 1) == ChatView;
            var cell = new Rect4(x + 3 + i * (half - 3), y + 3, half - 3, vh - 6);
            if (on) pen.Box(cell, (vh - 6) / 2, Rgba.Of(0xffffff, 0.09));
            var ic = i == 0 ? Glyphs.Office : Glyphs.Chat;
            pen.Icon(ic, cell.X + (cell.W - 15) / 2, cell.Y + (cell.H - 15) / 2, 15, on ? Pen.Lilac : Over(cell) ? Pen.Dim : Rgba.Of(0xf6f2ff, 0.45));
            Add(cell, "view", i);
        }
    }

    // MARK: The new-task circle and box

    void PaintTaskFab(bool compact)
    {
        if (!taskOpen)
        {
            var r = new Rect4(14, h - 14 - 36, 36, 36);
            var hov = Over(r);
            pen.Box(r, 18, Rgba.Of(0x1c1822, hov ? 0.95 : 0.82));
            pen.Border(r.X, r.Y, r.W, r.H, 18, 1, Rgba.Of(0xffffff, 0.14));
            pen.Icon(Glyphs.Plus, r.X + 10, r.Y + 10, 16, Pen.Ink);
            pen.Box(r.Right - 8, r.Y + 1, 6, 6, 3, Pen.Lilac);
            Add(r, "fab");
            return;
        }
        var bw = Math.Min(408f, w - 28);
        var innerW = bw - 24;
        var look = Pen.F(13.5, 400, Pen.Ink);
        var textW = innerW - 30 - 10 - 34;
        var th = Math.Clamp(task.HeightFor(pen, textW, look, 22, 120), 22, 120);
        var bh = 12 + ShotsHeight(true) + Math.Max(th, 30) + 8 + 40 + 6;
        var box = new Rect4(14, h - 14 - bh, bw, bh);
        PaintTaskBox(box, false, th);
    }

    /// <summary>Resolves the tool, folder, access and model the pills show.</summary>
    (ToolInfo? Tool, string Folder, string Access, string Model, string Effort) TaskPicks()
    {
        var toolId = taskTool ?? state?.Tool ?? state?.Tools.FirstOrDefault(t => t.Ready)?.Id ?? "kiro";
        var tool = ToolOf(toolId);
        var folder = taskFolder ?? state?.Folder ?? "";
        var access = taskAccess ?? (tool?.Access.Length > 0 ? tool.Access : "full");
        var model = taskModel ?? tool?.Model ?? "";
        var effort = taskEffort ?? tool?.Effort ?? "";
        return (tool, folder, access, model, effort);
    }

    static string AccessLabel(string id) => id switch
    {
        "risky" => "Ask first",
        "always" => "Ask always",
        "read" => "Read only",
        _ => "Full access",
    };

    static string ModelLabel(ToolInfo? tool, string model, string effort)
    {
        var name = tool?.Models.FirstOrDefault(m => m.Id == model)?.Name;
        if (string.IsNullOrEmpty(name)) name = model.Length > 0 ? model : "Default";
        return effort.Length > 0 ? name + " · " + effort : name;
    }

    /// <summary>A pill in the box's bar: an icon, words and a chevron. Returns its width.</summary>
    float Pill(string id, string? mark, string? icon, string text, float x, float y, float h0, Rgba fg, bool open, float maxW = 220)
    {
        var f = Pen.F(12.5, 500, fg);
        var tw = pen.Width(text, f);
        var iw = mark is not null || icon is not null ? 16f + 6 : 0f;
        var pw = Math.Min(maxW, 9 + iw + tw + 6 + 13 + 9);
        var r = new Rect4(x, y, pw, h0);
        var hov = Over(r) || open;
        if (hov) pen.Box(r, 8, Rgba.Of(0xffffff, 0.043));
        var tx = x + 9;
        if (mark is not null) { pen.Mark(mark, tx, y + (h0 - 16) / 2, 16); tx += 22; }
        else if (icon is not null) { pen.Icon(icon, tx + 1, y + (h0 - 14) / 2, 14, hov ? Pen.Ink : fg); tx += 22; }
        var color = hov ? Pen.Ink : fg;
        pen.Text(text, tx, y + (h0 - pen.LineHeight(f)) / 2, f with { Color = color }, Math.Max(pw - (tx - x) - 6 - 13 - 9, 0));
        pen.Icon(Glyphs.Chevron, x + pw - 9 - 13, y + (h0 - 13) / 2, 13, color);
        Add(r, id);
        return pw;
    }

    /// <summary>The new-task box: the words, then a bar of pills and the round button. <paramref name="start"/> is the chat view's wide start screen.</summary>
    void PaintTaskBox(Rect4 box, bool start, float textH)
    {
        var (tool, folder, access, model, effort) = TaskPicks();
        var foc = focus == Focus.Task;
        pen.Box(box, start ? 22 : 20, Rgba.Of(0x1b181f, 0.97));
        pen.Border(box.X, box.Y, box.W, box.H, start ? 22 : 20, 1, foc ? Rgba.Of(0xc4a2ff, 0.45) : Rgba.Of(0xffffff, 0.12));
        Add(box, "taskBox");
        var look = Pen.F(start ? 14 : 13.5, 400, Pen.Ink);
        float cx = box.X + (start ? 18 : 12), cy = box.Y + (start ? 14 : 12);
        if (ShotsHeight(true) > 0)
        {
            PaintShots(taskShots, true, cx + 3, cy + 3);
            cy += ShotsHeight(true);
        }
        float textX = cx, textW = box.W - (start ? 36 : 24);
        if (!start)
        {
            // The agent's logo before the words, and a fold at the end.
            pen.Mark(tool?.Id ?? "kiro", cx, cy, 30);
            Add(new Rect4(cx, cy, 30, 30), "pillAgent");
            textX = cx + 30 + 10;
            textW = box.W - 24 - 30 - 10 - 34;
        }
        var area = new Rect4(textX, cy + (start ? 0 : (30 - Math.Min(textH, 30)) / 2), textW, Math.Max(textH, 22));
        var shaped = pen.Shape(task.Text.Length == 0 ? " " : task.Text, look, area.W);
        taskScroll = task.ScrollFor(shaped, area.H, taskScroll);
        var placeholder = start ? $"Ask {ToolDisplay(tool)} to build, fix or explain something. @ adds files, / for commands" : "Describe a task for " + ToolDisplay(tool);
        var painted = task.Paint(pen, area, look, placeholder, foc, now, taskScroll);
        if (foc) { boxLayout = painted; }
        editArea = foc ? area : editArea;
        Add(area, "taskText");
        if (!start)
        {
            var fold = new Rect4(box.Right - 12 - 30, cy, 30, 30);
            pen.Box(fold, 8, Rgba.Of(0xffffff, Over(fold) ? 0.1 : 0.05));
            pen.Icon(Glyphs.Chevron, fold.X + 8, fold.Y + 8, 14, Pen.Dim);
            Add(fold, "taskFold");
        }
        // The bar.
        var barY = start ? box.Bottom - 14 - 28 : box.Bottom - 6 - 34;
        var bh = 28f;
        var x = box.X + (start ? 12 : 8);
        var attach = new Rect4(x, barY, 28, bh);
        pen.Icon(Glyphs.Plus, x + 7, barY + 7, 14, Over(attach) ? Pen.Ink : Pen.Dim);
        Add(attach, "attach");
        x += 28 + 2;
        if (start)
        {
            x += Pill("pillAgent", tool?.Id ?? "kiro", null, ToolDisplay(tool), x, barY, bh, Pen.Dim, menu?.Kind == "agent") + 2;
            x += Pill("pillWhere", null, taskCloud ? Glyphs.Cloud : Glyphs.Folder, taskCloud ? "Kiro Web" : "This computer", x, barY, bh, Pen.Dim, menu?.Kind == "where") + 2;
            if (taskCloud)
                x += Pill("pillRepo", null, null, taskRepo is { Length: > 0 } rp0 ? LastPart(rp0) : "Choose repo", x, barY, bh, Pen.Dim, menu?.Kind == "repo", 150) + 2;
        }
        else
        {
            var cloud = new Rect4(x, barY, 28, bh);
            var cloudOn = taskCloud;
            pen.Box(cloud, 8, cloudOn ? Rgba.Of(0x8fbcff, 0.18) : Over(cloud) ? Rgba.Of(0xffffff, 0.043) : Rgba.Clear);
            pen.Icon(Glyphs.Cloud, x + 7, barY + 7, 14, cloudOn ? Rgba.Of(0x8fbcff) : Pen.Dim);
            if (ToolSupportsCloud(tool)) Add(cloud, "cloud");
            x += 28 + 2;
            if (taskCloud)
                x += Pill("pillRepo", null, null, taskRepo is { Length: > 0 } rp ? LastPart(rp) : "Choose repo", x, barY, bh, Pen.Dim, menu?.Kind == "repo", 150) + 2;
            else if (folder.Length > 0 || true)
                x += Pill("pillFolder", null, Glyphs.Folder, folder.Length > 0 ? LastPart(folder) : "Folder", x, barY, bh, Pen.Dim, menu?.Kind == "project", 150) + 2;
        }
        var amber = access == "full";
        x += Pill("pillAccess", null, Glyphs.Shield, AccessLabel(access), x, barY, bh, amber ? Pen.Amber : Pen.Dim, menu?.Kind == "access") + 2;
        // The round button at the right; the model before it.
        var gs = start ? 32f : 30f;
        var send = new Rect4(box.Right - (start ? 14 : 8) - gs, barY + (bh - gs) / 2, gs, gs);
        var enabled = task.Text.Trim().Length > 0;
        var hov = Over(send) && enabled;
        pen.Box(send, gs / 2, !enabled ? Rgba.Of(0xffffff, 0.07) : hov ? Rgba.Of(0xffffff) : Pen.Ink);
        pen.Icon(Glyphs.Send, send.X + (gs - 16) / 2, send.Y + (gs - 16) / 2, 16, enabled ? Rgba.Of(0x0c0b0e) : Rgba.Of(0xf6f2ff, 0.28), 2.4f);
        Add(send, enabled ? "startTask" : "startOff");
        var mlabel = ModelLabel(tool, model, effort);
        var mf = Pen.F(12.5, 500, Pen.Dim);
        var mpw = Math.Min(Math.Max(send.X - 4 - x, 40), 9 + pen.Width(mlabel, mf) + 6 + 13 + 9);
        if (tool is { Models.Count: > 0 } || tool is not null)
            Pill("pillModel", null, null, mlabel, send.X - 4 - mpw, barY, bh, Pen.Dim, menu?.Kind == "model", mpw);
    }

    float taskScroll;

    string ToolDisplay(ToolInfo? t) => t is null ? "Kiro" : t.Name.Length > 0 ? t.Name : t.Id;

    static bool ToolSupportsCloud(ToolInfo? t) => t?.Id == "kiro";

    void StartTask()
    {
        var text = task.Text.Trim();
        if (text.Length == 0) return;
        var (tool, folder, access, model, effort) = TaskPicks();
        if (tool is not null && !tool.Ready)
        {
            Say(tool.Hint.Length > 0 ? tool.Hint : ToolDisplay(tool) + " is not ready yet.");
            return;
        }
        if (!taskCloud && folder.Length == 0)
        {
            Say("Pick a folder for the task first.");
            return;
        }
        idsBeforeStart = state?.Sessions.Select(s => s.Id).ToHashSet() ?? [];
        env.Send(new NewTaskCommand(tool?.Id ?? "kiro", taskCloud ? "" : folder, text, access)
        {
            Images = taskShots.Count > 0 ? [.. taskShots] : null,
            Cloud = taskCloud ? (taskRepo is { Length: > 0 } r ? [r] : []) : null,
            Model = model.Length > 0 ? model : null,
            Effort = effort.Length > 0 ? effort : null,
        });
        task.Clear();
        taskShots.Clear();
        taskScroll = 0;
        taskOpen = false;
        focus = Focus.None;
        Dirty = true;
    }

    // MARK: The chat view

    void PaintChatView(bool compact)
    {
        pen.Box(0, 0, w, h, 0, Rgba.Of(0x0c0b0e));
        Block(new Rect4(0, 0, w, h));
        var deskW = deskPanel is not null ? Math.Min(560f, Math.Max(360f, w * 0.42f)) : 0f;
        listAllowed = w >= 760 && !(deskPanel is not null && w < (env.InNotch ? 1240 : 1300));
        sideShown = listAllowed && !sideClosed;
        var lw = sideShown ? 260f : 0f;
        threadRect = default;
        chatRect = default;
        sideRect = new Rect4(0, 0, lw, h);
        if (sideShown) PaintSidebar(lw, compact);
        var opened = OpenId is not null || openKey is not null;
        if (opened) PaintChatPane(lw, 0, w - lw - deskW, h, 0, true, compact);
        else PaintStart(lw, w - lw, compact);
        if (deskPanel is not null) PaintDeskPanel(new Rect4(w - deskW, 0, deskW, h), 0, true);
        // The switch stays where the sidebar has no room, and on the start screen.
        if (!sideShown) PaintViewSwitch(12, compact ? 8 : 10, true, compact);
        if (!sideShown && listAllowed && !opened)
        {
            var bx = 12 + (compact ? 68 : 76) + 6;
            IconButton("showSide", Glyphs.Sidebar, bx, (compact ? 8 : 10) + ((compact ? 28 : 32) - 28) / 2f, 16);
        }
    }

    sealed record ListRow(string Key, int? Id, string Tool, string Title, string Folder, long At, string Stage, string Badge = "");

    List<ListRow> ListRows()
    {
        var rows = new List<ListRow>();
        if (state is null) return rows;
        var live = state.Sessions.ToDictionary(s => s.Key.Length > 0 ? s.Key : "#" + s.Id);
        var seen = new HashSet<string>();
        foreach (var hrow in state.History)
        {
            if (live.TryGetValue(hrow.Key, out var s))
            {
                seen.Add(hrow.Key);
                rows.Add(new ListRow(hrow.Key, s.Id, s.Tool, s.Title.Length > 0 ? s.Title : hrow.Title, s.Folder, LiveAt(s), s.Stage));
            }
            else rows.Add(new ListRow(hrow.Key, null, hrow.Tool, hrow.Title, hrow.Folder, hrow.At, hrow.Stage));
        }
        foreach (var (key, s) in live)
            if (!seen.Contains(key)) rows.Add(new ListRow(key, s.Id, s.Tool, s.Title, s.Folder, LiveAt(s), s.Stage));
        // This computer's Kiro sessions Hover does not have yet, with a badge, under their folder (the 30 newest).
        var haveKiro = state.Sessions.Select(KiroIdOf).Where(k => k.Length > 0).ToHashSet();
        foreach (var c in cli.List.Where(c => !haveKiro.Contains(c.Id)).OrderByDescending(c => c.Updated ?? 0).Take(30))
            rows.Add(new ListRow("cli:" + c.Id, null, "kiro", c.Title.Length > 0 ? c.Title : "Kiro CLI session", c.Folder ?? "", c.Updated ?? 0, "stopped", "CLI"));
        return [.. rows.OrderByDescending(r => r.At)];
    }

    static long LiveAt(SessionInfo s) => s.Turns.Count > 0 && s.Turns[^1].T0 > 0 ? s.Turns[^1].T0 : DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();

    string Ago(long atMs)
    {
        var nowMs = new DateTimeOffset(DateTime.SpecifyKind(env.Utc(), DateTimeKind.Utc)).ToUnixTimeMilliseconds();
        var s = Math.Max((nowMs - atMs) / 1000, 0);
        if (s < 60) return "now";
        if (s < 3600) return s / 60 + "m";
        if (s < 86400) return s / 3600 + "h";
        return s / 86400 + "d";
    }

    void PaintSidebar(float lw, bool compact)
    {
        pen.Box(lw - 1, 0, 1, h, 0, Rgba.Of(0xffffff, 0.06));
        var top = compact ? 8f : 10f;
        PaintViewSwitch(12, top, false, compact);
        var vh = compact ? 28f : 32f;
        // Hide the list.
        IconButton("hideSide", Glyphs.Sidebar, lw - 12 - 28, top + (vh - 28) / 2, 16);
        float y = top + vh + 14;
        // New chat.
        var nr = new Rect4(8, y, lw - 16, 32);
        var nhov = Over(nr);
        pen.Box(nr, 8, Rgba.Of(0xffffff, nhov ? 0.05 : 0));
        pen.Icon(Glyphs.Compose, 18, y + 8, 16, Pen.Ink);
        pen.Text("New chat", 43, y + (32 - pen.LineHeight(Pen.F(13.5, 500, Pen.Ink))) / 2, Pen.F(13.5, 500, Pen.Ink));
        Add(nr, "newChat");
        y += 32 + 10;
        var rows = ListRows();
        var listTop = y;
        pen.Cv.PushClip(new Rect4(0, listTop, lw - 1, h - listTop));
        y -= sideScroll;
        string? group = null;
        var gf = Pen.F(12.5, 400, Pen.Dim);
        var rf = Pen.F(13, 400, Rgba.Of(0xf6f2ff, 0.5));
        var selected = OpenId is { } oid ? rows.FirstOrDefault(r => r.Id == oid)?.Key : openKey;
        var total = 0f;
        for (var i = 0; i < rows.Count; i++)
        {
            var r = rows[i];
            var g = r.Folder.Length > 0 ? LastPart(r.Folder) : "Cloud";
            if (g != group)
            {
                group = g;
                y += 6;
                pen.Icon(Glyphs.Folder, 16, y + 7, 15, Pen.Dim);
                pen.Text(g, 37, y + (28 - pen.LineHeight(gf)) / 2, gf, lw - 37 - 12);
                y += 28 + 2;
                total += 36;
            }
            var rr = new Rect4(8, y, lw - 16, 30);
            var sel = r.Key == selected;
            var hov = Over(rr);
            if (sel) pen.Box(rr, 8, Rgba.Of(0xffffff, 0.06));
            else if (hov) pen.Box(rr, 8, Rgba.Of(0xffffff, 0.035));
            pen.Mark(r.Tool.Length > 0 ? r.Tool : "kiro", 16, y + 6, 18);
            var ago = r.Badge.Length > 0 && cli.Opening.Length > 0 && r.Key == "cli:" + cli.Opening ? "Opening…" : Ago(r.At);
            var af = Pen.F(11.5, 400, Rgba.Of(0xf6f2ff, 0.45));
            var aw = pen.Width(ago, af);
            pen.Text(ago, lw - 16 - 8 - aw, y + (30 - pen.LineHeight(af)) / 2, af);
            float badgeW = 0;
            if (r.Badge.Length > 0)
            {
                var bf = Pen.F(10, 500, Pen.Dim);
                badgeW = pen.Width(r.Badge, bf) + 10;
                pen.Box(lw - 16 - 8 - aw - 6 - badgeW, y + 7, badgeW, 16, 5, Rgba.Of(0xffffff, 0.07));
                pen.TextIn(r.Badge, new Rect4(lw - 16 - 8 - aw - 6 - badgeW, y + 7, badgeW, 16), bf);
                badgeW += 6;
            }
            var color = sel ? Pen.Ink : hov ? Pen.Dim : Rgba.Of(0xf6f2ff, 0.5);
            pen.Text(r.Title.Length > 0 ? r.Title : "New chat", 42, y + (30 - pen.LineHeight(rf)) / 2, rf with { Color = color }, lw - 42 - 16 - aw - 14 - badgeW);
            if (y + 30 > listTop && y < h) Add(rr, "row", i, r.Key);
            y += 30;
            total += 30;
        }
        pen.Cv.PopClip();
        var maxScroll = Math.Max(total + 40 - (h - listTop), 0);
        sideScroll = Math.Clamp(sideScroll, 0, maxScroll);
        rowKeys = [.. rows.Select(r => (r.Key, r.Id))];
    }

    List<(string Key, int? Id)> rowKeys = [];

    // MARK: The start screen

    void PaintStart(float x, float sw, bool compact)
    {
        var (tool, folder, access, model, effort) = TaskPicks();
        var cx = x + sw / 2;
        var bw = Math.Min(700f, sw - 48);
        var look = Pen.F(14, 400, Pen.Ink);
        var th = Math.Clamp(task.HeightFor(pen, bw - 36, look, 56, 200), 56, 200);
        var bh = 14 + ShotsHeight(true) + th + 10 + 28 + 14;
        var headF = Pen.F(24, 600, Pen.Ink);
        var headH = pen.LineHeight(headF);
        var chipH = 28f;
        var total = headH + 10 + chipH + 22 + bh;
        var top = Math.Max((h - total) / 2 - 40, 60);
        var hw = pen.Width("What should we work on?", headF);
        pen.Text("What should we work on?", cx - hw / 2, top, headF);
        var chipY = top + headH + 10;
        var fl = folder.Length > 0 ? LastPart(folder) : "Choose a folder";
        var ff = Pen.F(13, 400, Pen.Dim);
        var fw = pen.Width(fl, ff) + 16 + 6 + 13 + 12;
        var chip = new Rect4(cx - fw / 2, chipY, fw, chipH);
        var chov = Over(chip) || menu?.Kind == "project";
        pen.Icon(Glyphs.Folder, chip.X + 6, chipY + 7, 14, chov ? Pen.Ink : Pen.Dim);
        pen.Text(fl, chip.X + 6 + 14 + 8, chipY + (chipH - pen.LineHeight(ff)) / 2, ff with { Color = chov ? Pen.Ink : Pen.Dim });
        pen.Icon(Glyphs.Chevron, chip.Right - 6 - 13, chipY + 7.5f, 13, chov ? Pen.Ink : Pen.Dim);
        Add(chip, "pillFolder");
        var box = new Rect4(cx - bw / 2, chipY + chipH + 22, bw, bh);
        PaintTaskBox(box, true, th);
    }

    // MARK: The switch and the lists, as actions

    void OpenRow(int index)
    {
        if (index < 0 || index >= rowKeys.Count) return;
        var (key, id) = rowKeys[index];
        if (key.StartsWith("cli:", StringComparison.Ordinal))
        {
            OpenPanelRow(new PanelRow(6) { Open = 2, Key = key });
            ChatView = true;
            return;
        }
        if (id is { } sid) OpenChat(sid); else OpenSaved(key);
        ChatView = true;
    }

    /// <summary>A started task shows up in the next state; its chat opens.</summary>
    void OpenStarted()
    {
        if (idsBeforeStart is null || state is null) return;
        var fresh = state.Sessions.Where(s => !idsBeforeStart.Contains(s.Id)).OrderByDescending(s => s.Id).FirstOrDefault();
        if (fresh is null) return;
        idsBeforeStart = null;
        OpenChat(fresh.Id);
    }
}
