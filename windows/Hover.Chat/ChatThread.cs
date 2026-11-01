using Hover.Host.Protocol;

namespace Hover.Chat;

/// <summary>One turn laid out.</summary>
public sealed class Section
{
    public float Y, H;
    public Frag Frag = new();
    /// <summary>Where the answer's copy tokens start (a select-all inside the answer).</summary>
    public int AnswerTok = -1;
    /// <summary>Where the answer's texts, shapes and scrolling boxes start (it fades in on its own).</summary>
    public (int Texts, int Shapes, int Scrollers)? AnswerAt;
    /// <summary>The images the section shows, and whether it must be laid out again.</summary>
    public List<string> Images = [];
    public bool Stale;
    /// <summary>Something here is plain text because the backend has not answered yet.</summary>
    public bool Waiting;
    internal (int Turn, float Width, bool Live, int User, bool Last, long Flags) Key;
    internal int Signature;
}

/// <summary>A place in the text: a section, a text box in it, and an index of the box's text.</summary>
public readonly record struct Pos(int Section, int Text, int Index) : IComparable<Pos>
{
    public int CompareTo(Pos o)
    {
        if (Section != o.Section) return Section.CompareTo(o.Section);
        if (Text != o.Text) return Text.CompareTo(o.Text);
        return Index.CompareTo(o.Index);
    }

    public static bool operator <(Pos a, Pos b) => a.CompareTo(b) < 0;
    public static bool operator >(Pos a, Pos b) => a.CompareTo(b) > 0;
}

/// <summary>The whole chat, laid out.</summary>
public sealed partial class ChatThread
{
    readonly IShaper sh;
    readonly IRenderSource source;

    public float Width { get; private set; } = 360;
    /// <summary>The agent's name over each answer, and its tool for the mark beside it.</summary>
    public string Who { get; set; }
    public string Tool { get; set; } = "kiro";
    public List<Section> Sections { get; private set; } = [];
    public float Height { get; private set; }
    /// <summary>Room kept under the last turn (the reply circle sits over the thread's corner).</summary>
    public float ExtraBottom { get; set; }
    public bool HideSteps { get; set; }
    /// <summary>The session shown (its id in the state message). Each session keeps its own opened and folded steps.</summary>
    public int Session { get; set; }
    /// <summary>The folder the answers belong to, and the host their files are served from (for pictures in Markdown).</summary>
    public string Folder { get; set; } = "";
    public string Files { get; set; } = "";
    /// <summary>The code just copied: its button says "Copied" for a moment.</summary>
    public string? Copied { get; private set; }
    /// <summary>A command is running: its time counts, so the app lays the thread out again each second.</summary>
    public bool Ticking { get; private set; }
    /// <summary>How many sections were laid out since this was made (for the tests).</summary>
    public int Relayouts { get; private set; }
    /// <summary>The clock, replaceable in tests.</summary>
    public Func<DateTime> Clock { get; set; } = () => DateTime.UtcNow;

    /// <summary>The selection: anchor and focus, and what it takes in after its last box.</summary>
    public Pos SelAnchor { get; private set; }
    public Pos SelFocus { get; private set; }
    public bool HasSel { get; private set; }
    public Tail SelTail { get; private set; }

    readonly Dictionary<(int Session, int Turn, int Step, bool Now), bool> stepUser = [];
    readonly Dictionary<(int Session, int Turn, int First), bool> groupUser = [];
    readonly Dictionary<(int Session, int Turn, int Step, int Which), bool> flags = [];
    readonly Dictionary<(int Session, int Turn, int Step), DateTime> began = [];
    /// <summary>How far each sideways-scrolling box is scrolled, by (section, scroller). A section laid out again starts at 0.</summary>
    public Dictionary<(int Section, int Scroller), float> HScroll { get; } = [];

    public ChatThread(IShaper shaper, IRenderSource source, string who)
    {
        sh = shaper;
        this.source = source;
        Who = who;
    }

    // MARK: Opening and folding

    bool Flag(int turn, int step, int which) => flags.GetValueOrDefault((Session, turn, step, which));

    /// <summary>Flips a step's switch (a long change shown whole, more subagents, one's result).</summary>
    public void ToggleFlag(IReadOnlyList<Turn> turns, int section, int step, int which)
    {
        var key = (Session, section, step, which);
        if (!flags.Remove(key)) flags[key] = true;
        Lay(turns, Width);
    }

    /// <summary>A file under the answer was clicked: the timeline opens on its change. Returns where its row is now, in thread coordinates.</summary>
    public float? OpenDiff(IReadOnlyList<Turn> turns, int section, int step)
    {
        if (section < turns.Count)
        {
            foreach (var g in StepGroups(turns[section].Steps))
                if (step >= g.A && step < g.B) groupUser[(Session, section, g.A)] = true;
        }
        stepUser[(Session, section, step, false)] = true;
        Lay(turns, Width);
        if (section >= Sections.Count) return null;
        var s = Sections[section];
        foreach (var h in s.Frag.Hits)
            if (h.Act.Kind == ActKind.Step && h.Act.I == step) return s.Y + h.R.Y;
        return null;
    }

    /// <summary>A step's block (change, output, thought) is open while its turn runs, so what the agent is doing stays in view, and folded once the turn has ended.</summary>
    static bool StepDefaultOpen(Turn t) => t.Live;

    /// <summary>The user's own choice wins, during the turn and after it. The "now" row and the timeline's row show the same block, so a choice made on one holds on the other.</summary>
    bool StepOpen(Turn t, int ti, int j, bool now)
    {
        if (stepUser.TryGetValue((Session, ti, j, now), out var v)) return v;
        if (stepUser.TryGetValue((Session, ti, j, !now), out v)) return v;
        return StepDefaultOpen(t);
    }

    /// <summary>Splits a turn's steps where the agent spoke: [first, end) of each run of steps.</summary>
    internal static List<(int A, int B)> StepGroups(IReadOnlyList<Step> steps)
    {
        var groups = new List<(int, int)>();
        for (var j = 0; j < steps.Count; j++)
        {
            if (j == 0 || steps[j].Lead.Trim().Length > 0) groups.Add((j, j + 1));
            else groups[^1] = (groups[^1].Item1, j + 1);
        }
        return groups;
    }

    /// <summary>Whether steps [a, b) of a turn show their rows: the user's choice, else open while the turn runs and folded once it has ended, unless the user opened one of its steps.</summary>
    bool GroupOpen(Turn t, int ti, int a, int b)
    {
        if (groupUser.TryGetValue((Session, ti, a), out var v)) return v;
        if (StepDefaultOpen(t)) return true;
        foreach (var (k, open) in stepUser)
            if (open && k.Session == Session && k.Turn == ti && k.Step >= a && k.Step < b) return true;
        return false;
    }

    /// <summary>A click on a group's header: it opens or folds, and stays that way.</summary>
    public void ToggleGroup(IReadOnlyList<Turn> turns, int ti, int first)
    {
        if (ti >= turns.Count) return;
        foreach (var g in StepGroups(turns[ti].Steps))
            if (g.A == first) groupUser[(Session, ti, first)] = !GroupOpen(turns[ti], ti, g.A, g.B);
        Lay(turns, Width);
    }

    /// <summary>A click on a step with a change or output: its block opens or folds.</summary>
    public void ToggleStep(IReadOnlyList<Turn> turns, int section, int step, bool now)
    {
        var open = StepOpen(turns[section], section, step, now);
        stepUser[(Session, section, step, now)] = !open;
        Lay(turns, Width);
    }

    /// <summary>A Copy was clicked: its button says so until the copy is done with.</summary>
    public void SetCopied(IReadOnlyList<Turn> turns, string? text)
    {
        var was = Copied;
        Copied = text;
        // Only a section with a Copy button for the text that was copied, or the one that is,
        // says "Copied" (or stops saying it).
        bool Offers(Section s, string? t) => t is not null && s.Frag.Hits.Any(h => h.Act.Kind == ActKind.Copy && h.Act.Text == t);
        foreach (var s in Sections)
            if (Offers(s, was) || Offers(s, text)) s.Stale = true;
        Lay(turns, Width);
    }

    /// <summary>Something the backend or the loader was asked for has arrived: the sections that showed plain text are laid out again on the next <see cref="Set"/>.</summary>
    public bool Refresh(string? image = null)
    {
        var any = false;
        foreach (var s in Sections)
        {
            if (s.Waiting || (image is not null && s.Images.Contains(image)))
            {
                s.Stale = true;
                any = true;
            }
        }
        return any;
    }

    // MARK: Laying out

    /// <summary>Lays the thread out for the drawer's width, re-using every section whose turn, width and step-list state have not changed.</summary>
    public void Set(IReadOnlyList<Turn> turns, float width) => Lay(turns, width);

    void Lay(IReadOnlyList<Turn> turns, float width)
    {
        Width = width;
        Ticking = false;
        foreach (var t in turns)
        {
            var n = t.Steps.Count;
            if (t.Live && n > 0 && t.Steps[n - 1].Kind == StepIcon.Run && t.Steps[n - 1].Cmd.Length > 0 && !t.Steps[n - 1].Ended) Ticking = true;
        }
        var old = Sections;
        Sections = [];
        var pad = Palette.ThreadPad;
        float pt = pad[0], pr = pad[1], pb = pad[2], pl = pad[3];
        var y = pt;
        for (var i = 0; i < turns.Count; i++)
        {
            var t = turns[i];
            var user = 0;
            var n = 0;
            foreach (var (k, v) in stepUser)
            {
                if (k.Session != Session || k.Turn != i) continue;
                n++;
                var x = k.Step * 4;
                if (k.Now) x += 2;
                if (v) x++;
                user += x;
            }
            foreach (var (k, v) in groupUser)
            {
                if (k.Session != Session || k.Turn != i) continue;
                n++;
                user += k.First * unchecked((int)0x9e3779b1) + 7 + (v ? 3 : 5);
            }
            user += n;
            long fl = 0;
            foreach (var (k, v) in flags)
                if (k.Session == Session && k.Turn == i && v) fl += ((((long)k.Step << 20) | (uint)k.Which) + 1) * unchecked((long)0x9e3779b97f4a7c15);
            var last = i + 1 == turns.Count;
            var sig = t.Signature();
            var key = (i, width, t.Live, user, last, fl);
            Section s;
            if (i < old.Count && !old[i].Stale && old[i].Signature == sig && old[i].Key == key) s = old[i];
            else
            {
                Relayouts++;
                foreach (var k in HScroll.Keys.Where(k => k.Section == i).ToList()) HScroll.Remove(k);
                s = LayTurn(t, i, width - pl - pr, t.Live, last);
                s.Key = key;
                s.Signature = sig;
            }
            if (i > 0) y += Palette.ThreadGap;
            s.Y = y;
            y += s.H;
            Sections.Add(s);
        }
        Height = y + pb + ExtraBottom;
        if (HasSel && (SelAnchor.Section >= Sections.Count || SelFocus.Section >= Sections.Count)) HasSel = false;
    }

    TextBox Line(string s, Look look, float w)
    {
        var shaped = sh.Shape([Span.Plain(s)], look, w, Align.Start);
        return new TextBox { Layout = shaped.Layout, Text = shaped.Text, Runs = shaped.Runs };
    }

    TextBox Box(IReadOnlyList<Span> spans, Look look, float w, Align align = Align.Start)
    {
        var shaped = sh.Shape(spans, look, w, align);
        return new TextBox { Layout = shaped.Layout, Text = shaped.Text, Links = shaped.Links, Runs = shaped.Runs };
    }

    static Look Lk(float size, float lh, Rgba c, float weight) => new(size, lh, c, weight);
    static Look Lkm(float size, float lh, Rgba c, float weight) => new(size, lh, c, weight, FontFamily.Mono);

    /// <summary>What a laid-out turn is: its part of the frag and what it needs to be placed.</summary>
    Section LayTurn(Turn t, int ti, float w, bool live, bool last)
    {
        var o = new Section();
        var fr = new Frag();
        o.Frag = fr;
        pendingImages.Clear();
        waitingNow = false;
        var look = Palette.BodyLook();
        // .me: max-width 86%, padding 6px 10px, 1px border, radius 16 16 5 16, at the right.
        var me = look with { Color = t.Queued ? Palette.Dim : new Rgba(0xf1, 0xef, 0xf4) };
        var maxw = w * 0.86f - 22;
        var (_, mx) = sh.Widths([Span.Plain(t.Prompt)], me);
        var tw = (float)Math.Ceiling(Math.Min(mx, maxw));
        // .me .pics: 72 px thumbnails, gap 5, wrapping, 5 px under.
        var perRow = Math.Max((int)Math.Floor((maxw + 5) / 77), 1);
        var pics = t.Images.Count;
        if (pics > 0) tw = Math.Max(tw, Math.Min(Math.Min(pics, perRow) * 77 - 5, maxw));
        var prompt = Box([Span.Plain(t.Prompt)], me, tw);
        float picsH = pics > 0 ? (float)((pics + perRow - 1) / perRow) * 77 : 0;
        var bh = picsH + prompt.Layout.Height + 14;
        var bw = tw + 22;
        var bx = w - bw;
        float[] radius = [16, 16, 5, 16];
        if (t.Queued) Icons.Dashed(fr.Shapes, bx, 0, bw, bh, radius, new Rgba(0xff, 0xc4, 0x6b, 0x47));
        else fr.Shapes.Add(Shape.BoxOf(bx, 0, bw, bh, radius, Palette.YouBG, Palette.YouEdge, 1));
        for (var k = 0; k < t.Images.Count; k++)
        {
            float c = k % perRow, r = k / perRow;
            fr.Shapes.Add(new Shape { Kind = ShapeKind.Image, X = bx + 11 + c * 77, Y = 7 + r * 77, W = 72, H = 72, Radius = Shape.Round(8), Src = t.Images[k], Cover = true });
            o.Images.Add(t.Images[k]);
        }
        prompt.X = bx + 11;
        prompt.Y = 7 + picsH;
        fr.Add(prompt);
        fr.Copy.Add(Tok.Req(1));
        // .you-acts (and .q for a queued one): a row under the bubble, 3 px down, 4 px past its right edge.
        float rowY = bh + 3, rowH = 26;
        var faint = new Rgba(0xf6, 0xf2, 0xff, 97);
        var rx = bx + bw + 4;
        if (t.Queued)
        {
            // Edit takes it back into the reply box; Send now stops the run and sends it next. There is no Cancel.
            foreach (var (label, kind) in new[] { ("Send now", ActKind.SendNow), ("Edit", ActKind.Edit) })
            {
                var b = Line(label, Lk(11, 1.5f, faint, 400), 0);
                b.Text = "";
                var w2 = b.Layout.Width + 16;
                rx -= w2;
                b.X = rx + 8;
                b.Y = rowY + (rowH - b.Layout.Height) / 2;
                fr.Hits.Add(new HitRect(new Rect4(rx, rowY + 1, w2, 24), new Act(kind)));
                fr.Texts.Add(b);
                rx -= 2;
            }
            var q = Line("Queued · sends when this run ends", Lk(11, 1.5f, new Rgba(0xff, 0xc4, 0x6b, 204), 400), 0);
            rx -= 4;
            q.X = rx - q.Layout.Width;
            q.Y = rowY + (rowH - q.Layout.Height) / 2;
            fr.Add(q);
        }
        else
        {
            // Right to left: Edit, Copy, then the time.
            var done = t.Prompt.Length > 0 && Copied is not null && Copied == t.Prompt;
            var green = new Rgba(0x4a, 0xde, 0x80);
            var buttons = new List<(string Icon, Rgba C, Act Act)> { (Icons.Pencil, faint, new Act(ActKind.EditPrompt)) };
            if (t.Prompt.Length > 0)
                buttons.Add(done ? (Icons.Check, green, new Act(ActKind.Copy, t.Prompt)) : (Icons.Copy, faint, new Act(ActKind.Copy, t.Prompt)));
            foreach (var b in buttons)
            {
                rx -= 26;
                fr.Shapes.Add(Shape.SvgOf(rx + 6, rowY + 6, 14, 14, Icons.PathSvg(b.Icon, b.C, 14, 2)));
                fr.Hits.Add(new HitRect(new Rect4(rx, rowY, 26, rowH), b.Act));
            }
            if (t.When.Length > 0)
            {
                // The time is text in the page, so a selection still copies it.
                var wb = Line(t.When, Lk(11, 1.5f, faint, 400), 0);
                rx -= 4 + wb.Layout.Width;
                wb.X = rx;
                wb.Y = rowY + (rowH - wb.Layout.Height) / 2;
                fr.Add(wb);
            }
        }
        fr.Copy.Add(Tok.Req(1));
        var y = rowY + rowH;

        // The agent's part, in the order it happened: what it said, the tools it ran, what it
        // said next, and the answer last. A step's Lead is what was said just before it, so the
        // steps fall into groups at each one.
        var groups = StepGroups(t.Steps);
        var ans = t.Answer;
        if (groups.Count > 0 && ans.Trim() == t.Steps[groups[^1].A].Lead.Trim())
        {
            // Said before the last tools and nothing since: it is shown above them, not again.
            ans = "";
        }
        var hasText = ans.Length > 0 || groups.Any(g => t.Steps[g.A].Lead.Trim().Length > 0);
        var showSteps = t.Steps.Count > 0 && !HideSteps;
        var words = new List<string>();
        if (hasText || showSteps)
        {
            y += Palette.ThreadGap;
            // .who2: the tool's small logo and the agent's name, 6 px over what it did.
            var name = Line(Who, Lk(12, 1.5f, new Rgba(0xf3, 0xf1, 0xf6), 600), 0);
            var h = Math.Max(name.Layout.Height, 16);
            fr.Shapes.Add(Shape.SvgOf(0, y + (h - 16) / 2, 16, 16, "mark:" + Tool));
            name.X = 23;
            name.Y = y + (h - name.Layout.Height) / 2;
            fr.Add(name);
            fr.Copy.Add(Tok.Req(1));
            y += h + 6;
            var failed = t.Stage == Stage.Failed;
            var ansLook = look with { LH = 1.55f, Color = new Rgba(0xe9, 0xe7, 0xec) };
            var first = true;
            // Words the agent said, as Markdown. A failed turn's answer has the red rule.
            void Segment(string src, bool final)
            {
                if (!first) y += 8;
                first = false;
                float inset = final && failed ? 12 : 0;
                var m = new MdLayout(sh, source, Copied);
                var rendered = source.Blocks(src, Folder, Files);
                if (!rendered.Final) o.Waiting = true;
                var b = m.Blocks(rendered.Value ?? Plain(src), ansLook, w - inset, true);
                o.Images.AddRange(m.Used);
                o.Waiting |= m.Waiting;
                if (final && failed) fr.Shapes.Add(Shape.RectOf(0, y, 2, b.H, 0, Palette.Bad));
                var at = fr.Copy.Count;
                if (o.AnswerTok < 0) o.AnswerTok = at;
                if (final) o.AnswerAt = (fr.Texts.Count, fr.Shapes.Count, fr.Scrollers.Count);
                fr.Append(b.Frag, inset, y);
                y += b.H;
                var c = new Copier();
                foreach (var tok in fr.Copy.Skip(at))
                {
                    if (tok.Kind == TokKind.Text) c.Text(fr.Texts[tok.N].Text);
                    else c.Add(tok);
                }
                words.Add(c.Finish());
            }
            foreach (var g in groups)
            {
                var lead = t.Steps[g.A].Lead;
                if (lead.Trim().Length > 0) Segment(lead, false);
                if (showSteps)
                {
                    if (!first) y += 8;
                    first = false;
                    y += Group(fr, t, ti, g.A, g.B, y, w, live);
                }
            }
            if (ans.Length > 0) Segment(ans, true);
            // While it works: what it is doing, unless a running step or the words coming in
            // already show it. Once stopped, that it was.
            string status = "";
            var statusC = new Rgba(255, 255, 255, 128);
            var lastRuns = t.Steps.Count > 0 && !t.Steps[^1].Ended && showSteps;
            if (t.Stopping) status = "Stopping…";
            else if (live && t.Waiting) { status = "Waiting on you"; statusC = new Rgba(0xff, 0xc4, 0x6b, 230); }
            else if (live && !lastRuns && ans.Length == 0) status = "Working…";
            else if (!live && t.Stage == Stage.Stopped) status = "Stopped";
            if (status.Length > 0)
            {
                y += 6;
                var lay = Box([new Span { Text = status, Color = statusC, Weight = 500 }], Lk(11.5f, 1.5f, statusC, 500), 0);
                lay.Text = "";
                lay.Y = y;
                fr.Texts.Add(lay);
                y += lay.Layout.Height;
            }
            var whole = string.Join("\n\n", words);
            if (!HideSteps) y += Changes(fr, t, y, w);
            // .acts: Copy, Retry on the newest turn once it has ended, then how long the run took
            // and what it cost, when the tool says (unknown is left out, not 0).
            if (!live && !t.Queued)
            {
                y += 4;
                float arow = 24, x = -5;
                var label = Lk(11, 1.2f, new Rgba(0xf6, 0xf2, 0xff, 158), 400);
                var done = Copied is not null && Copied == whole;
                var buttons = new List<(string Icon, string Word, Act Act)>
                {
                    (done ? Icons.Check : Icons.Copy, done ? "Copied" : "Copy", new Act(ActKind.Copy, whole)),
                };
                // One way to send the newest message again: Try again where a checkpoint lets the
                // folder go back too, else Retry (the prompt again, files as they are).
                if (last && t.Stage != Stage.Waking && !t.Again) buttons.Add((Icons.Retry, "Retry", new Act(ActKind.Retry)));
                if (t.Restore) buttons.Add((Icons.Undo, "Restore", new Act(ActKind.Restore)));
                if (t.Again) buttons.Add((Icons.Try, "Try again", new Act(ActKind.TryAgain)));
                foreach (var b in buttons)
                {
                    var l = label;
                    var ic = new Rgba(0xf6, 0xf2, 0xff, 158);
                    if (b.Word == "Copied")
                    {
                        l = l with { Color = new Rgba(0x4a, 0xde, 0x80) };
                        ic = l.Color;
                    }
                    var tb = Line(b.Word, l, 0);
                    tb.Text = "";
                    var bw2 = 5 + 13 + 4 + tb.Layout.Width + 5;
                    fr.Shapes.Add(Shape.SvgOf(x + 5, y + (arow - 13) / 2, 13, 13, Icons.PathSvg(b.Icon, ic, 13, 2)));
                    tb.X = x + 22;
                    tb.Y = y + (arow - tb.Layout.Height) / 2;
                    fr.Texts.Add(tb);
                    fr.Hits.Add(new HitRect(new Rect4(x, y, bw2, arow), b.Act));
                    x += bw2 + 2;
                }
                var meta = new List<string>();
                if (t.Took.Length > 0) meta.Add(t.Took);
                if (t.Credits.Length > 0) meta.Add(t.Credits);
                if (meta.Count > 0)
                {
                    var mt = Line(string.Join(" · ", meta), label with { Color = new Rgba(0xf6, 0xf2, 0xff, 97) }, 0);
                    mt.Text = "";
                    mt.X = x + 6;
                    mt.Y = y + (arow - mt.Layout.Height) / 2;
                    fr.Texts.Add(mt);
                }
                y += arow;
            }
        }
        o.Images.AddRange(pendingImages);
        o.Waiting |= waitingNow;
        o.H = y;
        return o;
    }

    /// <summary>Plain paragraphs, for an answer the backend has not read yet.</summary>
    static List<MdBlock> Plain(string src)
    {
        var list = new List<MdBlock>();
        foreach (var para in src.Replace("\r\n", "\n").Split("\n\n"))
        {
            if (para.Length == 0) continue;
            var inl = new List<MdInline>();
            var lines = para.Split('\n');
            for (var i = 0; i < lines.Length; i++)
            {
                if (i > 0) inl.Add(new MdInline { Kind = "break" });
                inl.Add(new MdInline { Kind = "text", Text = lines[i] });
            }
            list.Add(new MdBlock { Kind = "para", Inlines = inl });
        }
        return list;
    }

    // MARK: Scroll boxes

    /// <summary>How far a text box is moved left by the box that scrolls it.</summary>
    public float OffsetOf(int section, TextBox t) => t.Scr == 0 ? 0 : HScroll.GetValueOrDefault((section, t.Scr - 1));

    /// <summary>The sideways scroll bars in thread coordinates: where each is, its length, content and position.</summary>
    public IEnumerable<(int Section, int Scroller, float X, float Y, float Len, float Content, float Pos)> HBars()
    {
        var ox = Palette.ThreadPad[3];
        for (var si = 0; si < Sections.Count; si++)
        {
            var s = Sections[si];
            for (var k = 0; k < s.Frag.Scrollers.Count; k++)
            {
                var sc = s.Frag.Scrollers[k];
                yield return (si, k, ox + sc.Clip.X, s.Y + sc.Clip.Y + sc.Clip.H, sc.Clip.W, sc.Content, HScroll.GetValueOrDefault((si, k)));
            }
        }
    }

    /// <summary>Scrolls a box sideways (clamped to its content).</summary>
    public void ScrollBox(int section, int scroller, float pos)
    {
        if (section >= Sections.Count || scroller >= Sections[section].Frag.Scrollers.Count) return;
        HScroll[(section, scroller)] = Math.Min(Math.Max(pos, 0), Sections[section].Frag.Scrollers[scroller].Max);
    }

    /// <summary>The sideways-scrolling box under a point in thread coordinates.</summary>
    public (int Section, int Scroller)? BoxAt(float x, float y)
    {
        x -= Palette.ThreadPad[3];
        for (var si = 0; si < Sections.Count; si++)
        {
            var s = Sections[si];
            for (var k = 0; k < s.Frag.Scrollers.Count; k++)
            {
                var c = s.Frag.Scrollers[k].Clip;
                if (x >= c.X && x < c.Right && y >= s.Y + c.Y && y < s.Y + c.Y + c.H + Palette.Thick) return (si, k);
            }
        }
        return null;
    }
}
