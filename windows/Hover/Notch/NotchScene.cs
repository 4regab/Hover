using Hover.Notch;

namespace Hover.Windowing;

/// <summary>A line of the change a question shows: 1 added, -1 removed, 0 context.</summary>
sealed record PreviewLine(string Text, int Kind);

/// <summary>The question in the notch's card: what it wants, where, and the buttons' words.</summary>
sealed record CardData(string Tool, string Title, string Sub, string Count, string Command, string Path,
    IReadOnlyList<PreviewLine> Preview, string Reason, bool Danger, string Allow);

/// <summary>One mark of the stack of agents at work, with how far it has come to the front (0 to 1).</summary>
sealed record StackMark(string Tool, double Front, int Index);

/// <summary>
/// Everything the notch draws in one frame, in DIPs of the notch window. The host fills it in
/// from the backend's messages and the controller's shape; the renderer only reads it.
/// </summary>
sealed class NotchScene
{
    // The shape: its left edge, size, bottom corner radius and ears; its fill (0xRRGGBB, black
    // while small as a real notch is, the panel's colour once open).
    public double ShapeX, ShapeW, ShapeH, ShapeR, ShapeEar;
    public uint ShapeFill;
    public double MiniOpacity, ViewOpacity;
    public bool ViewVisible, ShadowOn;
    public double Openness;
    /// <summary>The open size, and the resting size the island or card is laid out at.</summary>
    public double OpenW, OpenH, RestW, RestH;
    public RestKind Kind;
    public Island? Island;
    public CardData? Card;
    /// <summary>The glow round an island that asks, or one that ends: 0 for none.</summary>
    public uint Glow;
    /// <summary>The notch's clock in seconds, for the spinning and breathing rings.</summary>
    public double T;
    /// <summary>How far the changed words and the changed island have come in (0 to 1).</summary>
    public double Rise = 1, Fade = 1;
    public bool Dark = true;
    public uint Panel = 0x000000;
    /// <summary>The action under the pointer, so its button lights up.</summary>
    public string? Hover;

    /// <summary>Takes the shape from the controller's frame: its place, its corners, how opaque each part is and its fill.</summary>
    public void ApplyShape(NotchShape shape, Size open)
    {
        var f = shape.Frame;
        ShapeX = shape.X0;
        ShapeW = f.W;
        ShapeH = f.H;
        ShapeR = f.R;
        ShapeEar = f.Ear;
        // Black while small, as a real notch is; the panel's own colour by the time the cards are in.
        uint Channel(uint v) => (uint)(v * f.FillMix + 0.5);
        ShapeFill = (Channel((Panel >> 16) & 0xff) << 16) | (Channel((Panel >> 8) & 0xff) << 8) | Channel(Panel & 0xff);
        MiniOpacity = f.MiniOpacity;
        ViewOpacity = f.ViewOpacity;
        ViewVisible = shape.ViewVisible;
        ShadowOn = shape.ShadowOn;
        Openness = shape.Openness;
        OpenW = open.W;
        OpenH = open.H;
        RestW = shape.RestTarget.W;
        RestH = shape.RestTarget.H;
    }

    /// <summary>
    /// Where the office is drawn in the open notch, in DIPs of the window: the whole open size, from
    /// the shape's left edge when open. Phase 3 puts its Direct3D 11 view here.
    /// </summary>
    public (double X, double Y, double W, double H) OfficeArea(double windowWidth) => ((windowWidth - OpenW) / 2, 0, OpenW, OpenH);
}

/// <summary>A place on the notch that does something when clicked.</summary>
sealed record HitTarget(string Action, double X, double Y, double W, double H)
{
    public bool Contains(double x, double y) => x >= X && x < X + W && y >= Y && y < Y + H;
}
