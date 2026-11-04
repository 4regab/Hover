using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

public enum Focus { None, Reply, Task, Title, Custom, Find, Desk }

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
    float replyScroll, replyMax;
    (int Version, int Caret) replyFollowed;
    Rect4 composerRect;
    (int Turn, string Kind)? confirm;
    Rect4 confirmBox;

    EditBox? FocusedBox() => focus switch
    {
        Focus.Reply when compose => reply,
        Focus.Task when taskOpen || ChatView => task,
        Focus.Title when renaming => title,
        Focus.Custom => customBox,
        Focus.Find => repoFind,
        Focus.Desk => deskField?.Box,
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

    /// <summary>The width the thread was last laid out at, to see a change of it.</summary>
    float threadW;

    void SyncThread(SessionInfo s, float colw, float viewH)
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
        // A new width keeps the reader at the same place: the turn at the top of the view and how far down it (as a share of the
        // turn), not the same number of pixels, which are a different part of the chat once the lines wrap otherwise
        // (internal/shell/chat.go). A reader near the end stays at the end.
        var anchor = -1;
        var frac = 0f;
        var near = thread.Height - scroll - viewH < 40;
        var widthChanged = threadW > 0 && Math.Abs(threadW - colw) > 0.01f && !changed;
        if (widthChanged && !near)
        {
            for (var i = 0; i < thread.Sections.Count; i++)
            {
                var sec = thread.Sections[i];
                if (sec.Y + sec.H <= scroll) continue;
                anchor = i;
                frac = (scroll - sec.Y) / Math.Max(sec.H, 1);
                break;
            }
        }
        thread.Set(turns, colw);
        threadW = colw;
        if (anchor >= 0 && anchor < thread.Sections.Count)
        {
            scroll = thread.Sections[anchor].Y + frac * thread.Sections[anchor].H;
            smooth = null;
            follow = false;
        }
        else if (widthChanged && near) follow = true;
        // The running step shimmers and the clock counts: ten frames a second, as the Go chat repaints its thread.
        if (turns.Any(t => t.Live)) Need(100);
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
            pen.Box(x, y, pw, ph, radius, Rgba.Of(0x0e0a12, 0.985));
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
            compH = 7 + ChipsHeight(bw - 10 - 7 - 2) + ShotsHeight(false) + inputH + 3 + 30 + 6;
        }
        var boxH = compose ? compH + 16 : 0;
        float askH = 0, askFull = 0, askCard = 0;
        var askW = Math.Min(pw - 28, ColumnMax);
        float noteH = 0;
        var noteW = Math.Min(pw - 20, ColumnMax);
        var note = s.Held ? "The queue is held after a stop." : "";
        if (note.Length > 0) noteH = 4 + 8 + pen.HeightWhenWrapped(note, noteW - 16, Pen.F(12, 400, Pen.Ink)) + 6 + 20 + 8 + 2 + 2;
        var threadTop = y + hdrH + 1;
        if (s.Ask is { } ask && OpenId is not null)
        {
            askFull = AskCard(ask, s, false, askW, 0, 0, true);
            // A question taller than the room scrolls inside its card; the thread keeps 80 px at least.
            var room = bottom - boxH - noteH - threadTop - 4 - (compose ? 0 : 8) - 80;
            askCard = Math.Min(askFull, Math.Max(room, 96f));
            askH = 4 + askCard + (compose ? 0 : 8);
        }

        var threadH = Math.Max(bottom - boxH - askH - noteH - threadTop, 0);
        // Smooth scrolling steps with the clock.
        SyncThread(s, colw, threadH);
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
        painter.Time = now / 1000;
        painter.HoverBar = HBarAt(px, py) is { } hb ? (hb.Section, hb.Scroller) : hDrag is { } hd ? (hd.Section, hd.Scroller) : null;
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
        if (MaxScroll - scroll > 160)
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
            var th = pen.HeightWhenWrapped(note, noteW - 16, Pen.F(12, 400, Pen.Ink));
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
            var ah = askCard;
            AskFrame(q, false, ax, ny, askW, ah);
            PaintAskScrolled(q, s, askFull, new Rect4(ax, ny, askW, ah));
            ny += ah + (compose ? 0 : 8);
        }
        if (compose) PaintComposer(s, busy, x, ny, pw, bw, compH, inputH);
        // The reply circle needs room under the header: a question that fills the pane takes it away (the question is answered first).
        else if (threadH >= 56) PaintDock(s, x + pw, threadTop + threadH);
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
            // Kiro's auto compact: a short line across the ring where the chat is compacted (internal/ui/officechat.go headChip).
            if (s.Tool == "kiro" && kiroAutoCompact && kiroCompactAt is > 0 and <= 100) pen.Icon(TickBody(kiroCompactAt), hx + 7, cy + 6, 12, Rgba.Of(0x7fe3d4), 2);
            pen.Text(label, hx + 7 + 12 + 5, cy + (24 - pen.LineHeight(Pen.F(11, 500, Pen.Dim))) / 2, Pen.F(11, 500, Pen.Dim));
            hx += cw + 6;
        }
        if (HasWebPage(s) && hx < right)
        {
            var cy = y + (hdrH - 24) / 2;
            var hov = Over(new Rect4(hx, cy, 28, 24));
            pen.Icon(Glyphs.Cloud, hx + 7.5f, cy + 5.5f, 13, hov ? Rgba.Of(0xa9cbff) : Rgba.Of(0x8fbcff));
            Add(new Rect4(hx, cy, 28, 24), "openCloud");
        }
    }

    /// <summary>A short line across the context ring, turned clockwise from the top by the share (the 24-unit box of <see cref="ArcBody"/>).</summary>
    static string TickBody(int at)
    {
        var a = 2 * Math.PI * at / 100;
        double sa = Math.Sin(a), ca = Math.Cos(a);
        var f = System.Globalization.CultureInfo.InvariantCulture;
        return string.Create(f, $"<path d=\"M{12 + 6.5 * sa:0.###} {12 - 6.5 * ca:0.###}L{12 + 11.5 * sa:0.###} {12 - 11.5 * ca:0.###}\"/>");
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
        var draft = reply.Text.Length > 0 || ShotsOf(s.Id).Count > 0 || ChipsOf(s.Id).Count > 0;
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
        var shots = ShotsOf(id).ToList();
        var attached = ChipsOf(id).ToList();
        if (text.Length == 0 && shots.Count == 0 && attached.Count == 0)
        {
            // Pause, not Stop: the tool cancels the turn and the conversation stays.
            if (Busy(s) && !s.Stopping) { Trace($"pause {id}"); env.Send(new PauseCommand(id)); }
            return;
        }
        if (s.Ask?.Questions is { Count: > 0 } qs)
        {
            if (qs.Count == 1 && qs[0].Custom && shots.Count == 0)
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
        env.Send(new ReplyCommand(text, id)
        {
            Images = shots.Count > 0 ? [.. shots] : null,
            Chips = attached.Count > 0 ? [.. attached.Select(c => c.ToJson())] : null,
        });
        if (shots.Count > 0 || attached.Count > 0) lastReply = (now, id, text, shots, attached);
        chips.Remove(id);
        replyShots.Remove(id);
        PopClose();
        reply.Clear();
        SaveDraft();
        compose = false;
        focus = Focus.None;
        follow = true;
        Dirty = true;
    }

    /// <summary>Words go into the reply box after what is already written there, on a line of their own (the Go box: "draft", a break, the words); the box opens with the keyboard in it.</summary>
    void PutInReplyBox(string words)
    {
        reply.SetText(reply.Text.Trim().Length > 0 ? reply.Text + "\n" + words : words);
        SaveDraft();
        compose = true;
        focus = Focus.Reply;
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

    // MARK: The sideways bars of wide code boxes and tables

    /// <summary>The box (code, table) whose own bar is under the point, in the panels' coordinates, and the bar as drawn.</summary>
    (int Section, int Scroller, Bar Bar, float Along)? HBarAt(float x, float y)
    {
        if (!OpenCurrent() || !threadRect.Contains(x, y)) return null;
        foreach (var (section, scroller, bx, by, len, content, pos) in thread.HBars())
        {
            var bar = new Bar(false, threadRect.X + bx, threadRect.Y + by - scroll, len, content, len, pos);
            if (bar.Max > 0 && bar.Along(x, y) is { } along) return (section, scroller, bar, along);
        }
        return null;
    }

    (int Section, int Scroller, float Grab)? hDrag;

    /// <summary>A press on a box's own bar: the end buttons and the track step it, the thumb is taken. True when the press was on a bar.</summary>
    bool HBarDown(float x, float y)
    {
        if (HBarAt(x, y) is not { } hit) return false;
        var part = hit.Bar.PartAt(hit.Along);
        if (part == Bar.Part.Thumb) hDrag = (hit.Section, hit.Scroller, hit.Along - hit.Bar.Thumb().Start);
        else thread.ScrollBox(hit.Section, hit.Scroller, hit.Bar.Step(part, hit.Along));
        Dirty = true;
        return true;
    }

    /// <summary>The thumb of a box's bar follows the pointer.</summary>
    void HBarDrag(float x)
    {
        if (hDrag is not { } d) return;
        foreach (var (section, scroller, bx, by, len, content, pos) in thread.HBars())
        {
            if (section != d.Section || scroller != d.Scroller) continue;
            var bar = new Bar(false, threadRect.X + bx, threadRect.Y + by - scroll, len, content, len, pos);
            thread.ScrollBox(section, scroller, bar.Drag(d.Grab, x - bar.X));
            Dirty = true;
        }
    }

    /// <summary>The wheel turned sideways (or with Shift) over a code box or a table: it scrolls that box. False when there is none under the point.</summary>
    public bool WheelSideways(float x, float y, float delta)
    {
        if (!OpenCurrent() || !threadRect.Contains(x, y)) return false;
        if (thread.BoxAt(x - threadRect.X, y - threadRect.Y + scroll) is not { } box) return false;
        var bar = thread.HBars().FirstOrDefault(b => b.Section == box.Section && b.Scroller == box.Scroller);
        if (bar.Content <= bar.Len) return false;
        thread.ScrollBox(box.Section, box.Scroller, bar.Pos - delta);
        Dirty = true;
        return true;
    }

    void ThreadDown(float x, float y, int clicks, UiMods mods)
    {
        if (HBarDown(x, y)) return;
        // A press on the thread takes the keyboard from the reply box, and an empty reply box closes (internal/ui/officechat.go).
        if (compose && reply.Text.Length == 0 && Current() is { } cur && ShotsOf(cur.Id).Count == 0 && ChipsOf(cur.Id).Count == 0) { PopClose(); compose = false; }
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
                // Shift and a press grows the selection that is there from its anchor (the Go thread does too).
                if ((mods & UiMods.Shift) != 0 && thread.HasSel)
                {
                    dragUnit = Unit.Char;
                    dragAnchor = thread.SelAnchor;
                    thread.Select(dragAnchor, hit.Pos);
                    return;
                }
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
                // The newest turn's prompt again, with its pictures; never while a run is in flight, a message waits, or for an older turn.
                if (OpenId is { } rid && s is not null && section == turns.Count - 1 && !Busy(s) && !turns.Any(t => t.Queued))
                {
                    var again = turns[section];
                    env.Send(new ReplyCommand(again.Prompt, rid) { Images = again.Images.Count > 0 ? [.. again.Images] : null });
                    follow = true;
                }
                break;
            case ActKind.Restore:
            case ActKind.TryAgain:
                // Both change the project's files, so they ask first; never while a run goes (internal/shell/chatact.go askRewind).
                if (s is not null && Busy(s)) Say("Stop the run first.");
                else confirm = (section, a.Kind == ActKind.Restore ? "restore" : "again");
                break;
            case ActKind.Edit:
                if (OpenId is { } eid && section < turns.Count) env.Send(new QueueRemoveCommand(eid, turns[section].Uid));
                break;
            case ActKind.SendNow:
                if (OpenId is { } sid && section < turns.Count) env.Send(new QueueSendNowCommand(sid, turns[section].Uid));
                break;
            case ActKind.EditPrompt:
                if (section < turns.Count) PutInReplyBox(turns[section].Prompt);
                break;
        }
        Dirty = true;
        _ = s;
    }

    // MARK: Rewind confirmation

    /// <summary>What Restore and Try again ask, in the Go app's words (internal/shell/chatact.go askRewind).</summary>
    string RewindWords(bool restore, int turn)
    {
        var s = Current();
        var folder = s is null ? "" : LastPart(s.Folder);
        var later = Math.Max((s?.Turns.Count ?? 0) - (turn + 1), 0);
        var leave = later == 1 ? "1 message after it leaves" : $"{later} messages after it leave";
        const string undone = " Changes made since, by the agent or by you, are undone.";
        if (restore)
            return $"The files in “{folder}” go back to how they were after this answer" + (later > 0 ? $", and the {leave} this chat." : ".") + undone;
        return $"The files in “{folder}” go back to how they were before this message, and it is sent again." + (later > 0 ? $" The {leave} this chat." : "") + undone;
    }

    void PaintConfirm()
    {
        if (confirm is not { } c) return;
        var cv = pen.Cv;
        pen.Box(0, 0, w, h, 0, Rgba.Of(0x000000, 0.5));
        var bw = Math.Min(360, w - 32);
        var head = c.Kind switch { "restore" => "Restore to here?", "delete" or "deleteKey" or "deleteLive" => "Delete this chat?", _ => "Try again from here?" };
        var body = c.Kind switch
        {
            "restore" or "again" => RewindWords(c.Kind == "restore", c.Turn),
            _ => "The chat is removed from the list and its saved history is deleted. This cannot be undone.",
        };
        var hf = Pen.F(14, 600, Pen.Ink);
        var bf = Pen.F(12, 400, Pen.Dim);
        var bh = pen.HeightWhenWrapped(body, bw - 36, bf);
        var boxH = 18 + pen.LineHeight(hf) + 8 + bh + 16 + 30 + 18;
        var bx = (w - bw) / 2;
        var by = (h - boxH) / 2;
        confirmBox = new Rect4(bx, by, bw, boxH);
        pen.Box(bx, by, bw, boxH, 14, Rgba.Of(0x14101a));
        pen.Border(bx, by, bw, boxH, 14, 1, Rgba.Of(0xffffff, 0.12));
        pen.Text(head, bx + 18, by + 18, hf);
        pen.Wrapped(body, bx + 18, by + 18 + pen.LineHeight(hf) + 8, bw - 36, bf);
        var yb = by + boxH - 18 - 30;
        var yes = c.Kind switch { "restore" => "Restore", "delete" or "deleteKey" or "deleteLive" => "Delete", _ => "Try again" };
        var yw = pen.Width(yes, Pen.F(12, 600, Pen.Ink)) + 28;
        var cw = pen.Width("Cancel", Pen.F(12, 600, Pen.Ink)) + 28;
        var yr = new Rect4(bx + bw - 18 - yw, yb, yw, 30);
        var cr = new Rect4(yr.X - 8 - cw, yb, cw, 30);
        pen.Box(cr, 8, Rgba.Of(0xffffff, Over(cr) ? 0.17 : 0.1));
        pen.TextIn("Cancel", cr, Pen.F(12, 600, Pen.Ink));
        // The Go dialog's button is red with white words (internal/ui/officemenus.go confirm), for every kind.
        pen.Box(yr, 8, Over(yr) ? Rgba.Of(0xff453a) : new Rgba(0xff, 0x45, 0x3a, 0xd9));
        pen.TextIn(yes, yr, Pen.F(12, 600, Rgba.Of(0xffffff)));
        Add(confirmBox, "confirmBox");
        Add(cr, "confirmNo");
        Add(yr, "confirmYes");
        _ = cv;
    }
}
