using System.Globalization;

namespace Hover.Chat;

/// <summary>What a click selects: a caret, a word or a paragraph.</summary>
public enum Unit { Char, Word, Para }

public enum HitKind { None, Link, Act, Text }

/// <summary>What is under a point.</summary>
public readonly record struct Hit(HitKind Kind, string Link = "", int Section = 0, Act Act = default, Pos Pos = default);

public sealed partial class ChatThread
{
    /// <summary>Finds what is under a point in thread coordinates (y from the thread's top).</summary>
    public Hit Hit(float x, float y)
    {
        x -= Palette.ThreadPad[3];
        for (var i = 0; i < Sections.Count; i++)
        {
            var s = Sections[i];
            foreach (var h in s.Frag.Hits)
            {
                var r = h.R;
                if (x >= r.X && x < r.X + r.W && y >= s.Y + r.Y && y < s.Y + r.Y + r.H) return new Hit(HitKind.Act, Section: i, Act: h.Act);
            }
        }
        var best = new Hit(HitKind.None);
        float bestD = 0;
        for (var si = 0; si < Sections.Count; si++)
        {
            var s = Sections[si];
            for (var ti = 0; ti < s.Frag.Texts.Count; ti++)
            {
                var t = s.Frag.Texts[ti];
                if (!t.Selectable) continue;
                var off = OffsetOf(si, t);
                // What a scrolling box hides cannot be clicked, nor what a clip cuts off.
                if (t.Clip is { } c && t.Scr > 0)
                {
                    if (x < c.X || x >= c.X + c.W || y < s.Y + c.Y || y >= s.Y + c.Y + c.H) continue;
                }
                if (t.Clip is { } c2 && t.Scr == 0)
                {
                    float top = s.Y + t.Y, bot = s.Y + t.Y + t.Layout.Height;
                    if (y >= top && y < bot && (y < s.Y + c2.Y || y >= s.Y + c2.Y + c2.H)) continue;
                }
                float lx = x - t.X + off, ly = y - s.Y - t.Y;
                float h = t.Layout.Height, w = t.Layout.Width;
                float dy = ly < 0 ? -ly : ly > h ? ly - h : 0;
                float dx = lx < 0 ? -lx : lx > w ? lx - w : 0;
                var d = dy * 4 + dx;
                var p = new Pos(si, ti, 0);
                if (dy == 0 && dx == 0)
                {
                    // A link is what the pointer is over; the caret is the nearest boundary.
                    if (t.Layout.ClusterAt(lx, ly) is { } under)
                    {
                        foreach (var l in t.Links)
                            if (under >= l.Start && under < l.End) return new Hit(HitKind.Link, Link: l.Url);
                    }
                    return new Hit(HitKind.Text, Pos: p with { Index = t.Layout.IndexAt(lx, ly) });
                }
                if (best.Kind == HitKind.None || d < bestD)
                {
                    best = new Hit(HitKind.Text, Pos: p with { Index = t.Layout.IndexAt(Math.Min(Math.Max(lx, 0), w), Math.Min(Math.Max(ly, 0), h)) });
                    bestD = d;
                }
            }
        }
        return best;
    }

    /// <summary>Makes a selection from the anchor to the focus (none when they are the same).</summary>
    public void Select(Pos anchor, Pos focus)
    {
        HasSel = anchor != focus;
        SelAnchor = anchor;
        SelFocus = focus;
        SelTail = Tail.None;
    }

    public void ClearSelection() => HasSel = false;

    TextBox TextAt(Pos p) => Sections[p.Section].Frag.Texts[p.Text];

    /// <summary>
    /// The word a double click at caret p selects. The caret is the boundary nearest the pointer,
    /// and the word is the one that starts there when it is on a boundary, so the right half of a
    /// word's last letter selects what follows it; but at the end of a soft-wrapped line it is the
    /// word before. At a block's end that is the break to the next block; a table cell selects nothing there.
    /// </summary>
    public (Pos A, Pos Z, Tail Tail) WordAt(Pos p)
    {
        var t = TextAt(p);
        if (p.Index >= t.Text.Length) return t.Cell ? (p, p, Tail.None) : (p, p, Tail.Newline);
        var b = Words(t.Text);
        var at = p.Index;
        if (p.Index > 0 && t.Layout.SoftLineEnd(p.Index)) at = p.Index - 1;
        int s = 0, e = t.Text.Length;
        foreach (var x in b) if (x <= at) s = x;
        foreach (var x in b)
            if (x > at)
            {
                e = x;
                break;
            }
        return (p with { Index = s }, p with { Index = e }, Tail.None);
    }

    /// <summary>The paragraph a triple click at caret p selects: the line between hard breaks, and the break that ends it. The last line of a block takes the block's end; a table cell is selected alone.</summary>
    public (Pos A, Pos Z, Tail Tail) ParagraphAt(Pos p)
    {
        var tb = TextAt(p);
        var t = tb.Text;
        var at = Math.Min(p.Index, t.Length);
        var s = t.LastIndexOf('\n', Math.Max(at - 1, 0)) + 1;
        if (at == 0) s = 0;
        var nl = t.IndexOf('\n', at);
        if (nl >= 0) return (p with { Index = s }, p with { Index = nl + 1 }, Tail.None);
        return (p with { Index = s }, p with { Index = t.Length }, tb.Cell ? Tail.None : Tail.Block);
    }

    public (Pos A, Pos Z, Tail Tail) UnitAt(Pos p, Unit u) => u switch
    {
        Unit.Word => WordAt(p),
        Unit.Para => ParagraphAt(p),
        _ => (p, p, Tail.None),
    };

    /// <summary>Windows editing also selects the spaces after a double-clicked word, up to the next non-space or line break, inside the block.</summary>
    public Pos TrailingSpace(Pos p)
    {
        var t = TextAt(p).Text;
        var n = 0;
        for (var i = p.Index; i < t.Length; i++)
        {
            if (t[i] == '\n' || !(char.IsWhiteSpace(t[i]) || t[i] == '\u00a0')) break;
            n++;
        }
        return p with { Index = p.Index + n };
    }

    /// <summary>Selects a unit, or (after a double or triple click, while dragging) the anchor's unit grown by whole units to the one at focus.</summary>
    public void SelectUnits(Pos a0, Pos a1, Tail at, Pos focus, Unit u)
    {
        var (f0, f1, ft) = UnitAt(focus, u);
        Pos s, e;
        Tail tail;
        if (f0 < a0) (s, e, tail) = (a1, f0, at);
        else if (a1 < f1 || (f1 == a1 && ft != Tail.None)) (s, e, tail) = (a0, f1, ft);
        else (s, e, tail) = (a0, a1, at);
        HasSel = !(s == e && tail == Tail.None);
        SelAnchor = s;
        SelFocus = e;
        SelTail = tail;
    }

    (Pos Lo, Pos Hi) Bounds() => SelFocus < SelAnchor ? (SelFocus, SelAnchor) : (SelAnchor, SelFocus);

    /// <summary>The selected text as the page would copy it.</summary>
    public string SelectedText()
    {
        if (!HasSel) return "";
        var (lo, hi) = Bounds();
        var c = new Copier();
        bool started = false, ended = false;
        for (var si = 0; si < Sections.Count; si++)
        {
            var s = Sections[si];
            foreach (var tok in s.Frag.Copy)
            {
                // Past the selection's last box only its own block end counts.
                if (ended)
                {
                    if (tok.Kind is TokKind.Req or TokKind.TableEnd)
                    {
                        c.Add(tok);
                        continue;
                    }
                    return c.End(SelTail);
                }
                if (tok.Kind == TokKind.Text)
                {
                    var k = (si, tok.N);
                    if (k.CompareTo((lo.Section, lo.Text)) < 0) continue;
                    if (((hi.Section, hi.Text)).CompareTo(k) < 0) return c.Current;
                    started = true;
                    var t = s.Frag.Texts[tok.N];
                    int st = 0, en = t.Text.Length;
                    if (k == (lo.Section, lo.Text)) st = lo.Index;
                    if (k == (hi.Section, hi.Text)) en = hi.Index;
                    st = Math.Min(st, en);
                    if (en <= t.Text.Length && st >= 0) c.Text(t.Text[st..en]);
                    if (k == (hi.Section, hi.Text) && en < t.Text.Length) return c.Current;
                    if (k == (hi.Section, hi.Text)) ended = true;
                }
                else if (started) c.Add(tok);
            }
        }
        if (SelTail != Tail.None) return c.End(SelTail);
        return c.Finish();
    }

    /// <summary>A select-all inside turn i's answer, as the page copies it.</summary>
    public string AnswerText(int i)
    {
        var s = Sections[i];
        var from = s.AnswerTok >= 0 ? s.AnswerTok : s.Frag.Copy.Count;
        var c = new Copier();
        foreach (var tok in s.Frag.Copy.Skip(from))
        {
            if (tok.Kind == TokKind.Text) c.Text(s.Frag.Texts[tok.N].Text);
            else c.Add(tok);
        }
        return c.Finish();
    }

    /// <summary>Selects everything, as a select-all over the thread does.</summary>
    public void SelectAll()
    {
        Pos? first = null, last = null;
        for (var si = 0; si < Sections.Count && first is null; si++)
            foreach (var tok in Sections[si].Frag.Copy)
                if (tok.Kind == TokKind.Text)
                {
                    first = new Pos(si, tok.N, 0);
                    break;
                }
        for (var si = Sections.Count - 1; si >= 0 && last is null; si--)
        {
            var s = Sections[si];
            for (var i = s.Frag.Copy.Count - 1; i >= 0; i--)
                if (s.Frag.Copy[i] is { Kind: TokKind.Text } tok)
                {
                    last = new Pos(si, tok.N, s.Frag.Texts[tok.N].Text.Length);
                    break;
                }
        }
        if (first is { } a && last is { } z)
        {
            SelAnchor = a;
            SelFocus = z;
            HasSel = true;
            SelTail = Tail.None;
        }
    }

    /// <summary>The selected stretch of one text box, if any.</summary>
    public (int Start, int End)? SelectedIn(int section, int text)
    {
        if (!HasSel) return null;
        var (lo, hi) = Bounds();
        var key = (section, text);
        if (key.CompareTo((lo.Section, lo.Text)) < 0 || key.CompareTo((hi.Section, hi.Text)) > 0) return null;
        var n = Sections[section].Frag.Texts[text].Text.Length;
        int s = 0, e = n;
        if (key == (lo.Section, lo.Text)) s = lo.Index;
        if (key == (hi.Section, hi.Text)) e = hi.Index;
        return s < e ? (s, Math.Min(e, n)) : null;
    }

    /// <summary>The selection's rectangles for one text box, in its own coordinates.</summary>
    public IReadOnlyList<RectF> SelectionRects(int section, int text)
        => SelectedIn(section, text) is { } r ? Sections[section].Frag.Texts[text].Layout.SelectionRects(r.Start, r.End) : [];

    /// <summary>
    /// Word boundaries of a text as a browser's word selection draws them: runs of letters and
    /// digits are words, runs of spaces stay together, and every other character is a word of
    /// its own. A full stop is a boundary unless it sits between two digits ("foo.bar" is three
    /// words, "3.14" one).
    /// </summary>
    internal static List<int> Words(string text)
    {
        var set = new SortedSet<int> { 0, text.Length };
        static bool Alnum(string t, int i) => i >= 0 && i < t.Length && (char.IsLetterOrDigit(t[i]) || t[i] == '_' || char.IsSurrogate(t[i]));
        // A full stop or comma between digits and an apostrophe between letters stay inside the word.
        static bool InWord(string t, int i)
        {
            if (Alnum(t, i)) return true;
            if (t[i] is '.' or ',') return i > 0 && i + 1 < t.Length && char.IsAsciiDigit(t[i - 1]) && char.IsAsciiDigit(t[i + 1]);
            if (t[i] is '\'' or '\u2019') return i > 0 && i + 1 < t.Length && char.IsLetter(t[i - 1]) && char.IsLetter(t[i + 1]);
            return false;
        }
        for (var i = 1; i < text.Length; i++)
        {
            char p = text[i - 1], r = text[i];
            if (char.IsLowSurrogate(r) || CharUnicodeInfo.GetUnicodeCategory(r) is UnicodeCategory.NonSpacingMark or UnicodeCategory.SpacingCombiningMark) continue;
            var spaces = r != '\n' && r != '\r' && char.IsWhiteSpace(r) && p != '\n' && p != '\r' && char.IsWhiteSpace(p);
            if (spaces) continue;
            if (p == '\r' && r == '\n') continue;
            if (InWord(text, i - 1) && InWord(text, i)) continue;
            set.Add(i);
        }
        return [.. set];
    }
}
