using Hover.Chat;
using Hover.Host.Protocol;

namespace Hover.Tests;

/// <summary>
/// A shaper with fixed-width letters (half the font size) that wraps at spaces, so the thread's
/// layout, selection and copy can be tested on any system without DirectWrite.
/// </summary>
sealed class FakeShaper : IShaper
{
    public ShapedText Shape(IReadOnlyList<Span> spans, Look look, float width, Align align)
    {
        var (text, runs, links) = Runs.Build(spans, look);
        return new ShapedText(new FakeLayout(text, runs, width, align), text, links, runs);
    }

    public (float Min, float Max) Widths(IReadOnlyList<Span> spans, Look look)
    {
        var (text, runs, _) = Runs.Build(spans, look);
        var lay = new FakeLayout(text, runs, 0, Align.Start);
        float min = 0;
        foreach (var word in text.Split(' ', '\n'))
        {
            var idx = text.IndexOf(word, StringComparison.Ordinal);
            min = Math.Max(min, lay.Span(idx < 0 ? 0 : idx, idx < 0 ? 0 : idx + word.Length));
        }
        return (min, lay.Width);
    }
}

sealed class FakeLayout : IParaLayout
{
    readonly string text;
    readonly float[] advance;
    readonly List<LineBox> lines = [];
    readonly List<bool> hard = [];
    readonly float lineH;

    public FakeLayout(string text, IReadOnlyList<StyleRun> runs, float width, Align align)
    {
        this.text = text;
        advance = new float[text.Length];
        float size = 12;
        lineH = 12 * 1.5f;
        foreach (var r in runs)
        {
            size = r.Size;
            for (var i = r.Start; i < r.End && i < text.Length; i++) advance[i] = r.Gap > 0 ? r.Gap : r.Size * 0.5f;
            lineH = Math.Max(lineH, r.Size * 1.4f);
        }
        for (var i = 0; i < text.Length; i++) if (advance[i] == 0) advance[i] = size * 0.5f;
        // Break lines at hard breaks, and at the last space that fits.
        var pos = 0;
        float top = 0, widest = 0;
        while (true)
        {
            int start = pos, end = start, lastSpace = -1;
            float x = 0;
            var hardBreak = false;
            while (end < text.Length)
            {
                if (text[end] == '\n') { hardBreak = true; break; }
                if (width > 0 && x + advance[end] > width && end > start)
                {
                    if (lastSpace >= start) end = lastSpace + 1;
                    break;
                }
                if (text[end] == ' ') lastSpace = end;
                x += advance[end];
                end++;
            }
            lines.Add(new LineBox(start, end, top, lineH, lineH * 0.8f));
            hard.Add(hardBreak || end >= text.Length);
            widest = Math.Max(widest, Span(start, end));
            top += lineH;
            pos = hardBreak ? end + 1 : end;
            if (!hardBreak && end >= text.Length) break;
            if (hardBreak && pos >= text.Length)
            {
                lines.Add(new LineBox(pos, pos, top, lineH, lineH * 0.8f));
                hard.Add(true);
                top += lineH;
                break;
            }
        }
        Width = widest;
        Height = top;
    }

    public float Width { get; }
    public float Height { get; }
    public IReadOnlyList<LineBox> Lines => lines;

    public float Span(int a, int b)
    {
        float x = 0;
        for (var i = a; i < b && i < advance.Length; i++) if (text[i] != '\n') x += advance[i];
        return x;
    }

    int LineAt(float y)
    {
        for (var i = 0; i < lines.Count; i++) if (y < lines[i].Top + lines[i].Height) return i;
        return lines.Count - 1;
    }

    public float SnapX(float x)
    {
        float at = 0;
        for (var i = 0; i < text.Length && text[i] != '\n'; i++)
        {
            if (at + advance[i] > x) break;
            at += advance[i];
        }
        return at;
    }

    public int IndexAt(float x, float y)
    {
        var l = lines[LineAt(y)];
        float at = 0;
        for (var i = l.Start; i < l.End; i++)
        {
            if (x < at + advance[i] / 2) return i;
            at += advance[i];
        }
        return l.End;
    }

    public int? ClusterAt(float x, float y)
    {
        if (y < 0 || y >= Height) return null;
        var l = lines[LineAt(y)];
        float at = 0;
        for (var i = l.Start; i < l.End; i++)
        {
            if (x >= at && x < at + advance[i]) return i;
            at += advance[i];
        }
        return null;
    }

    public bool SoftLineEnd(int index)
    {
        for (var i = 0; i < lines.Count; i++) if (lines[i].End == index && !hard[i]) return true;
        return false;
    }

    public IReadOnlyList<RectF> SelectionRects(int start, int end)
    {
        var list = new List<RectF>();
        foreach (var l in lines)
        {
            int a = Math.Max(start, l.Start), b = Math.Min(end, l.End);
            if (a >= b) continue;
            list.Add(new RectF(Span(l.Start, a), l.Top, Span(l.Start, b), l.Top + l.Height));
        }
        return list;
    }
}

/// <summary>A render source the tests fill by hand.</summary>
sealed class FakeSource : IRenderSource
{
    public Func<string, IReadOnlyList<MdBlock>?> Markdown = src => ChatFixtures.Paragraphs(src);
    public Func<string, IReadOnlyList<HighlightSpan>?> Colour = _ => [];
    public Func<string, IReadOnlyList<DiffLine>?> Numbers = diff => [.. diff.Split('\n').Select(l => new DiffLine { Text = l })];
    public Func<string, ImageState> Pictures = _ => new ImageState(ImageKind.Broken);

    public Rendered<IReadOnlyList<MdBlock>> Blocks(string src, string folder, string files) => new(Markdown(src), true);
    public Rendered<IReadOnlyList<HighlightSpan>> Highlight(string info, string text) => new(Colour(text), true);
    public Rendered<IReadOnlyList<DiffLine>> Diff(string diff) => new(Numbers(diff), true);
    public ImageState Image(string src) => Pictures(src);
}

static class ChatFixtures
{
    public static IReadOnlyList<MdBlock> Paragraphs(string src)
        => [.. src.Split("\n\n").Where(p => p.Length > 0).Select(p => new MdBlock { Kind = "para", Inlines = [new MdInline { Kind = "text", Text = p }] })];

    public static MdBlock Code(string text, string? lang = "ts") => new() { Kind = "code", Lang = lang, Text = text };

    public static Turn Done(string prompt, string answer, params Step[] steps) => new()
    {
        Prompt = prompt, Answer = answer, Steps = [.. steps], Stage = Stage.Done, When = "7:14 AM", Took = "1 s", Credits = "0.09 credits",
    };

    public static ChatThread Thread(FakeSource? source = null, string who = "Moss") => new(new FakeShaper(), source ?? new FakeSource(), who);
}
