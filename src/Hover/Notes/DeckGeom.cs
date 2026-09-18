using Hover.Core;

namespace Hover.Notes;

/// Resolved metrics for one fan.
///
/// Tabs *shingle*: each is full height but sits `Pitch` below the one before, so it
/// laps over it like a roof tile. That keeps every tab tall enough to carry a label
/// while the deck as a whole stays well short of the screen.
public sealed class DeckLayout
{
    public double ItemHeight;   // full height of one tab
    public double Pitch;        // top-to-top spacing; < ItemHeight means overlap
    public int Count;
    public double PanelHeight;

    public double StackHeight => Count <= 0
        ? 0
        : (Count - 1) * Pitch + ItemHeight + DeckGeom.PlusGap + DeckGeom.PlusSize;

    public double Top => Math.Max(12, (PanelHeight - StackHeight) / 2);

    /// Centre of the strip of item `index` that is actually visible.
    public double Center(int index)
    {
        var strip = index == Count - 1 ? ItemHeight : Pitch;
        return Top + index * Pitch + strip / 2;
    }
}

public static class DeckGeom
{
    /// Every metric below is quoted at 100% and scaled by one preference, so the
    /// deck grows or shrinks without drifting out of proportion with itself.
    public static double Scale => Settings.DeckScale;

    public static double TabWidth => 30 * Scale;
    /// How far the next tab laps over the one before it.
    public static double TabLap => 40 * Scale;
    public static double PitchMin => 56 * Scale;
    public static double PitchMax => 106 * Scale;

    /// The strip is the label plus this much; the label is drawn inside it with
    /// LabelInset. Keeping the two different is what leaves the last glyph room —
    /// sizing the strip to exactly the text width truncates on rounding.
    public static double LabelPad => 20 * Scale;
    public static double LabelInset => 12 * Scale;

    /// Tabs are drawn a little past the screen edge so their lean cannot open a wedge
    /// of background between them and the edge they are stuck to.
    public static double Bleed => 14 * Scale;

    /// Everything leans the same way — a deck of tabs at matching angles reads as
    /// deliberate, where per-note angles just look scattered.
    public const double LeanDegrees = 3.0;
    public static double Lean(bool onRight) => onRight ? -LeanDegrees : LeanDegrees;

    public static double ChipHeight => 24 * Scale;
    public static double ChipGap => 6 * Scale;
    public static double FanWidth => 50 * Scale;
    public static double PlusSize => 28 * Scale;
    public static double PlusGap => 12 * Scale;

    /// The deck may claim at most this much of the screen before tabs start shrinking.
    public const double HeightBudget = 0.68;

    /// How wide the panel has to be to hold the fan alone, edge bleed included.
    public static double RestingWidth => TabWidth + Bleed + 10;

    /// The hover card that shows what is written on a note.
    public const double CardWidth = 300;

    /// The open note, and how wide the panel grows to hold it.
    public const double EditorWidth = 440;
    public static double OpenWidth => EditorWidth + FanWidth * 0.4;

    /// `count` tabs down a panel this tall, with the strip of each sized to the longest
    /// label so titles read in full until they hit the cap and ellipsise.
    public static DeckLayout Layout(double panelHeight, int count, DeckStyle style,
                                    double longestLabel = 0)
    {
        var n = Math.Max(1, count);
        if (style == DeckStyle.Compact)
        {
            return new DeckLayout
            {
                ItemHeight = ChipHeight,
                Pitch = ChipHeight + ChipGap,
                Count = n,
                PanelHeight = panelHeight,
            };
        }

        var pitch = Math.Min(PitchMax, Math.Max(PitchMin, longestLabel + LabelPad));

        // Guard rail: on a short display, shrink rather than run off-screen.
        var budget = panelHeight * HeightBudget;
        if (n * pitch + TabLap > budget)
            pitch = Math.Max(36 * Scale, (budget - TabLap) / n);

        return new DeckLayout
        {
            ItemHeight = pitch + TabLap,
            Pitch = pitch,
            Count = n,
            PanelHeight = panelHeight,
        };
    }
}
