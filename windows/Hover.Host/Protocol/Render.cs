using System.Text.Json;
using System.Text.Json.Serialization;

namespace Hover.Host.Protocol;

// What the backend answers to the chat's rendering commands (internal/backend/render.go):
// Markdown read into blocks, code coloured, a change numbered, a flowchart laid out. Each
// answer echoes the request's "ref". A request the backend refuses comes back with ok false
// and an error, so a host that waits is never left waiting.

/// <summary>A stretch of a paragraph: text with its marks, a line break, or a picture.</summary>
public sealed record MdInline
{
    /// <summary>text, break or image.</summary>
    public string Kind { get; init; } = "text";
    public string Text { get; init; } = "";
    public string Alt { get; init; } = "";
    public bool Strong { get; init; }
    public bool Em { get; init; }
    public bool Del { get; init; }
    public bool Code { get; init; }
    public string? Link { get; init; }
}

public sealed record MdCell
{
    /// <summary>left, center or right.</summary>
    public string Align { get; init; } = "left";
    public List<MdInline> Content { get; init; } = [];
}

public sealed record MdItem
{
    /// <summary>A task list item: true when ticked, null for a plain item.</summary>
    public bool? Task { get; init; }
    public List<MdInline> Content { get; init; } = [];
    public List<MdBlock> Lists { get; init; } = [];
}

/// <summary>One block of an answer. Which fields are set depends on <see cref="Kind"/>.</summary>
public sealed record MdBlock
{
    /// <summary>para, heading, rule, code, diagram, quote, list or table.</summary>
    public string Kind { get; init; } = "para";
    public List<MdInline>? Inlines { get; init; }
    public int Level { get; init; }
    public string? Lang { get; init; }
    /// <summary>Code, or a diagram's SVG.</summary>
    public string Text { get; init; } = "";
    public List<MdBlock>? Blocks { get; init; }
    public bool Ordered { get; init; }
    public long Start { get; init; }
    public List<MdItem>? Items { get; init; }
    public List<MdCell>? Head { get; init; }
    public List<List<MdCell>>? Rows { get; init; }
}

public sealed record MarkdownEvent : BackendEvent
{
    public string Ref { get; init; } = "";
    public bool Ok { get; init; }
    public string? Error { get; init; }
    public string? Format { get; init; }
    public List<MdBlock> Blocks { get; init; } = [];
}

/// <summary>A coloured stretch of code: UTF-16 offsets into the text, and a colour.</summary>
public sealed record HighlightSpan
{
    public int U0 { get; init; }
    public int U1 { get; init; }
    public int R { get; init; }
    public int G { get; init; }
    public int B { get; init; }
    public int A { get; init; }
    public bool Em { get; init; }
}

public sealed record HighlightEvent : BackendEvent
{
    public string Ref { get; init; } = "";
    public bool Ok { get; init; }
    public string? Error { get; init; }
    public List<HighlightSpan> Spans { get; init; } = [];
}

/// <summary>A line of a change with the file's line number, when the diff names it.</summary>
public sealed record DiffLine
{
    public long N { get; init; }
    public bool HasN { get; init; }
    /// <summary>A break between two hunks, drawn as an ellipsis.</summary>
    public bool Gap { get; init; }
    public string Text { get; init; } = "";
}

public sealed record DiffLinesEvent : BackendEvent
{
    public string Ref { get; init; } = "";
    public bool Ok { get; init; }
    public string? Error { get; init; }
    public List<DiffLine> Lines { get; init; } = [];
}

public sealed record FlowNode
{
    public string Id { get; init; } = "";
    public string Label { get; init; } = "";
    /// <summary>box, round, stadium, circle, sub, db, hex, diamond or flag.</summary>
    public string Shape { get; init; } = "box";
    public double X { get; init; }
    public double Y { get; init; }
    public double W { get; init; }
    public double H { get; init; }
}

public sealed record FlowEdge
{
    public int From { get; init; }
    public int To { get; init; }
    public string Label { get; init; } = "";
    public string Kind { get; init; } = "";
    public bool Head { get; init; }
}

public sealed record FlowGraph
{
    public string Dir { get; init; } = "";
    public double W { get; init; }
    public double H { get; init; }
    public List<FlowNode> Nodes { get; init; } = [];
    public List<FlowEdge> Edges { get; init; } = [];
}

public sealed record FlowchartEvent : BackendEvent
{
    public string Ref { get; init; } = "";
    public bool Ok { get; init; }
    public string? Error { get; init; }
    /// <summary>False for a chart Hover cannot read: the answer then shows its source as code.</summary>
    public bool Readable { get; init; }
    public FlowGraph? Graph { get; init; }
    public string? Svg { get; init; }
}

// MARK: Commands

public sealed record MarkdownCommand(string Src, string Ref) : BackendCommand("markdown")
{
    public string? Folder { get; init; }
    public string? Files { get; init; }
    public string? Format { get; init; }
}

public sealed record HighlightCommand(string Info, string Text, string Ref) : BackendCommand("highlight");

public sealed record DiffLinesCommand(string Diff, string Ref) : BackendCommand("diffLines");

public sealed record FlowchartCommand(string Src, string Ref) : BackendCommand("flowchart");

/// <summary>
/// The pieces of the wire a rendering answer needs to be matched with its request: the
/// <c>ref</c> of an answer, whatever its type.
/// </summary>
public static class RenderRefs
{
    public static string? Of(BackendEvent e) => e switch
    {
        MarkdownEvent m => m.Ref,
        HighlightEvent h => h.Ref,
        DiffLinesEvent d => d.Ref,
        FlowchartEvent f => f.Ref,
        _ => null,
    };
}
