namespace Hover.Chat;

public sealed partial class ChatThread
{
    (string Source, float Width)? documentKey;

    /// <summary>
    /// Lays one Markdown document out as the whole thread, with no prompt and no name: a Markdown file's preview, a
    /// pull request's description. It is laid out again when the text or the width changes, or when the backend's
    /// answer for it (or a picture) has arrived. True while some of it is plain text waiting for that answer.
    /// </summary>
    public bool SetDocument(string markdown, float width)
    {
        if (documentKey == (markdown, width) && Sections.Count == 1 && !Sections[0].Stale) return Sections[0].Waiting;
        documentKey = (markdown, width);
        Width = width;
        var m = new MdLayout(sh, source, Copied);
        var rendered = source.Blocks(markdown, Folder, Files);
        var look = Palette.BodyLook() with { LH = 1.55f, Color = new Rgba(0xe9, 0xe7, 0xec) };
        var b = m.Blocks(rendered.Value ?? Plain(markdown), look, width, true);
        var s = new Section { Frag = b.Frag, H = b.H, Y = 0, Waiting = m.Waiting || !rendered.Final };
        s.Images.AddRange(m.Used);
        Sections = [s];
        Height = b.H;
        ClearSelection();
        return s.Waiting;
    }

    /// <summary>The document's top left is the thread's: a click or a pointer at (x, y) of it.</summary>
    public bool IsDocument => documentKey is not null && Sections.Count == 1;
}
