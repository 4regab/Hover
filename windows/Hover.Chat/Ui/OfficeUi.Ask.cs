using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

public sealed partial class OfficeUi
{
    sealed class AskState
    {
        public readonly List<HashSet<string>> Sel = [];
        public readonly List<EditBox> Custom = [];

        public void Ensure(int n)
        {
            while (Sel.Count < n) Sel.Add([]);
            while (Custom.Count < n) Custom.Add(new EditBox());
        }
    }

    readonly Dictionary<string, AskState> asks = [];
    EditBox? customBox;
    int customQ;
    /// <summary>The question the custom-answer box in use belongs to, so Enter answers that one and not the first that asks.</summary>
    string customAsk = "";

    AskState AskOf(AskInfo a)
    {
        if (!asks.TryGetValue(a.Id, out var st))
        {
            if (asks.Count > 64) asks.Clear();
            asks[a.Id] = st = new AskState();
        }
        st.Ensure(a.Questions?.Count ?? 0);
        return st;
    }

    static readonly Look MonoAsk = Pen.Mono(11.5f, Palette.Ink);

    Rect4 AskButton(string label, int kind, float tall, float x, float y, string id, int n = 0, string s = "", bool dry = false)
    {
        var f = Pen.F(11.5, 600, Rgba.Of(0xffffff));
        var bw = pen.Width(label, f) + 20;
        var r = new Rect4(x, y, bw, tall);
        if (dry) return r;
        var hov = Over(r);
        Rgba bg, fg;
        switch (kind)
        {
            case 1: bg = hov ? Rgba.Of(0xffffff) : Rgba.Of(0xf5f5f7); fg = Rgba.Of(0x000000); break;
            case 2: bg = Rgba.Of(0xff453a); fg = Rgba.Of(0xffffff); break;
            default: bg = Rgba.Of(0xffffff, hov ? 0.17 : 0.1); fg = Rgba.Of(0xffffff); break;
        }
        pen.Box(r, 8, bg);
        pen.TextIn(label, r, f with { Color = fg });
        Add(r, id, n, s);
        return r;
    }

    /// <summary>
    /// The question as a card, over the bot's head (<paramref name="over"/>) or at the end of its chat:
    /// Deny, Trust and the allow button; or, for a question, its choices. Lays it out with its top left
    /// at (x, y) and returns its height. <paramref name="dry"/> measures only.
    /// </summary>
    float AskCard(AskInfo a, SessionInfo s, bool over, float cw, float x, float y, bool dry)
    {
        var st = AskOf(a);
        var pad = over ? 8f : 10f;
        var iw = cw - 2 * pad;
        var cy = y + pad;
        var cx = x + pad;
        var titleLook = Pen.F(12.5, 600, Rgba.Of(0xffffff));
        var hh = Math.Max(16, pen.LineHeight(titleLook));
        if (!dry) pen.Mark(s.Tool.Length > 0 ? s.Tool : "kiro", cx, cy + (hh - 16) / 2, 16);
        var mx = cx + iw;
        if (a.More > 0)
        {
            var ml = Pen.F(10.5, 400, Rgba.Of(0xffffff, 0.45));
            var mt = "+" + a.More + " more";
            var mw = pen.Width(mt, ml);
            if (!dry) pen.Text(mt, mx - mw, cy + (hh - pen.LineHeight(ml)) / 2, ml);
            mx -= mw + 7;
        }
        if (!dry) pen.Text(a.Title, cx + 16 + 7, cy + (hh - pen.LineHeight(titleLook)) / 2, titleLook, Math.Max(mx - (cx + 16 + 7), 0));
        cy += hh;

        if (a.Questions is null or { Count: 0 })
        {
            cy += over ? 6 : 8;
            string command = a.Command ?? "", path = a.Path ?? "", preview = a.Preview ?? "";
            float Mono(string t, float wrapW) => pen.HeightWhenWrapped(t, wrapW, MonoAsk);
            float preH;
            var dollarW = pen.Width("$", MonoAsk);
            if (command.Length > 0) preH = Mono(command, iw - 18 - dollarW - 8);
            else if (path.Length > 0 && preview.Length > 0) preH = pen.LineHeight(MonoAsk) + Mono(preview, iw - 18);
            else if (preview.Length > 0) preH = Mono(preview, iw - 18);
            else preH = Mono(path.Length > 0 ? path : a.Title, iw - 18);
            var bh = Math.Min(preH + 14, over ? 58 : 120);
            if (!dry)
            {
                pen.Box(cx, cy, iw, bh, 9, Rgba.Of(0x000000, 0.45));
                pen.Cv.PushClip(new Rect4(cx, cy, iw, bh));
                float ty = cy + 7, tx = cx + 9, wrapW = iw - 18;
                if (command.Length > 0)
                {
                    pen.Text("$", tx, ty, MonoAsk with { Color = Rgba.Of(0xffb340) });
                    pen.Wrapped(command, tx + dollarW + 8, ty, wrapW - dollarW - 8, MonoAsk with { Color = Rgba.Of(0xffffff) });
                }
                else if (path.Length > 0 && preview.Length > 0)
                {
                    pen.Text(path, tx, ty, MonoAsk with { Color = Rgba.Of(0xffffff, 0.5) });
                    pen.Wrapped(preview, tx, ty + pen.LineHeight(MonoAsk), wrapW, MonoAsk with { Color = Rgba.Of(0xffffff) });
                }
                else pen.Wrapped(preview.Length > 0 ? preview : path.Length > 0 ? path : a.Title, tx, ty, wrapW, MonoAsk with { Color = Rgba.Of(0xffffff) });
                pen.Cv.PopClip();
            }
            cy += bh + (over ? 5 : 7);
            var rl = Pen.F(11, 400, Rgba.Of(0xffffff, 0.58));
            var rh = pen.LineHeight(rl);
            var reason = a.Reason.Length > 0 ? a.Reason : Hover.Notch.Rest.AskReason(a);
            if (!dry)
            {
                pen.Box(cx, cy + (rh - 6) / 2, 6, 6, 3, a.Danger ? Rgba.Of(0xff453a) : Rgba.Of(0xffb340));
                pen.Text(reason, cx + 6 + 7, cy, rl, Math.Max(iw - 13, 0));
            }
            cy += rh + (over ? 7 : 9);
            var tall = over ? 24f : 26f;
            var allow = a.Allow.Length > 0 ? a.Allow : "Allow";
            var aw = pen.Width(allow, Pen.F(11.5, 600, Rgba.Of(0xffffff))) + 20;
            var tw = pen.Width("Trust", Pen.F(11.5, 600, Rgba.Of(0xffffff))) + 20;
            AskButton("Deny", 0, tall, cx, cy, "askDeny", 0, a.Id, dry);
            AskButton("Trust", 0, tall, cx + iw - aw - 6 - tw, cy, "askTrust", 0, a.Id, dry);
            AskButton(allow, a.Danger ? 2 : 1, tall, cx + iw - aw, cy, "askAllow", 0, a.Id, dry);
            cy += tall;
        }
        else if (over)
        {
            cy += 8;
            var qf = Pen.F(10.5, 600, Rgba.Of(0xffb340));
            var q = a.Questions![0];
            if (!dry) pen.Text(q.Header.ToUpperInvariant(), cx, cy, qf);
            cy += pen.LineHeight(qf) + 3;
            var ql = Pen.F(12, 400, Rgba.Of(0xffffff, 0.86));
            var th = Math.Min(pen.HeightWhenWrapped(q.Question, iw, ql), 31);
            if (!dry)
            {
                pen.Cv.PushClip(new Rect4(cx, cy, iw, 31));
                pen.Wrapped(q.Question, cx, cy, iw, ql);
                pen.Cv.PopClip();
            }
            cy += th + 6;
            var aw = pen.Width("Answer…", Pen.F(11.5, 600, Rgba.Of(0xffffff))) + 20;
            AskButton("Skip", 0, 24, cx, cy, "askDeny", 0, a.Id, dry);
            AskButton("Answer…", 1, 24, cx + iw - aw, cy, "askOpen", 0, a.Id, dry);
            cy += 24 + 2;
        }
        else
        {
            for (var qi = 0; qi < a.Questions!.Count; qi++)
            {
                var q = a.Questions[qi];
                cy += 8;
                var qf = Pen.F(10.5, 600, Rgba.Of(0xffb340));
                if (!dry) pen.Text(q.Header.ToUpperInvariant(), cx, cy, qf);
                cy += pen.LineHeight(qf) + 3;
                var ql = Pen.F(12, 400, Rgba.Of(0xffffff, 0.86));
                if (!dry) pen.Wrapped(q.Question, cx, cy, iw, ql);
                cy += pen.HeightWhenWrapped(q.Question, iw, ql) + 6;
                var of = Pen.F(11.5, 500, Rgba.Of(0xffffff));
                var df = Pen.F(10.5, 400, Rgba.Of(0xffffff, 0.55));
                foreach (var op in q.Options)
                {
                    var lh = pen.HeightWhenWrapped(op.Label, iw - 18, of);
                    var ih = 10 + lh;
                    if (op.Description.Length > 0) ih += 1 + pen.HeightWhenWrapped(op.Description, iw - 18, df);
                    var oh = Math.Max(26, ih);
                    if (!dry)
                    {
                        var r = new Rect4(cx, cy, iw, oh);
                        var on = st.Sel[qi].Contains(op.Label);
                        if (on)
                        {
                            pen.Box(r, 8, Rgba.Of(0xffb340, 0.22));
                            pen.Border(cx, cy, iw, oh, 8, 1, Rgba.Of(0xffb340, 0.7));
                        }
                        else pen.Box(r, 8, Rgba.Of(0xffffff, Over(r) ? 0.17 : 0.1));
                        pen.Wrapped(op.Label, cx + 9, cy + 5, iw - 18, of);
                        if (op.Description.Length > 0) pen.Wrapped(op.Description, cx + 9, cy + 5 + lh + 1, iw - 18, df);
                        Add(r, "askOpt", qi, op.Label);
                    }
                    cy += oh + 4;
                }
                if (q.Custom)
                {
                    if (!dry)
                    {
                        var box = st.Custom[qi];
                        var foc = focus == Focus.Custom && customBox == box;
                        var r = new Rect4(cx, cy, iw, 26);
                        pen.Box(r, 8, Rgba.Of(0x000000, 0.4));
                        if (foc) pen.Border(cx, cy, iw, 26, 8, 2, Rgba.Of(0xffb340));
                        var look = Pen.F(11.5, 500, Rgba.Of(0xffffff));
                        var area = new Rect4(cx + 9, cy + (26 - pen.LineHeight(look)) / 2, iw - 18, 26);
                        var shaped = box.Paint(pen, area, look, foc ? "" : "Or type your own answer", foc, now);
                        if (foc) boxLayout = shaped;
                        Add(r, "askCustom", qi, a.Id);
                    }
                    cy += 26 + 4;
                }
                cy += 2 + 2;
            }
            var aw = pen.Width("Answer", Pen.F(11.5, 600, Rgba.Of(0xffffff))) + 20;
            AskButton("Skip", 0, 26, cx, cy, "askDeny", 0, a.Id, dry);
            AskButton("Answer", 1, 26, cx + iw - aw, cy, "askAnswer", 0, a.Id, dry);
            cy += 26;
        }
        return cy + pad - y;
    }

    /// <summary>The card of the open chat in <paramref name="box"/>; when it is taller than the box (<paramref name="full"/>) it scrolls inside it, with a bar and a fade as in a menu.</summary>
    void PaintAskScrolled(AskInfo a, SessionInfo s, float full, Rect4 box)
    {
        if (askScrollFor != a.Id)
        {
            askScrollFor = a.Id;
            askScroll.ToStart();
        }
        askRect = box;
        askScroll.Set(full, box.H);
        var before = targets.Count;
        pen.Cv.PushClip(box);
        AskCard(a, s, false, box.W, box.X, box.Y - askScroll.Off, false);
        askScroll.PaintFade(pen.Cv, box, a.Danger ? Rgba.Of(0x1c0e14, 0.95) : Rgba.Of(0x1c1415, 0.95));
        pen.Cv.PopClip();
        ClipTargets(before, box);
        if (askScroll.Max <= 0) return;
        var strip = MenuScroll.Strip(box);
        Add(strip, "askBar");
        askScroll.PaintBar(pen.Cv, box, Over(strip));
    }

    readonly MenuScroll askScroll = new();
    string askScrollFor = "";
    Rect4 askRect;

    /// <summary>The wheel over the open chat's question moves it when it is cut.</summary>
    bool AskWheel(float x, float y, float delta)
    {
        if (askScroll.Max <= 0 || OpenId is null || Current()?.Ask is null || !askRect.Contains(x, y)) return false;
        askScroll.By(-delta);
        Dirty = true;
        return true;
    }

    /// <summary>The card's box: its fill, its line and, over a bot, the tail that points at it.</summary>
    void AskFrame(AskInfo a, bool over, float x, float y, float cw, float ch)
    {
        var bg = over ? Rgba.Of(0x0e0a12, 0.95) : a.Danger ? Rgba.Of(0xff453a, 0.06) : Rgba.Of(0xffb340, 0.06);
        var edge = a.Danger ? Rgba.Of(0xff453a, 0.55) : over ? Rgba.Of(0xffb340, 0.5) : Rgba.Of(0xffb340, 0.35);
        if (over)
        {
            // A soft shadow under the card.
            for (var i = 0; i < 5; i++) pen.Box(x - i * 2, y + 6 - i, cw + i * 4, ch + i * 2, 14 + i * 2, Rgba.Of(0x000000, 0.06));
        }
        pen.Box(x, y, cw, ch, 14, bg);
        pen.Border(x, y, cw, ch, 14, 1, edge);
        if (over)
            pen.Cv.Polygon([(x + cw / 2 - 7, y + ch - 0.5f), (x + cw / 2 + 7, y + ch - 0.5f), (x + cw / 2, y + ch + 7)], Rgba.Of(0x0e0a12, 0.95));
        Block(new Rect4(x, y, cw, ch));
    }

    // MARK: Answering

    void Answer(SessionInfo s, AskInfo a, string answer)
    {
        Trace($"answer {answer} to {a.Id}");
        env.Send(new AnswerCommand(s.Id, a.Id, answer));
        Dirty = true;
    }

    void SendQuestion(SessionInfo s, AskInfo a)
    {
        var st = AskOf(a);
        var answers = new string[a.Questions!.Count][];
        for (var i = 0; i < answers.Length; i++)
        {
            var picked = st.Sel[i].ToList();
            var own = st.Custom[i].Text.Trim();
            if (own.Length > 0) picked.Add(own);
            answers[i] = [.. picked];
        }
        // Every question needs an answer (the Go card refuses with these words).
        if (answers.Any(x => x.Length == 0))
        {
            Say(answers.Length > 1 ? "Answer each question first." : "Pick an answer first.");
            return;
        }
        env.Send(new AnswerCommand(s.Id, a.Id, null, answers));
        focus = Focus.None;
        customBox = null;
        Dirty = true;
    }
}
