using Hover.Chat.Desk;

namespace Hover.Chat.Ui;

public sealed partial class OfficeUi
{
    /// <summary>
    /// The tip beside the pointer over the office (internal/ui/officeview.go tip): "Chat with Pip" over a bot, "Pip’s desk" and
    /// what the desk holds over a desk with a session at it, and a prop's own hint ("Session board"). The office says what the
    /// pointer is on; nothing is drawn over the panels' own places or when a menu is open.
    /// </summary>
    void PaintTip()
    {
        if (ChatView || px < 0 || menu is not null || confirm is not null || Covers(px, py)) return;
        var (kind, id, said) = env.Tip();
        var hint = said ?? "";
        // Not over the bot whose chat is open (internal/shell/office.go); and none while a desk card is open, which Covers answers.
        if (kind is 1 or 2 && !(kind == 1 && OpenId == id) && DeskSession(id) is { } s)
        {
            var f = Pen.F(12, 400, Pen.Ink);
            var bf = Pen.F(12, 600, Rgba.Of(0xffffff));
            var name = Bots.Name(s.Bot) + (kind == 2 ? "’s desk" : "");
            var inner = 6f + 20 + 7;
            if (kind == 1) inner += pen.Width("Chat with", f) + 7;
            inner += pen.Width(name, bf);
            const string sub = "Browser, terminal, files, diff…";
            var sf = Pen.F(11, 400, Pen.Ink);
            if (kind == 2) inner += 7 + pen.Width(sub, sf);
            var tw = inner + 11;
            var x = Math.Min(px + 14, w - tw - 6);
            var y = py + 16;
            for (var i = 0; i < 4; i++) pen.Box(x - i * 3, y + 6 - i * 2, tw + i * 6, 30 + i * 3, 10 + i * 3, Rgba.Of(0x000000, 0.12));
            pen.Box(x, y, tw, 30, 10, Rgba.Of(0x0e0a12, 0.88));
            pen.Border(x, y, tw, 30, 10, 1, Rgba.Of(0xffffff, 0.1));
            var cx = x + 6;
            pen.Box(cx, y + 5, 20, 20, 6, Bots.Color(s.Bot) with { A = 77 });
            pen.Icon(kind == 1 ? Glyphs.Chat : DeskIcons.Terminal, cx + 3.5f, y + 8.5f, 13, Rgba.Of(0xffffff));
            cx += 20 + 7;
            if (kind == 1)
            {
                var lh = pen.LineHeight(f);
                cx += pen.Text("Chat with", cx, y + (30 - lh) / 2, f with { Color = Rgba.Of(0xf6f2ff, 0.6) }) + 7;
            }
            var nh = pen.LineHeight(bf);
            cx += pen.Text(name, cx, y + (30 - nh) / 2, bf) + 7;
            if (kind == 2) pen.Text(sub, cx, y + (30 - pen.LineHeight(sf)) / 2, sf with { Color = Rgba.Of(0xf6f2ff, 0.4) });
        }
        if (hint.Length > 0) PaintHintBox(hint, px + 14, py + 16);
    }

    /// <summary>A line of words in a small box at (x, y), kept inside the window (Go sets it in a pixel face; there is none here, so it is the semibold sans).</summary>
    void PaintHintBox(string hint, float x, float y)
    {
        var hf = Pen.F(12, 600, Rgba.Of(0xf6f2ff));
        var tw = Math.Min(pen.Width(hint, hf), Math.Max(w - 12 - 18, 0));
        var th = pen.LineHeight(hf);
        x = Math.Max(Math.Min(x, w - tw - 18 - 6), 6);
        pen.Box(x, y, tw + 18, th + 8, 8, Rgba.Of(0x0a060e, 0.85));
        pen.Border(x, y, tw + 18, th + 8, 8, 1, Rgba.Of(0xffffff, 0.1));
        pen.Text(hint, x + 9, y + 4, hf, tw);
    }
}
