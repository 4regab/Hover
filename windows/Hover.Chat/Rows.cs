using System.Globalization;
using Hover.Host.Protocol;

namespace Hover.Chat;

// The agent's steps as the chat shows them: a timeline of rows on a thin line, a command as
// one quiet line, a change or an output opened under its row, a thought, the subagents, and
// what a finished turn changed.
public sealed partial class ChatThread
{
    static string F1(double v) => v.ToString("0.0", CultureInfo.InvariantCulture);

    /// <summary>The timeline, one row per tool call on a thin line. Files read one after another fold into one row with their names under it.</summary>
    float Timeline(Frag fr, Turn t, int ti, int a, int b, float y0, float w, bool live)
    {
        var list = t.Steps.Take(b).ToList();
        bool IsLive(int j) => live && j + 1 == t.Steps.Count && !list[j].Ended;
        var y = y0;
        var lineAt = fr.Shapes.Count;
        for (var j = a; j < list.Count;)
        {
            var x = list[j];
            // Subagents started one after another: one list.
            if (x.Kind == StepIcon.Agent)
            {
                var e = j;
                while (e + 1 < list.Count && list[e + 1].Kind == StepIcon.Agent) e++;
                y += Agents(fr, t, ti, j, e, y, w);
                j = e + 1;
                continue;
            }
            if (x.Kind == StepIcon.Read && x.Name.Length > 0 && !IsLive(j) && x.Status != "failed")
            {
                var e = j;
                while (e + 1 < list.Count && list[e + 1].Kind == StepIcon.Read && list[e + 1].Name.Length > 0 && !IsLive(e + 1) && list[e + 1].Status != "failed") e++;
                var names = list.Skip(j).Take(e - j + 1).Select(s => s.Name).Distinct().ToList();
                if (names.Count > 1)
                {
                    var row = new Step { Kind = StepIcon.Read, Verb = "Read", Name = $"{names.Count} files", Status = "completed" };
                    y += StepRow(fr, row, ti, -1, false, y, w, false, false);
                    // .fchips: the names, mono 11 px, wrapping, 28 px in.
                    float cx = 28, cy = y + 1;
                    foreach (var n in names)
                    {
                        var c = Line(n, Lkm(11, 18f / 11, new Rgba(255, 255, 255, 179), 400), 0);
                        var cw = c.Layout.Width + 16;
                        if (cx + cw > w && cx > 28)
                        {
                            cx = 28;
                            cy += 22;
                        }
                        fr.Shapes.Add(Shape.BoxOf(cx, cy, cw, 20, Shape.Round(6), new Rgba(255, 255, 255, 13), new Rgba(255, 255, 255, 15), 1));
                        c.X = cx + 8;
                        c.Y = cy + 1;
                        fr.Add(c);
                        cx += cw + 4;
                    }
                    fr.Copy.Add(Tok.Req(1));
                    y = cy + 20 + 5;
                    j = e + 1;
                    continue;
                }
            }
            var open = StepOpen(t, ti, j, false);
            y += StepRow(fr, x, ti, j, false, y, w, IsLive(j), open);
            j++;
        }
        // .steps::before: the thin line under the icons, 12 px in from each end.
        if (y - y0 > 24) fr.Shapes.Insert(lineAt, Shape.RectOf(9, y0 + 12, 1, y - y0 - 24, 0, new Rgba(255, 255, 255, 20)));
        return y - y0;
    }

    /// <summary>
    /// Steps [a, b) of turn ti: one run of tools between two things the agent said. One step is its
    /// own row. More are one quiet line that says what the tools did, as a sentence, with the caret
    /// just after it; open, the calls sit on the timeline under it.
    /// </summary>
    float Group(Frag fr, Turn t, int ti, int a, int b, float y, float w, bool live)
    {
        if (b - a == 1) return Timeline(fr, t, ti, a, b, y, w, live);
        const float head = 26;
        var open = GroupOpen(t, ti, a, b);
        var working = live && b == t.Steps.Count;
        var label = StepSentence(t.Steps.Skip(a).Take(b - a).ToList(), working);
        if (label.Length == 0) label = $"Used {b - a} tools";
        var tb = Line(label, Lk(12.5f, 1.4f, new Rgba(255, 255, 255, 140), 500), 0);
        // user-select: none: drawn, not copied. Folded while it works, it shimmers.
        var room = Math.Max(w - 18, 0);
        tb.Text = "";
        tb.Y = y + (head - tb.Layout.Height) / 2;
        tb.Clip = new Rect4(0, y, room, head);
        tb.Shimmer = working && !open;
        fr.Texts.Add(tb);
        var cx = Math.Min(tb.Layout.Width, room) + 5;
        fr.Shapes.Add(Icons.Caret(cx, y + (head - 12) / 2, 12, new Rgba(255, 255, 255, 110), open));
        fr.Hits.Add(new HitRect(new Rect4(-4, y, w + 8, head), new Act(ActKind.Group, I: a)));
        var h = head;
        if (open)
        {
            var inner = new Frag();
            var rh = Timeline(inner, t, ti, a, b, 0, w, live);
            fr.Append(inner, 0, y + head + 2);
            h += 2 + rh;
        }
        return h;
    }

    /// <summary>A group's header as the chat words it: "Thought for 18s, read 2 files, ran 3 commands", the first word capitalised.</summary>
    internal static string StepSentence(IReadOnlyList<Step> steps, bool working)
    {
        var bits = StepBits(steps, working);
        if (bits.Count == 0) return "";
        var s = string.Join(", ", bits);
        return char.ToUpperInvariant(s[0]) + s[1..];
    }

    static List<string> StepBits(IReadOnlyList<Step> steps, bool working)
    {
        int N(StepIcon k) => steps.Count(s => s.Kind == k);
        static string Plural(int k, string one, string many) => k == 1 ? one : string.Format(CultureInfo.InvariantCulture, many, k);
        var files = new HashSet<string>();
        double thoughtMs = 0;
        foreach (var x in steps)
        {
            if (x.Kind == StepIcon.Edit) files.Add(x.Name.Length > 0 ? x.Name : x.Cmd.Length > 0 ? x.Cmd : x.Verb);
            else if (x.Kind == StepIcon.Thought && x.HasMs) thoughtMs += x.Ms;
        }
        var bits = new List<string>();
        if (!working && thoughtMs > 0) bits.Add("thought for " + Turns.Secs(thoughtMs));
        if (N(StepIcon.Read) is var r && r > 0) bits.Add(Plural(r, "read a file", "read {0} files"));
        if (N(StepIcon.Search) is var s && s > 0) bits.Add(Plural(s, "searched once", "searched {0} times"));
        if (files.Count > 0) bits.Add(Plural(files.Count, "edited a file", "edited {0} files"));
        if (N(StepIcon.Run) is var c && c > 0) bits.Add(Plural(c, "ran a command", "ran {0} commands"));
        if (N(StepIcon.Agent) is var g && g > 0) bits.Add(Plural(g, "used a subagent", "used {0} subagents"));
        if (N(StepIcon.Think) is var o && o > 0) bits.Add(Plural(o, "used a tool", "used {0} tools"));
        return bits;
    }

    /// <summary>Where a line cut short for an ellipsis ends: the cluster edge nearest the room left for the dots, never past it.</summary>
    static float CutAt(TextBox tb, float room) => Math.Max(Math.Min(tb.Layout.SnapX(Math.Max(room, 0)), room), 0);

    /// <summary>
    /// A command as one quiet line: "Ran `cmd`", its exit code and time at the right (red when it
    /// failed), and a click opens what it printed. The verb and the command are one text box, so
    /// the running one has one shimmer band across all of it, and its time counts.
    /// </summary>
    float CommandRow(Frag fr, Step x, int ti, int j, bool now, float y, float w, bool live, bool open)
    {
        const float rowH = 25, tx = 28;
        var fail = x.Status == "failed" || (x.HasExit && x.Exit != 0);
        var ic = new Rgba(0xff, 0xc4, 0x6b);
        var edge = new Rgba(255, 255, 255, 20);
        if (fail && !live) ic = new Rgba(0xff, 0x7b, 0x72);
        var iy = y + (rowH - 19) / 2;
        if (live)
        {
            edge = new Rgba(255, 196, 107, 128);
            fr.Shapes.Add(new Shape { Kind = ShapeKind.Glow, X = 9.5f, Y = iy + 9.5f, W = 12, Fill = new Rgba(255, 196, 107, 64) });
        }
        fr.Shapes.Add(Shape.BoxOf(0, iy, 19, 19, Shape.Round(6), new Rgba(0x1c, 0x1a, 0x20), edge, 1));
        fr.Shapes.Add(Shape.SvgOf(4, iy + 4, 11, 11, Icons.Of(StepIcon.Run, ic, 11, 2.2f)));
        var verb = live ? Icons.VerbOn(x.Verb) : x.Verb;
        var baseLook = Lk(12, 1.3f, Palette.Faint, 400);
        // .cl code: no background, the dim colour, the mono face a size under the words.
        var spans = new List<Span> { Span.Plain(verb + " "), new Span { Text = x.Cmd, Color = Palette.Dim, Family = FontFamily.Mono, Size = 11 } };
        // .cmd-m: "exit 0 · 0.3s"; a running one says only how long it has gone.
        Rgba green = new(0x7e, 0xe5, 0x9a, 204), red = new(0xff, 0x7b, 0x72);
        var sep = Span.Colored(" · ", new Rgba(255, 255, 255, 51));
        var meta = new List<Span>();
        if (live)
        {
            var k = (Session, ti, j);
            if (!began.TryGetValue(k, out var t0)) began[k] = t0 = Clock();
            meta.Add(Span.Plain($"{(int)(Clock() - t0).TotalSeconds}s"));
        }
        else
        {
            if (x.HasExit) meta.Add(Span.Colored($"exit {x.Exit}", x.Exit == 0 ? green : red));
            else if (x.Status == "failed") meta.Add(Span.Colored("failed", red));
            if (x.HasMs)
            {
                if (meta.Count > 0) meta.Add(sep);
                var s = x.Ms / 1000;
                meta.Add(Span.Plain(s < 10 ? F1(s) + "s" : s < 60 ? $"{Math.Round(s, MidpointRounding.AwayFromZero):0}s" : $"{(long)(s / 60)}m {Math.Round(s % 60, MidpointRounding.AwayFromZero):00}s"));
            }
        }
        var blk = x.HasBlock;
        var rx = w - 4;
        if (blk && !live)
        {
            rx -= 12;
            fr.Shapes.Add(Icons.Caret(rx, y + (rowH - 12) / 2, 12, new Rgba(255, 255, 255, 89), open));
            rx -= 8;
        }
        TextBox? right = null;
        if (meta.Count > 0)
        {
            right = Box(meta, baseLook with { Size = 11, LH = 1.3f }, 0);
            rx -= right.Layout.Width;
            right.X = rx;
            right.Y = y + (rowH - right.Layout.Height) / 2;
            rx -= 12;
        }
        var avail = Math.Max(rx - tx, 0);
        var tb = Box(spans, baseLook, 0);
        tb.X = tx;
        tb.Y = y + (rowH - tb.Layout.Height) / 2;
        tb.Shimmer = live;
        if (tb.Layout.Width > avail + 0.01f)
        {
            // white-space: nowrap; text-overflow: ellipsis.
            var e = Line("…", baseLook, 0);
            var cut = CutAt(tb, avail - e.Layout.Width);
            tb.Clip = new Rect4(tx, tb.Y - 2, cut, tb.Layout.Height + 4);
            e.Text = "";
            e.X = tx + cut;
            e.Y = tb.Y;
            fr.Texts.Add(e);
        }
        fr.Add(tb);
        fr.Copy.Add(Tok.Req(1));
        if (right is not null)
        {
            fr.Add(right);
            fr.Copy.Add(Tok.Req(1));
        }
        if (blk && j >= 0) fr.Hits.Add(new HitRect(new Rect4(-8, y, w + 16, rowH), new Act(ActKind.Step, I: j, Now: now)));
        var h = rowH;
        if (blk && open) h += Block(fr, x, ti, j, y + rowH, w);
        return h;
    }

    /// <summary>
    /// A step: the kind's icon, the verb and the file (name bright, folder dim) or the command,
    /// and on the right the counts, how it went and how long it took; a step with a change or
    /// output opens to show it. Returns its height, block and all.
    /// </summary>
    float StepRow(Frag fr, Step x, int ti, int j, bool now, float y, float w, bool live, bool open)
    {
        if (x.Kind == StepIcon.Thought) return ThoughtRow(fr, x, ti, j, now, y, w, open);
        if (x.Kind == StepIcon.Run && x.Name.Length == 0 && x.Cmd.Length > 0) return CommandRow(fr, x, ti, j, now, y, w, live, open);
        var verb = live ? Icons.VerbOn(x.Verb) : x.Verb;
        var fail = x.Status == "failed";
        const float rowH = 25;
        var baseLook = Lk(12, 1.3f, new Rgba(255, 255, 255, 148), 400);
        var bright = new Rgba(0xf3, 0xf1, 0xf6);
        var spans = new List<Span>();
        if (x.Name.Length > 0)
        {
            spans.Add(Span.Plain(verb + " "));
            spans.Add(new Span { Text = x.Name, Color = bright, Weight = 500 });
            if (x.Dir.Length > 0)
            {
                spans.Add(Span.GapOf(6));
                spans.Add(new Span { Text = x.Dir, Color = new Rgba(255, 255, 255, 82), Family = FontFamily.Mono, Size = 11 });
            }
        }
        else if (x.Cmd.Length > 0)
        {
            // The whole command (or pattern, or URL), a size under the row's words, wrapped under
            // the verb when it does not fit: cut short, it cannot be checked.
            spans.Add(Span.Plain(verb + " "));
            spans.Add(new Span { Text = x.Cmd, Code = true, Color = bright, Size = 11 });
        }
        else if (live) spans.Add(new Span { Text = verb, Color = bright, Weight = 500 });
        else spans.Add(Span.Plain(verb));
        // .r: mono 10.5, faint; its parts right to left after the caret.
        var mono = Lkm(10.5f, 1.0f, new Rgba(255, 255, 255, 97), 500);
        Rgba green = new(0x5d, 0xe3, 0x7a), red = new(0xff, 0x7b, 0x72);
        var parts = new List<(string S, Rgba C)>();
        if (x.Add != 0 || x.Del != 0)
        {
            parts.Add(($"+{x.Add}", green));
            parts.Add(($"−{x.Del}", red));
        }
        if (fail) parts.Add(("failed", red));
        else if (x.Kind == StepIcon.Run && x.HasExit && x.Exit != 0) parts.Add(($"exit {x.Exit}", red));
        if (x.HasMs && x.Ms >= 1000) parts.Add((Turns.TookText(x.Ms), mono.Color));
        var blk = x.HasBlock;
        var rx = w - 4;
        if (blk)
        {
            rx -= 12;
            fr.Shapes.Add(Icons.Caret(rx, y + (rowH - 12) / 2, 12, new Rgba(255, 255, 255, 89), open));
            rx -= 5;
        }
        // Laid out right to left, copied left to right after the row's text.
        var right = new List<TextBox>();
        for (var i = parts.Count - 1; i >= 0; i--)
        {
            var tb = Line(parts[i].S, mono with { Color = parts[i].C }, 0);
            rx -= tb.Layout.Width;
            tb.X = rx;
            tb.Y = y + (rowH - tb.Layout.Height) / 2;
            right.Add(tb);
            rx -= 5;
        }
        right.Reverse();
        // .n: the icon in its box, coloured by kind.
        Rgba ic;
        if (fail) ic = red;
        else ic = x.Kind switch
        {
            StepIcon.Edit => new Rgba(0xc9, 0xa8, 0xff),
            StepIcon.Run => new Rgba(0xff, 0xc4, 0x6b),
            StepIcon.Search => new Rgba(0x6f, 0xd6, 0xc9),
            StepIcon.Read => new Rgba(0x8f, 0xb6, 0xff),
            StepIcon.Think => new Rgba(255, 255, 255, 115),
            _ => new Rgba(0xc4, 0xa2, 0xff),
        };
        var iy = y + (rowH - 19) / 2;
        var edge = new Rgba(255, 255, 255, 20);
        if (live)
        {
            edge = new Rgba(255, 196, 107, 128);
            fr.Shapes.Add(new Shape { Kind = ShapeKind.Glow, X = 9.5f, Y = iy + 9.5f, W = 12, Fill = new Rgba(255, 196, 107, 64) });
        }
        fr.Shapes.Add(Shape.BoxOf(0, iy, 19, 19, Shape.Round(6), new Rgba(0x1c, 0x1a, 0x20), edge, 1));
        fr.Shapes.Add(Shape.SvgOf(4, iy + 4, 11, 11, Icons.Of(x.Kind, ic, 11, 2.2f)));
        const float tx = 28;
        var avail = Math.Max(rx - tx, 0);
        var wrap = x.Name.Length == 0 && x.Cmd.Length > 0;
        var label = Box(spans, baseLook, wrap ? avail : 0);
        label.X = tx;
        label.Y = y + (rowH - 15.6f) / 2;
        label.Shimmer = live;
        // A wrapped command makes the row taller; the icon and the right side stay on its first line.
        var fullH = rowH;
        if (wrap) fullH = Math.Max(rowH, label.Layout.Height + (rowH - 15.6f));
        if (!wrap && label.Layout.Width > avail + 0.01f)
        {
            // text-overflow: ellipsis: cut at a cluster that leaves room for the dots.
            var e = Line("…", baseLook, 0);
            var cut = CutAt(label, avail - e.Layout.Width);
            label.Clip = new Rect4(label.X, label.Y - 2, cut, label.Layout.Height + 4);
            e.Text = "";
            e.X = label.X + cut;
            e.Y = label.Y;
            fr.Texts.Add(e);
        }
        fr.Add(label);
        fr.Copy.Add(Tok.Req(1));
        var counts = x.Add != 0 || x.Del != 0;
        for (var k = 0; k < right.Count; k++)
        {
            fr.Add(right[k]);
            // "+2" and "−1" sit in one span, so they copy as one line.
            if (!(counts && k == 0)) fr.Copy.Add(Tok.Req(1));
        }
        if (blk && j >= 0) fr.Hits.Add(new HitRect(new Rect4(-4, y, w + 8, fullH), new Act(ActKind.Step, I: j, Now: now)));
        var h = fullH;
        if (blk && open) h += Block(fr, x, ti, j, y + fullH, w);
        return h;
    }

    sealed record BlockRow(long N, bool HasN, string Text, Rgba C, Rgba Bg = default, bool HasBg = false, bool Note = false);

    sealed record RightBit(string S, Rgba C, Rgba Bg = default, bool HasBg = false);

    /// <summary>
    /// An edit's change (its file, the +/− counts, Copy; the file's line numbers when the diff
    /// names them, never made up) or a command's output (its exit code when the tool said it, how
    /// long it took). Eight lines, then "Show N more lines"; all of what was kept once asked. A
    /// command shows its last lines, with "Show N earlier lines"; a change shows its first.
    /// </summary>
    float Block(Frag fr, Step x, int ti, int j, float y, float w)
    {
        const float head = 28;
        // A command's output is the whole row wide, with no header; a file's change keeps its header.
        var cmdblk = x.Diff.Length == 0;
        float bx = cmdblk ? 0 : 28, bw = cmdblk ? w : w - 28;
        var y0 = y + 4;
        var at = fr.Shapes.Count;
        Rgba green = new(0x4a, 0xde, 0x80), red = new(0xff, 0x6b, 0x62);
        var headL = Lkm(10.5f, 1.3f, new Rgba(0xf6, 0xf2, 0xff, 158), 400);
        var small = Lk(11, 1.3f, new Rgba(0xf6, 0xf2, 0xff, 97), 500);
        var mono = Lkm(11.5f, 1.6f, new Rgba(0xf6, 0xf2, 0xff, 214), 400);
        var rows = new List<BlockRow>();
        var right = new List<RightBit>();
        var title = "";
        var copyText = "";
        var hasCopy = false;
        var waiting = false;
        if (x.Diff.Length > 0)
        {
            title = x.Dir.Length > 0 && x.Name.Length > 0 ? x.Dir + "/" + x.Name : x.Name.Length > 0 ? x.Name : x.Cmd;
            var lines = new List<string>();
            var rendered = source.Diff(x.Diff);
            waiting = !rendered.Final;
            var numbered = rendered.Value ?? [.. x.Diff.Split('\n').Select(l => new DiffLine { Text = l })];
            foreach (var nl in numbered)
            {
                if (nl.Gap)
                {
                    rows.Add(new BlockRow(0, false, "⋯", new Rgba(0xf6, 0xf2, 0xff, 72)));
                    continue;
                }
                var l = nl.Text;
                lines.Add(l);
                if (l.StartsWith('+')) rows.Add(new BlockRow(nl.N, nl.HasN, l, new Rgba(0xb8, 0xf5, 0xc9), new Rgba(0x4a, 0xde, 0x80, 0x14), true));
                else if (l.StartsWith('-')) rows.Add(new BlockRow(nl.N, nl.HasN, l, new Rgba(0xff, 0xc2, 0xbd), new Rgba(0xff, 0x6b, 0x62, 0x14), true));
                else rows.Add(new BlockRow(nl.N, nl.HasN, l.Length == 0 ? " " : l, mono.Color));
            }
            if (x.Add != 0) right.Add(new RightBit($"+{x.Add}", green));
            if (x.Del != 0) right.Add(new RightBit($"−{x.Del}", red));
            copyText = string.Join("\n", lines);
            hasCopy = true;
        }
        else
        {
            if (x.Cmd.Length > 0) title = "$ " + x.Cmd;
            // Only a command has a prompt line; what a search or a fetch was for is on its row.
            if (cmdblk && x.Cmd.Length > 0 && x.Kind == StepIcon.Run) rows.Add(new BlockRow(0, false, "$ " + x.Cmd, mono.Color));
            var outLines = x.Out.Split('\n');
            for (var k = 0; k < outLines.Length; k++)
            {
                var l = outLines[k];
                // What the agent tool cut from a long output, said as its own first line.
                var note = k == 0 && l.StartsWith("… ") && l.EndsWith("not kept");
                if (l.Length == 0) l = " ";
                rows.Add(new BlockRow(0, false, l, note ? new Rgba(0xf6, 0xf2, 0xff, 97) : mono.Color, Note: note));
            }
            // Unknown is not success: no exit code, no green.
            if (x.HasExit)
                right.Add(x.Exit == 0 ? new RightBit("exit 0", green, new Rgba(0x4a, 0xde, 0x80, 0x1a), true) : new RightBit($"exit {x.Exit}", red, new Rgba(0xff, 0x6b, 0x62, 0x1a), true));
            else if (x.Status == "failed") right.Add(new RightBit("failed", red, new Rgba(0xff, 0x6b, 0x62, 0x1a), true));
            if (x.HasMs && x.Ms >= 1000) right.Add(new RightBit(Turns.TookText(x.Ms), small.Color));
        }
        // .bh: the title, the counts or exit code, Copy.
        if (cmdblk)
        {
            right = [];
            title = "";
        }
        var hy = y0 + 1;
        var rx = bx + bw - 8;
        if (hasCopy)
        {
            var done = Copied is not null && Copied == copyText;
            var l = small with { Color = done ? green : new Rgba(0xf6, 0xf2, 0xff, 158) };
            var tb = Line(done ? "Copied" : "Copy", l, 0);
            tb.Text = "";
            var cw = tb.Layout.Width + 13 + 4 + 10;
            rx -= cw;
            fr.Shapes.Add(Shape.SvgOf(rx + 5, hy + (head - 12) / 2, 12, 12, Icons.PathSvg(done ? Icons.Check : Icons.Copy, l.Color, 12, 2)));
            tb.X = rx + 21;
            tb.Y = hy + (head - tb.Layout.Height) / 2;
            fr.Texts.Add(tb);
            fr.Hits.Add(new HitRect(new Rect4(rx, hy + 2, cw, head - 4), new Act(ActKind.Copy, copyText)));
            rx -= 6;
        }
        for (var i = right.Count - 1; i >= 0; i--)
        {
            var p = right[i];
            var l = small with { Color = p.C, Size = 10.5f, Weight = p.HasBg ? 600 : 500 };
            var tb = Line(p.S, l, 0);
            tb.Text = "";
            float pad = p.HasBg ? 6 : 0;
            rx -= tb.Layout.Width + pad;
            tb.X = rx;
            tb.Y = hy + (head - tb.Layout.Height) / 2;
            if (p.HasBg) fr.Shapes.Add(Shape.RectOf(rx - 6, tb.Y - 1.5f, tb.Layout.Width + 12, tb.Layout.Height + 3, 5, p.Bg));
            fr.Texts.Add(tb);
            rx -= 6 + pad;
        }
        var headH = cmdblk ? 0 : head;
        if (title.Length > 0)
        {
            // A command wraps to all of it (the counts and Copy keep to its first line); a file's path keeps to one line.
            var wrap = x.Diff.Length == 0;
            var tw = Math.Max(rx - bx - 14, 0);
            var tb = Line(title, headL, wrap ? tw : 0);
            tb.Text = "";
            tb.X = bx + 10;
            tb.Y = hy + (head - headL.Size * headL.LH) / 2;
            if (wrap) headH = Math.Max(head, tb.Layout.Height + (head - headL.Size * headL.LH));
            else tb.Clip = new Rect4(bx + 10, hy, tw, head);
            fr.Texts.Add(tb);
        }
        if (!cmdblk) fr.Shapes.Add(Shape.RectOf(bx + 1, hy + headH, bw - 2, 1, 0, new Rgba(255, 255, 255, 12)));
        // pre: 8 px above and below; a 32 px gutter when the lines are numbered.
        var numberedRows = rows.Any(r => r.HasN);
        var tx = bx + 12;
        if (numberedRows) tx += 32;
        var total = rows.Count;
        var full = Flag(ti, j, 0);
        // A command's prompt line stays on top, then the end of what it printed. A change shows its first lines.
        var lead = cmdblk && x.Cmd.Length > 0 && x.Kind == StepIcon.Run ? 1 : 0;
        var visible = rows;
        var hidden = 0;
        if (total - lead > 8 && !full)
        {
            hidden = total - lead - 8;
            visible = cmdblk ? [.. rows.Take(lead), .. rows.Skip(total - 8)] : rows.Take(8).ToList();
        }
        var cy = cmdblk ? hy + headH + 10 : hy + headH + 1 + 8;
        var clipTop = cy;
        for (var ri = 0; ri < visible.Count; ri++)
        {
            var r = visible[ri];
            // The output's first line is the command after its "$" prompt, which is fainter.
            var prompt = lead == 1 && ri == 0;
            var spans = prompt
                ? new List<Span> { Span.Colored("$ ", new Rgba(0xf6, 0xf2, 0xff, 97)), Span.Colored(r.Text[2..], r.C) }
                : [new Span { Text = r.Text, Em = r.Note, Color = r.C }];
            // The command line above is cut short by its ellipsis, and the output is pre-wrap:
            // both wrap here. So does a change: a long line is read whole, not cut at the box's edge.
            var ww = cmdblk ? Math.Max(bw - 24, 0) : Math.Max(bw - (tx - bx) - 12, 0);
            var body = Box(spans, mono, ww);
            var h = body.Layout.Height;
            if (r.HasBg) fr.Shapes.Add(Shape.RectOf(bx + 1, cy, bw - 2, h, 0, r.Bg));
            if (r.HasN)
            {
                var g = new Rgba(0xf6, 0xf2, 0xff, 46);
                if (r.HasBg) g = r.Bg.R == 0x4a ? new Rgba(0x4a, 0xde, 0x80, 153) : new Rgba(0xff, 0x6b, 0x62, 153);
                var gb = Line(r.N.ToString(CultureInfo.InvariantCulture), mono with { Color = g }, 0);
                gb.Text = "";
                gb.X = bx + 12 + 22 - gb.Layout.Width;
                gb.Y = cy;
                fr.Texts.Add(gb);
            }
            body.X = tx;
            body.Y = cy;
            body.Clip = new Rect4(bx + 1, clipTop, bw - 2, 1e6f);
            fr.Add(body);
            fr.Copy.Add(Tok.Req(1));
            cy += h;
        }
        cy += cmdblk ? 10 : 8;
        if (total - lead > 8)
        {
            // .fold: the rest of what was kept, or fold it again.
            fr.Shapes.Add(Shape.RectOf(bx + 1, cy, bw - 2, 1, 0, new Rgba(255, 255, 255, 10)));
            var label = "Fold";
            if (!full) label = $"Show {hidden} {(cmdblk ? "earlier" : "more")} line{Plural1(hidden)}";
            var tb = Line(label, small, 0);
            tb.Text = "";
            tb.X = bx + 12;
            tb.Y = cy + 1 + (24 - tb.Layout.Height) / 2;
            fr.Texts.Add(tb);
            fr.Hits.Add(new HitRect(new Rect4(bx, cy, bw, 25), new Act(ActKind.Flag, I: j, K: 0)));
            cy += 25;
        }
        fr.Shapes.Insert(at, Shape.BoxOf(bx, y0, bw, cy - y0 + 1, Shape.Round(9), new Rgba(0x0a, 0x09, 0x0c), new Rgba(255, 255, 255, 16), 1));
        waitingNow |= waiting;
        return cy + 1 - y + (cmdblk ? 8 : 4);
    }

    static string Plural1(int n) => n == 1 ? "" : "s";

    /// <summary>
    /// A thought: the bulb, "Thinking…" while it streams (its text under it, the newest lines in
    /// view) or "Thought for 14s" and its first words once done, which opens to all of it.
    /// </summary>
    float ThoughtRow(Frag fr, Step x, int ti, int j, bool now, float y, float w, bool open)
    {
        const float rowH = 25;
        var live = !x.Ended;
        var body = x.Out.Trim();
        var lilac = new Rgba(0xc4, 0xa2, 0xff);
        var iy = y + (rowH - 19) / 2;
        var edge = live ? new Rgba(0xc4, 0xa2, 0xff, 128) : new Rgba(255, 255, 255, 20);
        fr.Shapes.Add(Shape.BoxOf(0, iy, 19, 19, Shape.Round(6), new Rgba(0x1c, 0x1a, 0x20), edge, 1));
        fr.Shapes.Add(Shape.SvgOf(4, iy + 4, 11, 11, Icons.Of(StepIcon.Thought, lilac, 11, 2.2f)));
        var baseLook = Lk(12, 1.3f, new Rgba(255, 255, 255, 148), 400);
        var label = "Thinking…";
        if (!live) label = x.HasMs ? "Thought for " + Turns.Secs(x.Ms) : "Thought";
        var spans = new List<Span> { new() { Text = label, Color = new Rgba(0xf3, 0xf1, 0xf6), Weight = 500 } };
        if (!live && !open && body.Length > 0)
        {
            var words = body.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries).Take(8);
            spans.Add(Span.GapOf(8));
            spans.Add(new Span { Text = string.Join(" ", words).TrimEnd('.', ',', ':') + "…", Color = new Rgba(255, 255, 255, 82), Size = 11.5f });
        }
        var lay = Box(spans, baseLook, 0);
        const float tx = 28;
        var rx = w - 4;
        if (body.Length > 0)
        {
            rx = w - 4 - 12 - 5;
            fr.Shapes.Add(Icons.Caret(w - 16, y + (rowH - 12) / 2, 12, new Rgba(255, 255, 255, 89), open));
        }
        // Drawn: the label isn't the thought.
        lay.Text = "";
        lay.X = tx;
        lay.Y = y + (rowH - 15.6f) / 2;
        lay.Clip = new Rect4(tx, y, Math.Max(rx - tx, 0), rowH);
        lay.Shimmer = live;
        fr.Texts.Add(lay);
        if (body.Length > 0) fr.Hits.Add(new HitRect(new Rect4(-4, y, w + 8, rowH), new Act(ActKind.Step, I: j, Now: now)));
        var h = rowH;
        if (open && body.Length > 0)
        {
            // .th .body: a 2 px rule, 10 px in, 12.5/1.6. While it streams, the newest 168 px show
            // (the label stays above them) until "Show all of it".
            float bx = tx + 12, bw = w - tx - 12;
            var rendered = source.Blocks(body, Folder, Files);
            var m = new MdLayout(sh, source, Copied);
            var b = m.Blocks(rendered.Value ?? Plain(body), Lk(12.5f, 1.6f, new Rgba(0xcf, 0xc6, 0xdc), 400), bw, true);
            pendingImages.AddRange(m.Used);
            waitingNow |= !rendered.Final || m.Waiting;
            var full = Flag(ti, j, 0);
            var top = y + rowH + 2;
            var capped = live && !full && b.H > 168;
            var shown = b.H;
            if (capped)
            {
                // The newest lines that fit, from a whole line: a cut at 168 px exactly would slice a line through the middle.
                var cut = b.H - 168;
                var best = b.H;
                foreach (var tb in b.Frag.Texts)
                    foreach (var ln in tb.Layout.Lines)
                    {
                        var lt = tb.Y + ln.Top;
                        if (lt >= cut - 0.01f && lt < best) best = lt;
                    }
                if (best < b.H) cut = best;
                shown = b.H - cut;
            }
            var bodyFrag = b.Frag;
            if (capped)
            {
                bodyFrag.Shift(0, shown - b.H);
                foreach (var t in bodyFrag.Texts)
                {
                    var c = t.Clip ?? new Rect4(-bx, -1e9f, w + bx, 2e9f);
                    float a = Math.Max(c.Y, 0), z = Math.Min(c.Y + c.H, shown);
                    t.Clip = new Rect4(c.X, a, c.W, Math.Max(z - a, 0));
                }
                bodyFrag.Shapes.RemoveAll(s => s.Top < 0);
            }
            fr.Shapes.Add(Shape.RectOf(tx, top, 2, shown + 4, 0, new Rgba(0xc4, 0xa2, 0xff, 0x33)));
            fr.Append(bodyFrag, bx, top + 2);
            fr.Copy.Add(Tok.Req(1));
            h += shown + 8;
            if (live && (capped || full))
            {
                var mm = Line(full ? "Show less" : "Show all of it", Lk(11, 1.4f, lilac, 500), 0);
                mm.Text = "";
                mm.X = bx;
                mm.Y = y + h;
                fr.Hits.Add(new HitRect(new Rect4(bx - 4, mm.Y - 2, mm.Layout.Width + 8, mm.Layout.Height + 4), new Act(ActKind.Flag, I: j, K: 0)));
                h += mm.Layout.Height + 4;
                fr.Texts.Add(mm);
            }
        }
        return h;
    }

    readonly List<string> pendingImages = [];
    bool waitingNow;

    /// <summary>
    /// The subagents a turn started one after another as one compact list: how many and how they
    /// stand, then a row each with its state, its kind, what it was asked and, once done, how long
    /// it took; four show, the rest behind "Show N more". A row with a result opens to it.
    /// </summary>
    float Agents(Frag fr, Turn t, int ti, int s, int e, float y, float w)
    {
        var list = t.Steps.Skip(s).Take(e - s + 1).ToList();
        const float rowH = 25;
        var iy = y + (rowH - 19) / 2;
        var lilac = new Rgba(0xc4, 0xa2, 0xff);
        int running = list.Count(x => !x.Ended), done = list.Count(x => x.Status == "completed"), failed = list.Count(x => x.Status == "failed");
        var live = running > 0;
        var edge = live ? new Rgba(0xc4, 0xa2, 0xff, 128) : new Rgba(255, 255, 255, 20);
        fr.Shapes.Add(Shape.BoxOf(0, iy, 19, 19, Shape.Round(6), new Rgba(0x1c, 0x1a, 0x20), edge, 1));
        fr.Shapes.Add(Shape.SvgOf(4, iy + 4, 11, 11, Icons.Of(StepIcon.Agent, lilac, 11, 2.2f)));
        var baseLook = Lk(12, 1.3f, new Rgba(255, 255, 255, 148), 400);
        var n = list.Count;
        var sub = running > 0 ? $"{running} of {n} running" : failed > 0 ? $"{done} done · {failed} failed" : $"{done} done";
        var head = Box([new Span { Text = "Subagents", Color = new Rgba(0xf3, 0xf1, 0xf6), Weight = 500 }, Span.GapOf(7), new Span { Text = sub, Color = new Rgba(255, 255, 255, 97), Size = 11.5f }], baseLook, 0);
        head.Text = "";
        head.X = 28;
        head.Y = y + (rowH - 15.6f) / 2;
        head.Shimmer = live;
        fr.Texts.Add(head);
        // The list, 28 px in.
        float bx = 28, bw = w - 28;
        var y0 = y + rowH + 3;
        var at = fr.Shapes.Count;
        var small = Lk(11, 1.3f, new Rgba(0xf6, 0xf2, 0xff, 97), 400);
        // .sh: "6 subagents", then the counts.
        var hx = bx + 9;
        var cy = y0 + 1;
        const float headH = 22;
        var bits = new List<(string S, Rgba C, float W)> { ($"{n} subagent{Plural1(n)}", new Rgba(0xf6, 0xf2, 0xff, 158), 600) };
        if (running > 0) bits.Add(($"{running} running", small.Color, 400));
        if (done > 0) bits.Add(($"{done} done", small.Color, 400));
        if (failed > 0) bits.Add(($"{failed} failed", new Rgba(0xff, 0x6b, 0x62), 400));
        foreach (var b in bits)
        {
            var tb = Line(b.S, small with { Color = b.C, Weight = b.W }, 0);
            tb.Text = "";
            tb.X = hx;
            tb.Y = cy + (headH - tb.Layout.Height) / 2;
            hx += tb.Layout.Width + 10;
            fr.Texts.Add(tb);
        }
        cy += headH;
        fr.Shapes.Add(Shape.RectOf(bx + 1, cy, bw - 2, 1, 0, new Rgba(255, 255, 255, 10)));
        cy++;
        var more = Flag(ti, s, 1);
        var shown = n > 4 && !more ? 4 : n;
        for (var k = 0; k < shown; k++)
        {
            var x = list[k];
            if (k > 0) fr.Shapes.Add(Shape.RectOf(bx + 1, cy, bw - 2, 1, 0, new Rgba(255, 255, 255, 8)));
            const float rh = 26;
            fr.Shapes.Add(Shape.SvgOf(bx + 9, cy + (rh - 14) / 2, 14, 14, Icons.StateDot(x.Status)));
            var rx = bx + bw - 9;
            if (x.HasMs && x.Ended)
            {
                var tb = Line(Turns.ClockText(x.Ms), small, 0);
                tb.Text = "";
                rx -= tb.Layout.Width;
                tb.X = rx;
                tb.Y = cy + (rh - tb.Layout.Height) / 2;
                fr.Texts.Add(tb);
                rx -= 8;
            }
            var lx = bx + 9 + 14 + 8;
            // .ty: the kind it was started as (explore, general…).
            if (x.Cmd.Length > 0)
            {
                var tb = Line(x.Cmd, Lkm(10.5f, 1.3f, new Rgba(0xf6, 0xf2, 0xff, 158), 400), 0);
                var kw = tb.Layout.Width + 10;
                fr.Shapes.Add(Shape.RectOf(lx, cy + (rh - 16) / 2, kw, 16, 5, new Rgba(255, 255, 255, 13)));
                tb.X = lx + 5;
                tb.Y = cy + (rh - tb.Layout.Height) / 2;
                lx += kw + 8;
                fr.Add(tb);
            }
            var title = x.Verb.Length > 0 ? x.Verb : "Subagent";
            var c = x.Status == "failed" ? new Rgba(0xff, 0x9b, 0x94) : new Rgba(0xf6, 0xf2, 0xff, 158);
            var ttb = Line(title, Lk(12, 1.3f, c, 400), 0);
            ttb.X = lx;
            ttb.Y = cy + (rh - ttb.Layout.Height) / 2;
            ttb.Clip = new Rect4(lx, cy, Math.Max(rx - lx, 0), rh);
            ttb.Shimmer = !x.Ended;
            fr.Add(ttb);
            fr.Copy.Add(Tok.Req(1));
            var res = x.Out.Trim();
            if (res.Length > 0) fr.Hits.Add(new HitRect(new Rect4(bx, cy, bw, rh), new Act(ActKind.Flag, I: s, K: (uint)(2 + k))));
            cy += rh;
            if (res.Length > 0 && Flag(ti, s, 2 + k))
            {
                // .res: what it found, under its row.
                var rl = Box([Span.Plain(res)], small with { Size = 11.5f, LH = 1.5f }, bw - 31 - 9);
                rl.X = bx + 31;
                rl.Y = cy + 1;
                fr.Add(rl);
                fr.Copy.Add(Tok.Req(1));
                cy += rl.Layout.Height + 7;
            }
        }
        if (n > 4)
        {
            fr.Shapes.Add(Shape.RectOf(bx + 1, cy, bw - 2, 1, 0, new Rgba(255, 255, 255, 8)));
            var tb = Line(more ? "Show less" : $"Show {n - 4} more", small, 0);
            tb.Text = "";
            tb.X = bx + 9;
            tb.Y = cy + 1 + (22 - tb.Layout.Height) / 2;
            fr.Texts.Add(tb);
            fr.Hits.Add(new HitRect(new Rect4(bx, cy, bw, 23), new Act(ActKind.Flag, I: s, K: 1)));
            cy += 23;
        }
        fr.Shapes.Insert(at, Shape.BoxOf(bx, y0, bw, cy - y0 + 1, Shape.Round(10), new Rgba(255, 255, 255, 5), new Rgba(255, 255, 255, 16), 1));
        return cy + 1 + 4 - y;
    }

    /// <summary>What a finished turn changed, file by file, with its +/− counts.</summary>
    float Changes(Frag fr, Turn t, float y, float w)
    {
        var by = new List<(string Name, int Add, int Del, int Step, bool HasStep)>();
        for (var j = 0; j < t.Steps.Count; j++)
        {
            var x = t.Steps[j];
            if (x.Kind != StepIcon.Edit || x.Name.Length == 0 || (x.Add == 0 && x.Del == 0)) continue;
            var dir = x.Dir;
            var cut = dir.LastIndexOf('/');
            if (cut >= 0) dir = dir[(cut + 1)..];
            var k = dir.Length > 0 ? dir + "/" + x.Name : x.Name;
            // Only this turn's own edits: a click opens the newest change of the file.
            var hasD = x.Diff.Length > 0;
            var idx = by.FindIndex(f => f.Name == k);
            if (idx >= 0)
            {
                var f = by[idx];
                by[idx] = (f.Name, f.Add + x.Add, f.Del + x.Del, hasD ? j : f.Step, hasD || f.HasStep);
            }
            else by.Add((k, x.Add, x.Del, j, hasD));
        }
        if (by.Count == 0) return 0;
        int a = by.Sum(v => v.Add), d = by.Sum(v => v.Del);
        var y0 = y + Palette.ThreadGap;
        var at = fr.Shapes.Count;
        var sans = Lk(12, 1.3f, Palette.Ink, 600);
        // .ch: a flex row, so its words and its span copy as two lines.
        var h1 = Line($"{by.Count} file{Plural1(by.Count)} changed", sans, 0);
        var h2 = Line($"+{a} −{d}", Lk(12, 1.3f, new Rgba(255, 255, 255, 102), 500), 0);
        var hh = h1.Layout.Height + 14;
        h1.X = 11; h1.Y = y0 + 7;
        h2.X = 11 + h1.Layout.Width + 8; h2.Y = y0 + 7;
        fr.Add(h1);
        fr.Copy.Add(Tok.Req(1));
        fr.Add(h2);
        fr.Copy.Add(Tok.Req(1));
        fr.Shapes.Add(Shape.RectOf(1, y0 + hh, w - 2, 1, 0, new Rgba(255, 255, 255, 15)));
        var cy = y0 + hh + 1;
        var mono = Lkm(11.5f, 1.3f, new Rgba(255, 255, 255, 191), 400);
        for (var k = 0; k < by.Count; k++)
        {
            var f = by[k];
            if (k > 0) fr.Shapes.Add(Shape.RectOf(1, cy, w - 2, 1, 0, new Rgba(255, 255, 255, 10)));
            var tb = Line(f.Name, mono, 0);
            var rh = tb.Layout.Height + 10;
            if (f.HasStep) fr.Hits.Add(new HitRect(new Rect4(0, cy, w, rh), new Act(ActKind.OpenDiff, I: f.Step)));
            tb.X = 11;
            tb.Y = cy + 5;
            var rx = w - 11;
            var nums = new List<TextBox>();
            foreach (var (s, c) in new[] { (f.Del == 0 ? "" : $"−{f.Del}", new Rgba(0xff, 0x7b, 0x72)), (f.Add == 0 ? "" : $"+{f.Add}", new Rgba(0x5d, 0xe3, 0x7a)) })
            {
                if (s.Length == 0) continue;
                var n = Line(s, mono with { Color = c }, 0);
                rx -= n.Layout.Width;
                n.X = rx;
                n.Y = cy + 5;
                nums.Add(n);
                rx -= 6;
            }
            tb.Clip = new Rect4(11, cy, Math.Max(rx - 19, 0), rh);
            fr.Add(tb);
            fr.Copy.Add(Tok.Req(1));
            // In DOM order: + before −, each a flex item of its own.
            for (var i = nums.Count - 1; i >= 0; i--)
            {
                fr.Add(nums[i]);
                fr.Copy.Add(Tok.Req(1));
            }
            cy += rh;
        }
        fr.Shapes.Insert(at, Shape.BoxOf(0, y0, w, cy - y0 + 1, Shape.Round(12), new Rgba(255, 255, 255, 5), new Rgba(255, 255, 255, 20), 1));
        return cy + 1 - y;
    }
}
