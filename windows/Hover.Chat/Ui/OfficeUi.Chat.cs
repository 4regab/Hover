using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

public enum Focus { None, Reply, Task, Title, Custom, Find }

public sealed partial class OfficeUi
{
    readonly EditBox reply, task, title;
    Focus focus, focusBeforeClick;
    ShapedText? boxLayout;
    bool compose, renaming;
    float scroll;
    bool follow = true;
    Smooth? smooth;
    double copiedAt;
    List<Turn> turns = [];
    Rect4 chatRect, threadRect, sideRect;
    float sideScroll;
    bool dragging;
    string? dragLink;
    Pos dragAnchor, dragA0, dragA1;
    Tail dragTail;
    Unit dragUnit;
    (bool Vertical, float Grab)? barDrag;
    float replyScroll;
    (int Turn, string Kind)? confirm;
    Rect4 confirmBox;

    EditBox? FocusedBox() => focus switch
    {
        Focus.Reply when compose => reply,
        Focus.Task when taskOpen || ChatView => task,
        Focus.Title when renaming => title,
        Focus.Custom => customBox,
        Focus.Find => repoFind,
        _ => null,
    };

    void ResetThreadView()
    {
        scroll = 0;
        follow = true;
        smooth = null;
        thread.ClearSelection();
        renaming = false;
        shownSession = int.MinValue;
        shownKey = null;
        var s = Current();
        // Each chat keeps the words typed in its reply box.
        reply.SetText(s is not null && drafts.TryGetValue(SessionKey(s), out var d) ? d : "");
    }

    void SaveDraft()
    {
        if (Current() is not { } s) return;
        if (reply.Text.Length == 0) drafts.Remove(SessionKey(s)); else drafts[SessionKey(s)] = reply.Text;
    }

    // MARK: Scrolling

    float MaxScroll => Math.Max(thread.Height - threadRect.H, 0);

    void ScrollBy(float delta)
    {
        var target = Math.Clamp((smooth is { } sm && !sm.Done(now) ? sm.Target : scroll) + delta, 0, MaxScroll);
        if (smooth is { } cur && !cur.Done(now)) cur.Retarget(target, now);
        else smooth = new Smooth(scroll, target, now);
        follow = target >= MaxScroll - 2;
        Dirty = true;
    }

    void ScrollTo(float y)
    {
        scroll = Math.Clamp(y, 0, MaxScroll);
        smooth = null;
        follow = scroll >= MaxScroll - 2;
        Dirty = true;
    }

    /// <summary>
    /// The host a session's files are named by in its Markdown (f&lt;first 12 of the key&gt;.hover, as the Go
    /// app names it): the backend writes a picture's address as https://host/path and the picture store
    /// reads it from the session's folder.
    /// </summary>
    public static string FilesHost(string key) => key.Length >= 12 ? "f" + key[..12].ToLowerInvariant() + ".hover" : "";

    // MARK: Syncing the thread with the session

    void SyncThread(SessionInfo s, float colw)
    {
        var id = s.Id != 0 ? s.Id : SessionKey(s).GetHashCode();
        var changed = shownSession != id || shownKey != s.Key;
        if (changed)
        {
            shownSession = id;
            shownKey = s.Key;
            thread.Session = id;
            thread.ClearSelection();
            follow = true;
        }
        thread.Who = Bots.Name(s.Bot);
        thread.Tool = s.Tool.Length > 0 ? s.Tool : "kiro";
        thread.Folder = s.Folder;
        thread.Files = FilesHost(s.Key);
        thread.HideSteps = ToolOf(s.Tool)?.HideSteps == true;
        var nowMs = new DateTimeOffset(DateTime.SpecifyKind(env.Utc(), DateTimeKind.Utc)).ToUnixTimeMilliseconds();
        turns = Turns.From(s, nowMs);
        thread.Set(turns, colw);
        if (turns.Any(t => t.Live)) Need(500);
        if (copiedAt > 0 && now - copiedAt > 1500)
        {
            copiedAt = 0;
            thread.SetCopied(turns, null);
        }
        else if (copiedAt > 0) Need(250);
    }

    // MARK: The chat pane

    const float ColumnMax = 820;

    /// <summary>Draws the open chat in a pane (the drawer in the office, or the chat view's main area).</summary>
    void PaintChatPane(float x, float y, float pw, float ph, float radius, bool wide, bool compact)
    {
        var cv = pen.Cv;
        var s = Current();
        chatRect = new Rect4(x, y, pw, ph);
        if (wide) pen.Box(x, y, pw, ph, 0, Rgba.Of(0x0d0c0f));
        else
        {
            pen.Box(x, y, pw, ph, radius, Rgba.Of(0x0e0a12, 0.96));
            pen.Border(x, y, pw, ph, radius, 1, Rgba.Of(0xffffff, 0.1));
        }
        Block(chatRect);
        if (s is null)
        {
            var load = openKey is not null ? "Opening the chat…" : "";
            pen.TextIn(load, chatRect, Pen.F(13, 400, Pen.Faint));
            threadRect = default;
            return;
        }
        cv.PushClip(new Rect4(x, y, pw, ph));
        var hdrH = compact ? 44f : 52f;
        PaintHeader(s, x, y, pw, hdrH, wide, compact);
        pen.Box(x, y + hdrH, pw, 1, 0, Rgba.Of(0xffffff, 0.06));

        var colw = wide ? Math.Min(pw, ColumnMax) : pw;
        var cx0 = x + (pw - colw) / 2;
        var bottom = y + ph;
        var busy = Busy(s);

        // The reply box (or its dock), the question and the note, measured first and drawn from the bottom up.
        float inputH = 0, compH = 0;
        var bw = wide ? Math.Min(pw - 20, ColumnMax) : pw - 20;
        if (compose)
        {
            var look = Pen.F(13, 400, Pen.Ink);
            inputH = Math.Max(24, reply.HeightFor(pen, bw - 10 - 7 - 4, look, 20, 140) + 6);
            compH = 7 + inputH + 3 + 30 + 6;
        }
        var boxH = compose ? compH + 16 : 0;
        float askH = 0;
        var askW = Math.Min(pw - 28, ColumnMax);
        if (s.Ask is { } ask && OpenId is not null) askH = 4 + AskCard(ask, s, false, askW, 0, 0, true) + (compose ? 0 : 8);
        float noteH = 0;
        var noteW = Math.Min(pw - 20, ColumnMax);
        var note = s.Held ? "The queue is held after a stop." : "";
        if (note.Length > 0) noteH = 4 + 8 + pen.WrappedHeight(note, noteW - 16, Pen.F(12, 400, Pen.Ink)) + 6 + 20 + 8 + 2 + 2;

        var threadTop = y + hdrH + 1;
        var threadH = Math.Max(bottom - boxH - askH - noteH - threadTop, 0);
        // Smooth scrolling steps with the clock.
        SyncThread(s, colw);
        threadRect = new Rect4(cx0, threadTop, colw, threadH);
        if (smooth is { } sm)
        {
            scroll = sm.Value(now);
            if (sm.Done(now)) smooth = null; else Need(16);
        }
        if (follow) scroll = MaxScroll;
        scroll = Math.Clamp(scroll, 0, MaxScroll);

        cv.PushClip(new Rect4(x, threadTop, pw, threadH));
        cv.PushMove(cx0, threadTop);
        painter.HoverBar = null;
        painter.Paint(thread, cv, scroll, threadH);
        cv.PopMove();
        // The thread's own scroll bar, at the pane's right edge.
        if (thread.Height > threadH + 1)
        {
            var bar = new Bar(true, x + pw - Bar.Thick, threadTop, threadH, thread.Height, threadH, scroll);
            bar.Paint(cv, barDrag is not null || Over(new Rect4(bar.X, bar.Y, Bar.Thick, bar.Len)), [0, 0, 0, 0]);
            Add(new Rect4(bar.X, bar.Y, Bar.Thick, bar.Len), "vbar");
        }
        cv.PopClip();

        // "Latest": the reader is well above the end of the thread.
        if (MaxScroll - scroll > 240)
        {
            var tw = pen.Width("Latest", Pen.F(11, 400, Pen.Ink));
            var jw = 10 + 12 + 5 + tw + 10;
            var jx = x + (pw - jw) / 2;
            var jy = threadTop + threadH - 24 - (compose ? 12 : 56);
            pen.Box(jx, jy, jw, 24, 12, Rgba.Of(0x1c1a24, 0.95));
            pen.Border(jx, jy, jw, 24, 12, 1, Rgba.Of(0xffffff, 0.1));
            pen.Icon(Glyphs.Down, jx + 10, jy + 6, 12, Pen.Ink);
            pen.Text("Latest", jx + 10 + 12 + 5, jy + (24 - pen.LineHeight(Pen.F(11, 400, Pen.Ink))) / 2, Pen.F(11, 400, Pen.Ink));
            Add(new Rect4(jx, jy, jw, 24), "latest");
        }

        var ny = threadTop + threadH;
        if (note.Length > 0)
        {
            ny += 4;
            var nx = x + (pw - noteW) / 2;
            var th = pen.WrappedHeight(note, noteW - 16, Pen.F(12, 400, Pen.Ink));
            var nh = 8 + th + 6 + 20 + 8 + 2;
            pen.Box(nx, ny, noteW, nh, 12, Rgba.Of(0xffb340, 0.1));
            pen.Border(nx, ny, noteW, nh, 12, 1, Rgba.Of(0xffb340, 0.22));
            pen.Wrapped(note, nx + 8, ny + 8, noteW - 16, Pen.F(12, 400, Rgba.Of(0xffd9a0)));
            var bx = nx + noteW - 8;
            var rw = pen.Width("Resume", Pen.F(11.5, 500, Pen.Ink)) + 20;
            bx -= rw;
            var by = ny + 8 + th + 6;
            var hov = Over(new Rect4(bx, by, rw, 20));
            pen.Box(bx, by, rw, 20, 7, Rgba.Of(0xffffff, hov ? 0.2 : 0.12));
            pen.TextIn("Resume", new Rect4(bx, by, rw, 20), Pen.F(11.5, 500, Pen.Ink));
            Add(new Rect4(bx, by, rw, 20), "resume");
            ny += nh + 2;
        }
        if (s.Ask is { } q && OpenId is not null)
        {
            ny += 4;
            var ax = x + (pw - askW) / 2;
            var ah = AskCard(q, s, false, askW, 0, 0, true);
            AskFrame(q, false, ax, ny, askW, ah);
            AskCard(q, s, false, askW, ax, ny, false);
            ny += ah + (compose ? 0 : 8);
        }
        if (compose) PaintComposer(s, busy, x, ny, pw, bw, compH, inputH);
        else PaintDock(s, x + pw, threadTop + threadH);
        cv.PopClip();

    }

    // MARK: The header

    void PaintHeader(SessionInfo s, float x, float y, float pw, float hdrH, bool wide, bool compact)
    {
        var right = x + pw - (wide ? 10 : 8);
        var by = y + (hdrH - 28) / 2;
        if (!wide)
        {
            IconButton("close", Glyphs.Close, right - 28, by, 15);
            right -= 28 + 6;
        }
        IconButton("more", Glyphs.More, right - 28, by, 16, menu?.Kind == "more");
        right -= 28 + 6;
        var left = !wide ? 16 : (sideShown ? 20 : (listAllowed ? 10 : 12 + (compact ? 68 : 76) + 10));
        var hx = x + left;
        if (wide && !sideShown && listAllowed)
        {
            IconButton("showSide", Glyphs.Sidebar, hx, by, 16);
            hx += 28 + 6;
        }
        var tf = Pen.F(14, 600, Pen.Ink);
        var titleText = s.Title.Length > 0 ? s.Title : "New chat";
        var tw = pen.Width(titleText, tf);
        var maxT = Math.Max(120, (wide ? pw : pw) * 0.46f);
        var tbw = Math.Min(tw, maxT);
        if (renaming)
        {
            tbw = Math.Min(Math.Max(180, tw + 24), maxT + 60);
            pen.Box(hx, by, tbw, 28, 7, Rgba.Of(0xffffff, 0.06));
            pen.Border(hx, by, tbw, 28, 7, 1, Rgba.Of(0xc4a2ff, 0.35));
            var tb = new Rect4(hx + 7, by + (28 - pen.LineHeight(tf)) / 2, tbw - 14, 28);
            boxLayout = title.Paint(pen, tb, tf, "", focus == Focus.Title, now);
            Add(new Rect4(hx, by, tbw, 28), "titleBox");
        }
        else
        {
            var hover = Over(new Rect4(hx, by, tbw, 28));
            pen.Text(titleText, hx, y + (hdrH - pen.LineHeight(tf)) / 2, tf, tbw);
            Add(new Rect4(hx, by, tbw, 28), "rename");
        }
        hx += tbw + 10;
        if (s.Ctx is { } ctx && hx < right)
        {
            var label = ctx + "%";
            var cw = pen.Width(label, Pen.F(11, 500, Pen.Dim)) + 12 + 7 + 7;
            var cy = y + (hdrH - 24) / 2;
            pen.Box(hx, cy, cw, 24, 7, Rgba.Of(0xffffff, 0.05));
            // A small ring that fills as the context does.
            var ring = (float)Math.Clamp(ctx / 100.0, 0, 1);
            pen.Icon("<circle cx=\"12\" cy=\"12\" r=\"9\" opacity=\".25\"/>" + ArcBody(ring), hx + 7, cy + 6, 12, ctx >= 90 ? Pen.Red : ctx >= 70 ? Pen.Amber : Pen.Dim, 3);
            pen.Text(label, hx + 7 + 12 + 5, cy + (24 - pen.LineHeight(Pen.F(11, 500, Pen.Dim))) / 2, Pen.F(11, 500, Pen.Dim));
            hx += cw + 6;
        }
        if (s.Cloud is not null && hx < right)
        {
            var cy = y + (hdrH - 24) / 2;
            var hov = Over(new Rect4(hx, cy, 28, 24));
            pen.Icon(Glyphs.Cloud, hx + 7.5f, cy + 5.5f, 13, hov ? Rgba.Of(0xa9cbff) : Rgba.Of(0x8fbcff));
            Add(new Rect4(hx, cy, 28, 24), "openCloud");
        }
    }

    static string ArcBody(float share)
    {
        if (share <= 0.01f) return "";
        if (share >= 0.999f) return "<circle cx=\"12\" cy=\"12\" r=\"9\"/>";
        var a = 2 * Math.PI * share;
        double ex = 12 + 9 * Math.Sin(a), ey = 12 - 9 * Math.Cos(a);
        var f = System.Globalization.CultureInfo.InvariantCulture;
        return string.Create(f, $"<path d=\"M12 3A9 9 0 {(share > 0.5f ? 1 : 0)} 1 {ex:0.###} {ey:0.###}\"/>");
    }

    /// <summary>A 28 px square button with an icon; it lights under the pointer.</summary>
    void IconButton(string id, string body, float x, float y, float iconSize, bool active = false, int n = 0)
    {
        var r = new Rect4(x, y, 28, 28);
        var hov = Over(r) || active;
        pen.Box(r, 7, hov ? Rgba.Of(0xffffff, 0.043) : Rgba.Clear);
        pen.Icon(body, x + (28 - iconSize) / 2, y + (28 - iconSize) / 2, iconSize, hov ? Pen.Ink : Rgba.Of(0xf6f2ff, 0.5));
        Add(r, id, n);
    }

    // MARK: The reply box

    void PaintDock(SessionInfo s, float right, float bottom)
    {
        var draft = reply.Text.Length > 0;
        var r = new Rect4(right - 34 - 10, bottom - 34 - 10, 34, 34);
        var hov = Over(r);
        pen.Box(r, 17, hov ? Rgba.Of(0x282230, 0.62) : Rgba.Of(0x1c1822, 0.32));
        pen.Border(r.X, r.Y, r.W, r.H, 17, 1, Rgba.Of(0xffffff, 0.1));
        pen.Icon(Glyphs.Chat, r.X + 9, r.Y + 9, 16, Pen.Ink);
        if (draft)
        {
            pen.Box(r.X + r.W - 9, r.Y - 1, 10, 10, 5, Pen.Lilac);
        }
        Add(r, "compose");
    }

    void PaintComposer(SessionInfo s, bool busy, float x, float y, float pw, float bw, float compH, float inputH)
    {
        var bx = x + (pw - bw) / 2;
        var by = y + 6;
        var foc = focus == Focus.Reply;
        pen.Box(bx, by, bw, compH, 17, Rgba.Of(0x1b181f, 0.94));
        pen.Border(bx, by, bw, compH, 17, 1, foc ? Rgba.Of(0xc4a2ff, 0.55) : Rgba.Of(0xffffff, 0.1));
        Add(new Rect4(bx, by, bw, compH), "composer");
        var cy = by + 7;
        var cx = bx + 10;
        var iw = bw - 10 - 7;
        var look = Pen.F(13, 400, Pen.Ink);
        var area = new Rect4(cx + 2, cy + 3, iw - 4, inputH - 3);
        boxLayout = null;
        var shaped = pen.Shape(reply.Text.Length == 0 ? " " : reply.Text, look, area.W);
        replyScroll = reply.ScrollFor(shaped, area.H, replyScroll);
        boxLayout = reply.Paint(pen, area, look, "Reply to " + Bots.Name(s.Bot) + "…", foc, now, replyScroll);
        replyArea = area;
        Add(area, "replyText");
        cy += inputH + 3;
        // The bar: the folder's name, the model, and the round button.
        var bx2 = cx;
        if (s.Folder.Length > 0)
        {
            var name = LastPart(s.Folder);
            var fl = Pen.F(12.5, 400, Pen.Dim);
            var fw = Math.Min(Math.Min(240, iw * 0.3f), 9 + 13 + 6 + pen.Width(name, fl) + 9);
            pen.Icon(Glyphs.Folder, bx2 + 9, cy + 1 + 7.5f, 13, Pen.Dim);
            pen.Text(name, bx2 + 9 + 13 + 6, cy + 1 + (28 - pen.LineHeight(fl)) / 2, fl, Math.Max(fw - 9 - 13 - 6 - 9, 0));
            bx2 += fw + 4;
        }
        var empty = reply.Text.Trim().Length == 0;
        var enabled = !empty || (busy && !s.Stopping);
        var pause = empty && busy;
        var gx = cx + iw - 30;
        var gr = new Rect4(gx, cy, 30, 30);
        var ghov = Over(gr) && enabled;
        pen.Box(gr, 15, !enabled ? Rgba.Of(0xffffff, 0.07) : ghov ? Rgba.Of(0xffffff) : Pen.Ink);
        if (pause) pen.Icon(Glyphs.Pause, gx + 7, cy + 7, 16, Rgba.Of(0x131116), 3);
        else pen.Icon(Glyphs.Send, gx + 7, cy + 7, 16, enabled ? Rgba.Of(0x0c0b0e) : Rgba.Of(0xf6f2ff, 0.28), 2.4f);
        Add(gr, enabled ? (pause ? "pause" : "send") : "sendOff");
        var rx = gx - 4;
        if (s.Stopping)
        {
            var sl = Pen.F(11.5, 400, Rgba.Of(0xffc46b));
            var sw = pen.Width("Stopping…", sl);
            pen.Text("Stopping…", rx - sw, cy + (30 - pen.LineHeight(sl)) / 2, sl);
        }
        else if (busy)
        {
            var sl = Pen.F(11.5, 400, Pen.Faint);
            var label = reply.Text.Trim().Length > 0 ? "Sends after this run" : "";
            var sw = pen.Width(label, sl);
            if (sw > 0) pen.Text(label, rx - sw, cy + (30 - pen.LineHeight(sl)) / 2, sl);
        }
    }

    Rect4 replyArea;

    static string LastPart(string folder)
    {
        var t = folder.TrimEnd('\\', '/');
        var i = t.LastIndexOfAny(['\\', '/']);
        return i >= 0 ? t[(i + 1)..] : t;
    }

    // MARK: Sending, pausing, queueing

    void SendReply()
    {
        var s = Current();
        if (s is null || OpenId is not { } id) return;
        var text = reply.Text.Trim();
        if (text.Length == 0)
        {
            // Pause, not Stop: the tool cancels the turn and the conversation stays.
            if (Busy(s) && !s.Stopping) { Trace($"pause {id}"); env.Send(new PauseCommand(id)); }
            return;
        }
        if (s.Ask?.Questions is { Count: > 0 } qs)
        {
            if (qs.Count == 1 && qs[0].Custom)
            {
                env.Send(new AnswerCommand(id, s.Ask.Id, null, [[text]]));
                reply.Clear();
                SaveDraft();
                compose = false;
                focus = Focus.None;
                return;
            }
            Say("Answer the question above first, or skip it.");
            return;
        }
        Trace($"reply to {id}");
        env.Send(new ReplyCommand(text, id));
        reply.Clear();
        SaveDraft();
        compose = false;
        focus = Focus.None;
        follow = true;
        Dirty = true;
    }

    void CommitRename()
    {
        var s = Current();
        renaming = false;
        focus = Focus.None;
        if (s is null || OpenId is not { } id) return;
        var name = title.Text.Trim();
        if (name.Length > 0 && name != s.Title) env.Send(new RenameCommand(id, name));
    }

    // MARK: The thread's pointer

    void ThreadDown(float x, float y, int clicks, UiMods mods)
    {
        focus = Focus.None;
        var tx = x - threadRect.X;
        var ty = y - threadRect.Y + scroll;
        var hit = thread.Hit(tx, ty);
        switch (hit.Kind)
        {
            case HitKind.Act:
                ThreadAct(hit);
                return;
            case HitKind.Link:
                dragging = true;
                dragLink = hit.Link;
                thread.ClearSelection();
                return;
            case HitKind.Text:
                dragging = true;
                dragLink = null;
                dragUnit = clicks >= 3 ? Unit.Para : clicks == 2 ? Unit.Word : Unit.Char;
                dragAnchor = hit.Pos;
                if (dragUnit == Unit.Char) thread.Select(hit.Pos, hit.Pos);
                else
                {
                    (dragA0, dragA1, dragTail) = thread.UnitAt(hit.Pos, dragUnit);
                    if (dragUnit == Unit.Word) dragA1 = thread.TrailingSpace(dragA1);
                    thread.SelectUnits(dragA0, dragA1, dragTail, hit.Pos, dragUnit);
                }
                return;
            default:
                thread.ClearSelection();
                return;
        }
    }

    void DragTo(float x, float y)
    {
        if (dragLink is not null) return;
        var tx = x - threadRect.X;
        var ty = Math.Clamp(y, threadRect.Y - 40, threadRect.Bottom + 40) - threadRect.Y + scroll;
        var hit = thread.Hit(tx, ty);
        if (hit.Kind is HitKind.Text or HitKind.Link or HitKind.None && hit.Kind != HitKind.None)
        {
            var pos = hit.Kind == HitKind.Text ? hit.Pos : dragAnchor;
            if (dragUnit == Unit.Char) thread.Select(dragAnchor, pos);
            else thread.SelectUnits(dragA0, dragA1, dragTail, pos, dragUnit);
        }
        // Dragging past an edge moves the thread.
        if (y < threadRect.Y + 8) ScrollBy(-30);
        else if (y > threadRect.Bottom - 8) ScrollBy(30);
    }

    void OpenLink(string url)
    {
        if (url.StartsWith("http://", StringComparison.OrdinalIgnoreCase) || url.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
            env.OpenUrl?.Invoke(url);
        else Say("Only web addresses open from the chat.");
    }

    void ThreadAct(Hit hit)
    {
        var a = hit.Act;
        var section = hit.Section;
        Trace($"thread {a.Kind} in turn {section}");
        var s = Current();
        switch (a.Kind)
        {
            case ActKind.Copy:
                env.Clipboard.Set(a.Text);
                copiedAt = now;
                thread.SetCopied(turns, a.Text);
                break;
            case ActKind.Step:
                thread.ToggleStep(turns, section, a.I, a.Now);
                break;
            case ActKind.Group:
                thread.ToggleGroup(turns, section, a.I);
                break;
            case ActKind.Flag:
                thread.ToggleFlag(turns, section, a.I, (int)a.K);
                break;
            case ActKind.OpenDiff:
                if (thread.OpenDiff(turns, section, a.I) is { } at) ScrollTo(at - 40);
                break;
            case ActKind.Retry:
                if (OpenId is { } rid && section < turns.Count) env.Send(new ReplyCommand(turns[section].Prompt, rid));
                follow = true;
                break;
            case ActKind.Restore:
                confirm = (section, "restore");
                break;
            case ActKind.TryAgain:
                confirm = (section, "again");
                break;
            case ActKind.Edit:
                if (OpenId is { } eid && section < turns.Count) env.Send(new QueueRemoveCommand(eid, turns[section].Uid));
                break;
            case ActKind.SendNow:
                if (OpenId is { } sid && section < turns.Count) env.Send(new QueueSendNowCommand(sid, turns[section].Uid));
                break;
            case ActKind.EditPrompt:
                if (section < turns.Count)
                {
                    reply.SetText(turns[section].Prompt);
                    compose = true;
                    focus = Focus.Reply;
                }
                break;
        }
        Dirty = true;
        _ = s;
    }

    // MARK: Rewind confirmation

    void PaintConfirm()
    {
        if (confirm is not { } c) return;
        var cv = pen.Cv;
        pen.Box(0, 0, w, h, 0, Rgba.Of(0x000000, 0.5));
        var bw = Math.Min(360, w - 32);
        var head = c.Kind switch { "restore" => "Restore to after this answer?", "delete" => "Delete this chat?", _ => "Try this message again?" };
        var body = c.Kind switch
        {
            "restore" => "The chat goes back to how it was after this answer, and the folder goes back with it. Later messages are removed.",
            "delete" => "The chat is removed from the list and its saved history is deleted. This cannot be undone.",
            _ => "The chat goes back to just before this message, the folder with it, and the message is sent again.",
        };
        var hf = Pen.F(14, 600, Pen.Ink);
        var bf = Pen.F(12, 400, Pen.Dim);
        var bh = pen.WrappedHeight(body, bw - 36, bf);
        var boxH = 18 + pen.LineHeight(hf) + 8 + bh + 16 + 30 + 18;
        var bx = (w - bw) / 2;
        var by = (h - boxH) / 2;
        confirmBox = new Rect4(bx, by, bw, boxH);
        pen.Box(bx, by, bw, boxH, 14, Rgba.Of(0x14101a));
        pen.Border(bx, by, bw, boxH, 14, 1, Rgba.Of(0xffffff, 0.12));
        pen.Text(head, bx + 18, by + 18, hf);
        pen.Wrapped(body, bx + 18, by + 18 + pen.LineHeight(hf) + 8, bw - 36, bf);
        var yb = by + boxH - 18 - 30;
        var yes = c.Kind switch { "restore" => "Restore", "delete" => "Delete", _ => "Try again" };
        var yw = pen.Width(yes, Pen.F(12, 600, Pen.Ink)) + 28;
        var cw = pen.Width("Cancel", Pen.F(12, 600, Pen.Ink)) + 28;
        var yr = new Rect4(bx + bw - 18 - yw, yb, yw, 30);
        var cr = new Rect4(yr.X - 8 - cw, yb, cw, 30);
        pen.Box(cr, 8, Rgba.Of(0xffffff, Over(cr) ? 0.17 : 0.1));
        pen.TextIn("Cancel", cr, Pen.F(12, 600, Pen.Ink));
        pen.Box(yr, 8, Over(yr) ? Rgba.Of(0xffffff) : Rgba.Of(0xf5f5f7));
        pen.TextIn(yes, yr, Pen.F(12, 600, Rgba.Of(0x000000)));
        Add(confirmBox, "confirmBox");
        Add(cr, "confirmNo");
        Add(yr, "confirmYes");
        _ = cv;
    }
}
