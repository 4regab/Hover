using Hover.Chat.Desk;
using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

public sealed partial class OfficeUi
{
    // The desk card (office.slint's DeskCard, internal/ui/deskcard.go): who works here and on what, what it does now (its
    // last steps, the question it waits on, or the answer it gave), its eight surfaces as tiles, a reply box and the chat.

    const float CardMaxW = 408;
    const int AnswerLines = 4;

    /// <summary>Where the card sits in the last frame (for the tests and the dump).</summary>
    public Rect4 DeskCardRect { get; private set; }

    static int StageNumber(string stage) => stage switch
    {
        "working" => 1,
        "done" => 2,
        "failed" => 3,
        "stopped" => 4,
        "waiting" => 5,
        _ => 0,
    };

    static string StageWord(int stage) => stage switch
    {
        1 => "Working",
        2 => "Done",
        3 => "Couldn’t finish",
        4 => "Stopped",
        5 => "Waiting for you",
        _ => "Waking up",
    };

    /// <summary>A step as the card's live rows show it: its kind (for the icon), colour, verb and target.</summary>
    sealed record CardStep(string Kind, Rgba Color, string Name, string Text, bool Live, bool Fail);

    static CardStep StepLine(StepInfo x, bool live)
    {
        var kind = x.K switch
        {
            "read" or "edit" or "run" or "search" or "thought" or "agent" or "web" => x.K,
            _ => "think",
        };
        var color = kind switch
        {
            "edit" => 0xc9a8ffu, "run" => 0xffc46bu, "search" => 0x6fd6c9u, "read" => 0x8fb6ffu,
            "thought" or "agent" => 0xc4a2ffu, "web" => 0x7cc0ffu, _ => 0xa0a0a8u,
        };
        var target = x.Cmd ?? (x.Name is { Length: > 0 } n ? (x.Dir is { Length: > 0 } d ? d.TrimEnd('/') + "/" + n : n) : "");
        if (kind is "run" or "search")
        {
            var nl = target.IndexOf('\n');
            if (nl >= 0) target = target[..nl];
            target = target.TrimEnd('\r');
        }
        return new CardStep(kind, Rgba.Of(color), x.Verb, target, live, x.Status == "failed");
    }

    /// <summary>The answer as one plain line: no code blocks, marks or link addresses, cut at 220 characters.</summary>
    public static string PlainLine(string answer)
    {
        var sb = new System.Text.StringBuilder();
        var fence = false;
        foreach (var raw in answer.Replace("\r\n", "\n").Split('\n'))
        {
            if (raw.TrimStart().StartsWith("```", StringComparison.Ordinal))
            {
                fence = !fence;
                sb.Append(' ');
                continue;
            }
            if (fence) continue;
            sb.Append(raw).Append(' ');
        }
        // [text](address) is its text.
        var cs = sb.ToString();
        var t = new System.Text.StringBuilder();
        for (var i = 0; i < cs.Length;)
        {
            if (cs[i] == '[')
            {
                var close = cs.IndexOf(']', i);
                if (close > 0 && close + 1 < cs.Length && cs[close + 1] == '(')
                {
                    var end = cs.IndexOf(')', close);
                    if (end > 0)
                    {
                        t.Append(cs, i + 1, close - i - 1);
                        i = end + 1;
                        continue;
                    }
                }
            }
            t.Append(cs[i]);
            i++;
        }
        var chars = t.ToString().Select(c => "#*_`>|-".Contains(c) ? ' ' : c).ToArray();
        var s = string.Join(' ', new string(chars).Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries));
        var runes = s.EnumerateRunes().ToList();
        return runes.Count > 220 ? string.Concat(runes.Take(219)) + "…" : s;
    }

    /// <summary>Text cut to a number of lines with an ellipsis where it was cut; returns it and its height.</summary>
    (string Text, float Height) CutLines(string text, Look look, float width, int maxLines)
    {
        var shaped = pen.Shape(text, look, width);
        var lines = shaped.Layout.Lines;
        if (lines.Count <= maxLines) return (text, shaped.Layout.Height);
        var last = lines[maxLines - 1];
        var kept = text[..last.Start] + pen.Fit(text[last.Start..].Replace("\n", " "), look, width);
        return (kept, last.Top + last.Height);
    }

    float TilesHeight(bool tight) => 2 * (tight ? 26 : 36) + 4;

    /// <summary>The card: measured when <paramref name="dry"/>, else drawn at (x, y). Returns its height.</summary>
    float PaintDeskCard(SessionInfo s, float x, float y, float maxW, float maxH, bool dry)
    {
        var id = s.Id;
        var tight = maxH < 340;
        var w = Math.Min(CardMaxW, maxW);
        var iw = w - 12;
        var nw2 = iw - 16;
        var stage = StageNumber(s.Stage);
        var busy = Busy(s);
        var ask = s.Ask;
        var question = ask?.Questions is { Count: > 0 };
        var turn = s.Turns.LastOrDefault(t => !t.Queued);
        var steps = busy && turn is not null ? turn.Steps : [];
        var live = new List<CardStep>();
        string line = "", answer = "", meta = "";
        if (ask is null)
        {
            if (busy)
            {
                for (var i = Math.Max(steps.Count - 3, 0); i < steps.Count; i++)
                    live.Add(StepLine(steps[i], i + 1 == steps.Count && steps[i].Status is not ("completed" or "failed")));
                if (live.Count == 0) line = "Getting started…";
            }
            else if (turn is not null && turn.Answer.Trim().Length > 0)
            {
                answer = PlainLine(turn.Answer);
                var edits = turn.Steps.Where(st => st.K == "edit").Select(st => st.Name ?? st.Verb).Distinct().Count();
                var parts = new List<string> { $"{turn.Steps.Count} step{(turn.Steps.Count == 1 ? "" : "s")}" };
                if (edits > 0) parts.Add($"{edits} file{(edits == 1 ? "" : "s")} edited");
                meta = string.Join(" · ", parts);
            }
            else line = "Nothing yet.";
        }
        var tiles = Tiles(s);
        var box = CardBox(id);
        var f12 = Pen.F(12, 400, Pen.Ink);
        var lh = pen.LineHeight(f12);
        var hd = 2 + Math.Max(30, 20 + 2 + pen.LineHeight(Pen.F(13, 600, Pen.Ink))) + 1;
        // The status box's content.
        float nowH = 6 + 7;
        var f125 = Pen.F(12.5, 400, Pen.Ink);
        string cutAnswer = "";
        if (ask is not null)
        {
            nowH += lh;
            if (AskLineOf(ask).Length > 0) nowH += 2 + lh;
            nowH += 2 + 4 + 26;
        }
        else if (live.Count > 0) nowH += live.Count * 22;
        else if (answer.Length == 0) nowH += Math.Min(lh * 2, 36);
        else
        {
            var (cut, ah) = CutLines(answer, f125, w - 12 - 16, AnswerLines);
            cutAnswer = cut;
            nowH += ah;
            if (meta.Length > 0) nowH += 2 + lh;
        }
        var tw = w - 12 - 20 - 6;
        var ih = box.Text.Length == 0 ? pen.LineHeight(f125) : pen.WrappedHeight(box.Text, tw, f125);
        var input = Math.Max(28, Math.Min(96, Math.Max(ih, pen.LineHeight(f125)) + 6)) + 6;
        var rest = 2 * 6 + hd + TilesHeight(tight) + input + 28 + 4 * 5;
        var nowCap = Math.Max(56, Math.Min(104, maxH - rest));
        var h = Math.Min(rest + Math.Min(nowH, nowCap), maxH);
        if (dry) return h;

        var bot = Bots.Color(s.Bot);
        DeskCardRect = new Rect4(x, y, w, h);
        for (var i = 0; i < 5; i++) pen.Box(x - i * 3, y + 18 - i * 2, w + i * 6, h + i * 3, 16 + i * 3, Rgba.Of(0x000000, 0.1));
        pen.Box(x, y, w, h, 16, Rgba.Of(0x140f18, 0.97));
        pen.Border(x, y, w, h, 16, 1, Rgba.Of(0xffffff, 0.09));
        Block(DeskCardRect);
        Add(DeskCardRect, "deskCard");
        pen.Cv.PushClip(new Rect4(x, y, w, h));
        pen.Box(x, y, w, 56, 0, bot with { A = 51 });
        var ix = x + 6;
        var cy = y + 6;
        var hy = cy + 2;
        pen.Mark(s.Tool.Length > 0 ? s.Tool : "kiro", ix, hy, 30);
        var dot = stage switch { 0 or 1 => Rgba.Of(0xc4a2ff), 2 => Rgba.Of(0x30d158), 3 => Rgba.Of(0xff453a), 5 => Rgba.Of(0xffb340), _ => Rgba.Of(0x8e8e93) };
        pen.Box(ix + 22, hy + 22, 10, 10, 5, dot);
        pen.Border(ix + 22, hy + 22, 10, 10, 5, 2, Rgba.Of(0x17141b));
        var tx = ix + 30 + 9;
        var nameLook = Pen.F(11.5, 600, Mix(Rgba.Of(0xffffff), bot, 0.45f));
        var name = (Bots.Name(s.Bot) + "’s desk").ToUpperInvariant();
        var nw = pen.Width(name, nameLook);
        pen.Text(name, tx, hy + (20 - pen.LineHeight(nameLook)) / 2, nameLook);
        var clock = "";
        if (turn is not null)
        {
            var started = turn.T0;
            var nowMs = new DateTimeOffset(DateTime.SpecifyKind(env.Utc(), DateTimeKind.Utc)).ToUnixTimeMilliseconds();
            if (busy && started > 0) clock = Turns.ClockText(nowMs - started);
            else if (!busy && turn.Took is { } took && took > 0) clock = DeskRows.Dur(took);
        }
        var what = StageWord(stage) + (clock.Length > 0 ? " · " + clock : "");
        var (cfg, cbg) = stage switch
        {
            0 or 1 => (Rgba.Of(0xd9c4ff), Rgba.Of(0x9b6bff, 0.14)),
            2 => (Rgba.Of(0x7ee59a), Rgba.Of(0x30d158, 0.1)),
            3 => (Rgba.Of(0xff9a92), Rgba.Of(0xff453a, 0.12)),
            5 => (Rgba.Of(0xffc46b), Rgba.Of(0xffb340, 0.12)),
            _ => (Rgba.Of(0xf6f2ff, 0.6), Rgba.Of(0xffffff, 0.055)),
        };
        var cx = tx + nw + 8;
        var grey = Rgba.Of(0xffffff, 0.6);
        var greyBg = Rgba.Of(0xffffff, 0.055);
        cx += CardChip(null, what, cfg, cbg, cx, hy, 0) + 5;
        var accessId = s.Access.Length > 0 ? s.Access : "full";
        var folderName = s.Folder.Length > 0 ? LastPart(s.Folder) : "";
        var rowRight = x + w - 6;
        var buttonsW = CardChipW(DeskIcons.Code, "Editor") + 5 + CardChipW(DeskIcons.Expand, "Expand") + 5;
        if (folderName.Length > 0)
        {
            var fw = CardChipW(DeskIcons.Folder, folderName);
            var over = cx + fw + 5 + buttonsW + 40 - rowRight;
            cx += CardChip(DeskIcons.Folder, folderName, grey, greyBg, cx, hy, over > 0 ? Math.Max(fw - over, 48) : 0) + 5;
        }
        if (accessId.Length > 0)
        {
            var amber = accessId == "full";
            cx += CardChip(DeskIcons.Shield, AccessLabel(accessId), amber ? Rgba.Of(0xffc46b) : grey, amber ? Rgba.Of(0xffb340, 0.09) : greyBg, cx, hy, 0) + 5;
        }
        if (s.Ctx is { } ctx) cx += CardChip(null, ctx + "%", grey, greyBg, cx, hy, 0) + 5;
        var ed = CardChipButton("cardEditor", DeskIcons.Code, "Editor", cx, hy);
        cx += ed + 5;
        var ex = CardChipButton("cardExpand", DeskIcons.Expand, "Expand", cx, hy);
        var rowEnd = cx + ex;
        pen.Text(s.Title.Length > 0 ? s.Title : "New chat", tx, hy + 20 + 2, Pen.F(13, 600, Rgba.Of(0xffffff)), 0);
        cy += hd + 5;

        // The status box: its last steps, the question, or the answer.
        var shownNow = Math.Min(nowH, nowCap);
        pen.Box(ix, cy, iw, shownNow, 11, Rgba.Of(0xffffff, 0.03));
        pen.Border(ix, cy, iw, shownNow, 11, 1, Rgba.Of(0xffffff, 0.05));
        pen.Cv.PushClip(new Rect4(ix, cy, iw, shownNow));
        var ny = cy + 6;
        var nx = ix + 8;
        if (ask is not null)
        {
            pen.Text(AskTitleOf(ask), nx, ny, Pen.F(12, 600, Rgba.Of(0xffd59a)), nw2);
            ny += lh;
            var al = AskLineOf(ask);
            if (al.Length > 0)
            {
                pen.Text(al, nx, ny + 2, Pen.F(12, 400, Rgba.Of(0xffd59a, 0.75)), nw2);
                ny += 2 + lh;
            }
            ny += 2 + 4;
            if (!question)
            {
                var deny = CardAskButton("cardDeny", "Deny", 0, nx, ny);
                CardAskButton("cardReview", "Review", 0, nx + deny + 6, ny);
                var allowText = ask.Allow.Length > 0 ? ask.Allow : "Allow";
                var aw = pen.Width(allowText, Pen.F(11.5, 600, Pen.Ink)) + 20;
                var tr = pen.Width("Trust", Pen.F(11.5, 600, Pen.Ink)) + 20;
                CardAskButton("cardAllow", allowText, ask.Danger ? 2 : 1, nx + nw2 - aw, ny);
                CardAskButton("cardTrust", "Trust", 0, nx + nw2 - aw - 6 - tr, ny);
            }
            else
            {
                CardAskButton("cardSkip", "Skip", 0, nx, ny);
                var aw = pen.Width("Answer…", Pen.F(11.5, 600, Pen.Ink)) + 20;
                CardAskButton("cardAnswer", "Answer…", 1, nx + nw2 - aw, ny);
            }
        }
        else if (live.Count > 0)
        {
            foreach (var st in live)
            {
                pen.Box(nx + 2, ny + 2, 18, 18, 6, st.Color with { A = 36 });
                if (st.Live) pen.Border(nx + 2, ny + 2, 18, 18, 6, 1, Rgba.Of(0xffc46b, 0.5));
                pen.Icon(DeskIcons.Step(st.Kind), nx + 2 + 3, ny + 2 + 3, 12, st.Fail ? Rgba.Of(0xff7b72) : st.Color);
                var nl = Pen.F(12, 600, st.Fail ? Rgba.Of(0xff9a92) : Rgba.Of(0xf3f1f6));
                var nameW = pen.Text(st.Name, nx + 2 + 18 + 8, ny + (22 - pen.LineHeight(nl)) / 2, nl);
                var txx = nx + 2 + 18 + 8 + nameW + 8;
                var ml = Pen.Mono(11, Pen.Dim);
                pen.Text(st.Text, txx, ny + (22 - pen.LineHeight(ml)) / 2, ml, Math.Max(nx + nw2 - txx, 0));
                ny += 22;
            }
        }
        else if (answer.Length == 0) pen.Wrapped(line, nx, ny, nw2, Pen.F(12, 400, Pen.Dim));
        else
        {
            var col = stage == 3 ? Rgba.Of(0xffb0aa) : Rgba.Of(0xe9e7ec);
            var ah = pen.Wrapped(cutAnswer, nx, ny, nw2, f125 with { Color = col });
            ny += ah;
            if (meta.Length > 0) pen.Text(meta, nx, ny + 2, Pen.F(12, 400, Pen.Dim));
        }
        pen.Cv.PopClip();
        cy += shownNow + 5;

        // The tiles, four to a row.
        var th = tight ? 26f : 36f;
        for (var i = 0; i < tiles.Count; i++)
        {
            var t = tiles[i];
            var tlx = ix + i % 4 * (iw + 4) / 4;
            var tly = cy + i / 4 * (th + 4);
            var tlw = (iw - 12) / 4;
            var tr = new Rect4(tlx, tly, tlw, th);
            var hov = Over(tr) && t.Enabled;
            pen.Box(tr, 9, hov ? bot with { A = 51 } : Rgba.Of(0xffffff, t.Enabled ? 0.043 : 0.027));
            pen.Border(tlx, tly, tlw, th, 9, 1, hov ? bot with { A = 102 } : Rgba.Of(0xffffff, 0.05));
            var detail = t.Enabled ? t.Detail : t.Reason;
            var rows = 15f;
            if (!tight) rows += 1 + pen.LineHeight(Pen.F(11, 500, Pen.Ink));
            var ry = tly + (th - rows) / 2;
            var tix = tlx + 7;
            var icol = t.Enabled ? Rgba.Of(0xececf0, 0.9) : Rgba.Of(0xececf0, 0.38);
            pen.Icon(DeskIcons.Of(t.Id), tix, ry + (15 - 14) / 2f, 14, icol);
            if (t.Badge > 0)
            {
                var bw = Math.Max(14, pen.Width(t.Badge.ToString(), Pen.F(9, 600, Pen.Ink)) + 6);
                pen.Box(tix + 9, ry - 6, bw, 14, 7, Rgba.Of(0x3b82f6));
                pen.TextIn(t.Badge.ToString(), new Rect4(tix + 9, ry - 6, bw, 14), Pen.F(9, 600, Rgba.Of(0xffffff)));
            }
            var title = t.Title == "Linked pull requests" ? "Linked PRs" : t.Title;
            var tl = Pen.F(11, 600, t.Enabled ? Rgba.Of(0xececf0) : Rgba.Of(0xececf0, 0.38));
            pen.Text(title, tix + 14 + 5, ry + (15 - pen.LineHeight(tl)) / 2, tl, Math.Max(tlx + tlw - 5 - (tix + 14 + 5), 0));
            if (!tight) pen.Text(detail, tix, ry + 15 + 1, Pen.F(11, 500, Rgba.Of(0xececf0, 0.45)), Math.Max(tlw - 12, 0));
            Add(tr, "cardTile", i, t.Id);
        }
        cy += TilesHeight(tight) + 5;

        // The reply, and the round button that sends it or stops the run.
        pen.Box(ix, cy, iw, input, 13, Rgba.Of(0x1b181f));
        var foc = focus == Focus.Desk && deskField?.Key == "card";
        pen.Border(ix, cy, iw, input, 13, 1, foc ? Rgba.Of(0xc4a2ff, 0.55) : Rgba.Of(0xffffff, 0.08));
        var placeholder = ask is not null ? "Or tell " + Bots.Name(s.Bot) + " what to do instead…"
            : busy ? "Reply. " + Bots.Name(s.Bot) + " reads it after this run" : "Reply to " + Bots.Name(s.Bot) + "…";
        var area = new Rect4(ix + 10, cy + 3 + 4, iw - 10 - 3 - 30 - 6, input - 6 - 4);
        PaintField("card", true, area, f125 with { Color = Rgba.Of(0xf6f2ff) }, placeholder);
        var empty = box.Text.Length == 0;
        var enabled = !empty || (busy && !s.Stopping && ask is null);
        var stop = empty && busy && ask is null;
        var go = new Rect4(ix + iw - 3 - 30, cy + input - 3 - 30, 30, 30);
        pen.Box(go, 15, !enabled ? Rgba.Of(0xffffff, 0.07) : Over(go) ? Rgba.Of(0xffffff) : Rgba.Of(0xf6f2ff));
        if (stop) pen.Box(go.X + 9.5f, go.Y + 9.5f, 11, 11, 2.5f, Rgba.Of(0x000000));
        else pen.Icon(DeskIcons.Up, go.X + 7, go.Y + 7, 16, enabled ? Rgba.Of(0x0c0b0e) : Rgba.Of(0xf6f2ff, 0.28));
        Add(go, enabled ? (stop ? "cardStop" : "cardSend") : "cardSendOff");
        cy += input + 5;

        // The chat itself.
        var chat = new Rect4(ix, cy, iw, 28);
        var chov = Over(chat);
        pen.Box(chat, 10, bot with { A = (byte)(chov ? 82 : 51) });
        pen.Border(ix, cy, iw, 28, 10, 1, bot with { A = 89 });
        pen.Icon(DeskIcons.Chat, ix + 10, cy + 6, 16, Rgba.Of(0xffffff));
        pen.Text("Open the chat with " + Bots.Name(s.Bot), ix + 10 + 16 + 10, cy + (28 - pen.LineHeight(Pen.F(12.5, 600, Pen.Ink))) / 2, Pen.F(12.5, 600, Rgba.Of(0xffffff)), iw - 20 - 16 - 10 - 13 - 10);
        pen.Icon(DeskIcons.Enter, ix + iw - 10 - 13, cy + 7.5f, 13, Rgba.Of(0xffffff, 0.55));
        Add(chat, "cardChat");
        pen.Cv.PopClip();
        _ = rowEnd;
        return h;
    }

    static Rgba Mix(Rgba a, Rgba b, float t) => new(
        (byte)(a.R + (b.R - a.R) * t), (byte)(a.G + (b.G - a.G) * t), (byte)(a.B + (b.B - a.B) * t), 255);

    string AskTitleOf(AskInfo a) => a.Title.Length > 0 ? a.Title : a.Questions is { Count: > 0 } ? "Has a question" : "Wants to continue";

    static string AskLineOf(AskInfo a)
    {
        if (a.Questions is { Count: > 0 } q) return q[0].Question;
        if (!string.IsNullOrEmpty(a.Command)) return a.Command;
        if (!string.IsNullOrEmpty(a.Path)) return a.Path;
        return a.Reason;
    }

    float CardChipW(string? icon, string text)
    {
        var w = 7 + pen.Width(text, Pen.F(11, 500, Pen.Ink)) + 7;
        return icon is not null ? w + 11 + 5 : w;
    }

    /// <summary>A 20 px chip with an icon and a word; <paramref name="forceW"/> (more than 0) squeezes it. Returns its width.</summary>
    float CardChip(string? icon, string text, Rgba fg, Rgba bg, float x, float y, float forceW)
    {
        var w = forceW > 0 ? forceW : CardChipW(icon, text);
        pen.Box(x, y, w, 20, 6, bg);
        var cx = x + 7;
        if (icon is not null)
        {
            pen.Icon(icon, cx, y + (20 - 11) / 2f, 11, fg);
            cx += 11 + 5;
        }
        var look = Pen.F(11, 500, fg);
        pen.Text(text, cx, y + (20 - pen.LineHeight(look)) / 2, look, Math.Max(x + w - 7 - cx, 0));
        return w;
    }

    float CardChipButton(string id, string icon, string text, float x, float y)
    {
        var w = CardChipW(icon, text);
        var r = new Rect4(x, y, w, 20);
        CardChip(icon, text, Rgba.Of(0xffffff, 0.6), Rgba.Of(0xffffff, 0.055), x, y, 0);
        if (Over(r)) pen.Box(r, 6, Rgba.Of(0xffffff, 0.08));
        Add(r, id);
        return w;
    }

    /// <summary>An ask button of the card: 0 plain, 1 light, 2 red. Returns its width.</summary>
    float CardAskButton(string id, string text, int kind, float x, float y)
    {
        var look = Pen.F(11.5, 600, Pen.Ink);
        var w = pen.Width(text, look) + 20;
        var r = new Rect4(x, y, w, 26);
        var hov = Over(r);
        var (bg, ink) = kind switch
        {
            1 => (hov ? Rgba.Of(0xffffff) : Rgba.Of(0xf5f5f7), Rgba.Of(0x000000)),
            2 => (Rgba.Of(0xff453a), Rgba.Of(0xffffff)),
            _ => (Rgba.Of(0xffffff, hov ? 0.17 : 0.1), Rgba.Of(0xffffff)),
        };
        pen.Box(r, 8, bg);
        pen.TextIn(text, r, look with { Color = ink });
        Add(r, id);
        return w;
    }

    /// <summary>Where the card goes: by its desk's bot when the office says where that is, else at the top right.</summary>
    (float X, float Y) CardPlace(int id, float cw, float ch)
    {
        float cx, cy;
        if (env.BotRect(id) is { } bot)
        {
            cx = bot.X + bot.W + 6;
            cy = bot.Y - 12;
            if (cx + cw > w - 8) cx = bot.X - cw - 6;
        }
        else
        {
            cx = w - cw - 20;
            cy = 56;
        }
        return (Math.Max(8, Math.Min(cx, w - cw - 8)), Math.Max(8, Math.Min(cy, h - ch - 8)));
    }

    void PaintDeskCardLayer()
    {
        if (deskCard is not { } id || DeskSession(id) is not { } s) return;
        // A click anywhere else puts it away.
        Add(new Rect4(0, 0, w, h), "cardAway");
        var maxW = w - 16;
        var maxH = h - 16;
        var ch = PaintDeskCard(s, 0, 0, maxW, maxH, true);
        var cw = Math.Min(CardMaxW, maxW);
        var (x, y) = CardPlace(id, cw, ch);
        PaintDeskCard(s, x, y, maxW, maxH, false);
    }

    /// <summary>The card's buttons, by id.</summary>
    void CardClick(Target t)
    {
        if (deskCard is not { } id || DeskSession(id) is not { } s) return;
        switch (t.Id)
        {
            case "cardAway": CloseDeskCard(); break;
            case "cardTile": PickTile(t.S); break;
            case "cardEditor": DeskOpenEditor(); break;
            case "cardExpand": DeskCardExpand(); break;
            case "cardChat": DeskCardChat(); break;
            case "cardSend": case "cardStop": DeskCardSend(); break;
            case "cardSendOff": FocusField(Fld("card", true), false); break;
            case "cardReview": DeskCardChat(); break;
            case "cardAnswer": DeskCardChat(); break;
            case "cardDeny": case "cardSkip": if (s.Ask is { } d) env.Send(new AnswerCommand(id, d.Id, "deny")); break;
            case "cardAllow": if (s.Ask is { } a) env.Send(new AnswerCommand(id, a.Id, "allow")); break;
            case "cardTrust": if (s.Ask is { } tr) env.Send(new AnswerCommand(id, tr.Id, "trust")); break;
        }
    }
}
