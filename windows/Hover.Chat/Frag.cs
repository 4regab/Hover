using System.Text;
using Hover.Host.Protocol;

namespace Hover.Chat;

// The thread laid out: every turn as positioned text boxes and shapes, with the box model of
// the page the Go chat follows. Each message is a section laid out on its own and cached, so
// a streaming answer or a new turn lays out one section, never the thread. Selection runs
// over every text box in document order, which is what lets it cross paragraphs, lists,
// tables and code.

/// <summary>A laid-out paragraph at a place.</summary>
public sealed class TextBox
{
    public required IParaLayout Layout;
    public float X, Y;
    /// <summary>The text as laid out. Empty for text that is drawn but not selectable (list markers, the code block's tag, an ellipsis).</summary>
    public string Text = "";
    public IReadOnlyList<Link> Links = [];
    public IReadOnlyList<StyleRun> Runs = [];
    /// <summary>`overflow: auto` of the box it sits in, in the same coordinates as X, Y.</summary>
    public Rect4? Clip;
    /// <summary>The live step's moving highlight.</summary>
    public bool Shimmer;
    /// <summary>A table cell: a double or triple click stays inside it.</summary>
    public bool Cell;
    /// <summary>The box that scrolls it sideways: an index into <see cref="Frag.Scrollers"/> plus one (0: none).</summary>
    public int Scr;

    public bool Selectable => Text.Length > 0;

    public TextBox Copy() => (TextBox)MemberwiseClone();
}

/// <summary>A box with `overflow: auto` that is wider inside than out (a code block, a table).</summary>
public sealed class Scroller
{
    public Rect4 Clip;
    public float Content;
    /// <summary>Shapes that scroll with the content (a table's header fill and row rules).</summary>
    public List<Shape> Shapes = [];

    public float Max => Math.Max(Content - Clip.W, 0);
}

public enum ShapeKind { Rect, Image, Line, Svg, Broken, Glow }

/// <summary>Something drawn. A rect has a radius per corner, a fill and a border of width SW in Stroke.</summary>
public sealed class Shape
{
    public ShapeKind Kind;
    public float X, Y, W, H;
    public float[] Radius = [0, 0, 0, 0];
    public Rgba Fill, Stroke;
    public float SW;
    /// <summary>Image: the source; Svg: the document.</summary>
    public string Src = "";
    /// <summary>Image: object-fit: cover (a prompt's thumbnails).</summary>
    public bool Cover;
    public List<(float X, float Y)> Pts = [];

    public void Shift(float dx, float dy)
    {
        X += dx;
        Y += dy;
        for (var i = 0; i < Pts.Count; i++) Pts[i] = (Pts[i].X + dx, Pts[i].Y + dy);
    }

    public float Top => Kind switch
    {
        ShapeKind.Glow => Y - W,
        ShapeKind.Line => Pts.Count == 0 ? 0 : Pts.Min(p => p.Y),
        _ => Y,
    };

    public Shape Clone()
    {
        var s = (Shape)MemberwiseClone();
        s.Pts = [.. Pts];
        return s;
    }

    public static Shape RectOf(float x, float y, float w, float h, float r, Rgba fill)
        => new() { Kind = ShapeKind.Rect, X = x, Y = y, W = w, H = h, Radius = [r, r, r, r], Fill = fill };

    public static Shape BoxOf(float x, float y, float w, float h, float[] r, Rgba fill, Rgba stroke, float sw)
        => new() { Kind = ShapeKind.Rect, X = x, Y = y, W = w, H = h, Radius = r, Fill = fill, Stroke = stroke, SW = sw };

    public static Shape SvgOf(float x, float y, float w, float h, string svg)
        => new() { Kind = ShapeKind.Svg, X = x, Y = y, W = w, H = h, Src = svg };

    public static float[] Round(float r) => [r, r, r, r];
}

/// <summary>What a click on a drawn control does.</summary>
public enum ActKind
{
    None,
    /// <summary>A Copy button: Text is what it copies.</summary>
    Copy,
    /// <summary>A step with a change or output opens or folds: I is the step, Now the "now" row.</summary>
    Step,
    /// <summary>One of a step's own switches: I is the step, K which. 0 shows all of a long block; 1 the subagents past the fourth; 2 + k subagent k's result.</summary>
    Flag,
    /// <summary>A file under the answer: its change opens in the timeline (I is its step).</summary>
    OpenDiff,
    /// <summary>The newest turn's Retry: its prompt goes again.</summary>
    Retry,
    /// <summary>Back to just after this turn's answer.</summary>
    Restore,
    /// <summary>Back to just before this turn's message, which goes again.</summary>
    TryAgain,
    /// <summary>A queued reply taken back into the composer to be changed.</summary>
    Edit,
    /// <summary>A sent prompt's Edit: its words go into the reply box.</summary>
    EditPrompt,
    /// <summary>A queued reply sent now, ahead of the others.</summary>
    SendNow,
    /// <summary>A group's header: its steps open or fold.</summary>
    Group,
}

public readonly record struct Act(ActKind Kind, string Text = "", int I = 0, bool Now = false, uint K = 0);

/// <summary>A clickable place and what it does.</summary>
public readonly record struct HitRect(Rect4 R, Act Act);

public enum TokKind { Text, Req, Lit, Img, Hr, Virt, TableEnd }

/// <summary>
/// What a copy is made of, in document order: the selection serialiser of the page adds
/// newlines at block edges, and this reproduces what it gives for the blocks md.js writes.
/// </summary>
public readonly record struct Tok(TokKind Kind, int N = 0, string S = "")
{
    public static Tok OfText(int i) => new(TokKind.Text, i);
    public static Tok Req(int n) => new(TokKind.Req, n);
    public static Tok Lit(string s) => new(TokKind.Lit, 0, s);
}

/// <summary>What a selection that runs on past its last box takes in after it.</summary>
public enum Tail { None, Newline, Block }

/// <summary>The serialiser's state: newlines owed at block edges, owed only once something has been written.</summary>
public sealed class Copier
{
    readonly StringBuilder output = new();
    int pending;
    bool emitted, textSeen, tableLast;
    string lit = "";

    public string Current => output.ToString();

    void Flush()
    {
        if (emitted)
        {
            output.Append('\n', pending);
            output.Append(lit);
        }
        lit = "";
    }

    public void Text(string piece)
    {
        // Gaps are room, not characters.
        piece = piece.Replace(Runs.GapChar.ToString(), "");
        if (piece.Length == 0) return;
        tableLast = false;
        Flush();
        output.Append(piece);
        pending = 0;
        emitted = true;
        textSeen = true;
    }

    public void Add(Tok t)
    {
        switch (t.Kind)
        {
            case TokKind.Req: pending = Math.Max(pending, t.N); break;
            case TokKind.Lit: lit += t.S; break;
            case TokKind.Hr:
                if (emitted)
                {
                    Flush();
                    pending = 0;
                }
                break;
            case TokKind.Img:
                Flush();
                pending = textSeen ? 2 : 1;
                emitted = true;
                break;
            case TokKind.Virt: Text(t.S); break;
            case TokKind.TableEnd:
                pending = Math.Max(pending, 1);
                tableLast = true;
                break;
        }
        if (t.Kind != TokKind.TableEnd && t.Kind != TokKind.Req) tableLast = false;
    }

    /// <summary>The whole of something was copied: a table that ends it leaves its newline.</summary>
    public string Finish()
    {
        if (tableLast) output.Append('\n');
        return output.ToString();
    }

    /// <summary>A selection that runs on past its last box.</summary>
    public string End(Tail tail)
    {
        switch (tail)
        {
            case Tail.Newline: output.Append('\n'); break;
            case Tail.Block: output.Append('\n', pending); break;
        }
        return output.ToString();
    }
}

/// <summary>Laid-out content in coordinates relative to its own top left.</summary>
public sealed class Frag
{
    public List<TextBox> Texts = [];
    public List<Shape> Shapes = [];
    public List<Tok> Copy = [];
    public List<Scroller> Scrollers = [];
    public List<HitRect> Hits = [];

    public void Shift(float dx, float dy)
    {
        foreach (var t in Texts)
        {
            t.X += dx;
            t.Y += dy;
            if (t.Clip is { } c) t.Clip = c.Moved(dx, dy);
        }
        foreach (var s in Shapes) s.Shift(dx, dy);
        for (var i = 0; i < Hits.Count; i++) Hits[i] = Hits[i] with { R = Hits[i].R.Moved(dx, dy) };
        foreach (var sc in Scrollers)
        {
            sc.Clip = sc.Clip.Moved(dx, dy);
            foreach (var s in sc.Shapes) s.Shift(dx, dy);
        }
    }

    /// <summary>A copy that can be shifted without moving this one. Layouts are shared: nothing writes them after shaping.</summary>
    public Frag Clone() => new()
    {
        Texts = [.. Texts.Select(t => t.Copy())],
        Shapes = [.. Shapes.Select(s => s.Clone())],
        Copy = [.. Copy],
        Scrollers = [.. Scrollers.Select(s => new Scroller { Clip = s.Clip, Content = s.Content, Shapes = [.. s.Shapes.Select(x => x.Clone())] })],
        Hits = [.. Hits],
    };

    /// <summary>Moves other by dx, dy and adds it to this.</summary>
    public void Append(Frag o, float dx, float dy)
    {
        o.Shift(dx, dy);
        int baseText = Texts.Count, baseScr = Scrollers.Count;
        foreach (var t in o.Copy) Copy.Add(t.Kind == TokKind.Text ? t with { N = t.N + baseText } : t);
        foreach (var t in o.Texts) if (t.Scr > 0) t.Scr += baseScr;
        Texts.AddRange(o.Texts);
        Scrollers.AddRange(o.Scrollers);
        Shapes.AddRange(o.Shapes);
        Hits.AddRange(o.Hits);
    }

    /// <summary>Adds a selectable text box, and its place in the copy.</summary>
    public void Add(TextBox t)
    {
        Copy.Add(Tok.OfText(Texts.Count));
        Texts.Add(t);
    }

    public static Frag One(TextBox t, int after)
    {
        var f = new Frag();
        f.Add(t);
        f.Copy.Add(Tok.Req(after));
        return f;
    }
}

/// <summary>A block's box: its collapsible outer margins and its border-box height.</summary>
public sealed class Boxed
{
    public required Frag Frag;
    public float Mt, H, Mb;

    public Boxed CloneBox() => new() { Frag = Frag.Clone(), Mt = Mt, H = H, Mb = Mb };
}

/// <summary>Vertical flow with CSS margin collapsing between siblings.</summary>
public sealed class Flow
{
    readonly Frag frag = new();
    float y, prevMb, firstMt;
    bool hasPrev;

    public void Add(Boxed b, float dx = 0)
    {
        if (!hasPrev) firstMt = b.Mt;
        else y += Math.Max(prevMb, b.Mt);
        frag.Append(b.Frag, dx, y);
        y += b.H;
        prevMb = b.Mb;
        hasPrev = true;
    }

    /// <summary>The content, with the first child's top margin and the last's bottom margin passed out to the parent.</summary>
    public Boxed Through() => new() { Frag = frag, Mt = firstMt, H = y, Mb = prevMb };

    /// <summary>The content inside a box with padding: the margins stay inside.</summary>
    public (Frag Frag, float Height) Inside()
    {
        frag.Shift(0, firstMt);
        return (frag, firstMt + y + prevMb);
    }
}
