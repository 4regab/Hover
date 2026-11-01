using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

public sealed partial class OfficeUi
{
    const float TagH = 20;

    /// <summary>The bot's tag, by session: where it stands, or null when the office does not say.</summary>
    BotTag? TagOf(int id) => env.Tags().FirstOrDefault(t => t.Id == id);

    /// <summary>Whether a question is shown over this bot (not while its desk card is open: the card shows the question itself).</summary>
    bool AskOver(SessionInfo s) => s.Ask is not null && deskCard != s.Id;

    /// <summary>
    /// The bots' name tags (internal/ui/officeview.go tag): a chip with the tool's mark and the bot's name standing on the
    /// office's tag point, and over it a bubble with what the bot says. A click on the chip opens the bot's chat. The bubble's
    /// tail and the pixel face of Go's names are not drawn.
    /// </summary>
    void PaintTags()
    {
        var tags = env.Tags();
        if (tags.Count == 0) return;
        var nf = Pen.F(12, 600, Rgba.Of(0xffffff));
        foreach (var t in tags)
        {
            var asking = state?.Sessions.FirstOrDefault(x => x.Id == t.Id) is { } s && AskOver(s);
            var chipW = 3 + 15 + 5 + pen.Width(t.Name, nf) + 8;
            var cx = t.X - chipW / 2;
            var cy = t.Y - TagH;
            if (t.Text.Length > 0 && !asking)
            {
                var bf = Pen.F(t.Stage == 4 ? 14 : 12, 500, Rgba.Of(0xffffff));
                var tw = pen.Width(t.Text, bf);
                var bw = Math.Min(240, Math.Min(218, tw) + 22);
                var bh = pen.LineHeight(bf) + 13;
                var bx = t.X - bw / 2;
                var by = cy - 4 - bh - (t.Hot ? 2 : 0);
                Rgba ink = Rgba.Of(0xffffff);
                if (t.Stage != 4)
                {
                    var (bg, edge) = t.Stage switch
                    {
                        2 => (Rgba.Of(0x0a1e12, 0.92), Rgba.Of(0x4ade80, 0.35)),
                        3 => (Rgba.Of(0x280a0a, 0.92), Rgba.Of(0xff5b52, 0.35)),
                        _ => (Rgba.Of(0x0e0a12, 0.9), Rgba.Of(0xffffff, 0.1)),
                    };
                    for (var i = 0; i < 3; i++) pen.Box(bx - i * 3, by + 8 - i * 2, bw + i * 6, bh + i * 3, 12 + i * 3, Rgba.Of(0x000000, 0.1));
                    pen.Box(bx, by, bw, bh, 12, bg);
                    pen.Border(bx, by, bw, bh, 12, 1, edge);
                }
                ink = t.Stage switch { 4 => Rgba.Of(0xc9c2e6), 2 => Rgba.Of(0xb8f5c9), 3 => Rgba.Of(0xffb3ad), _ => ink };
                pen.Text(t.Text, bx + 11, by + 6, bf with { Color = ink }, Math.Min(218, bw - 22));
            }
            var chip = new Rect4(cx, cy, chipW, TagH);
            if (t.Hot)
            {
                pen.Box(chip, 7, t.Color);
                pen.Border(chip.X, chip.Y, chip.W, chip.H, 7, 2, Rgba.Of(0xffffff, 0.7));
            }
            else
            {
                pen.Box(chip, 7, Rgba.Of(0x0a060e, 0.72));
                pen.Border(chip.X, chip.Y, chip.W, chip.H, 7, 1, Rgba.Of(0xffffff, 0.08));
            }
            pen.Mark(t.Tool.Length > 0 ? t.Tool : "kiro", cx + 3, cy + 2 + (TagH - 5 - 15) / 2, 15);
            pen.Text(t.Name, cx + 3 + 15 + 5, cy + 2 + (TagH - 5 - pen.LineHeight(nf)) / 2, nf);
            Add(chip, "botTag", t.Id);
        }
    }
}
