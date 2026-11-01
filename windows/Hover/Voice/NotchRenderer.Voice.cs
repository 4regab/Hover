using Hover.Voice;
using Vortice.Direct2D1;

namespace Hover.Windowing;

/// <summary>The voice card, drawn by <see cref="VoiceCardPainter"/> with the notch's own graphics and text.</summary>
sealed partial class NotchRenderer
{
    VoiceCardPainter? voicePainter;

    // Inter comes from the graphics; the menu headings' pixel face is the office's font, beside the app.
    VoiceCardPainter VoicePainter => voicePainter ??= new VoiceCardPainter(g.Write, g.Fonts, g.D2D, Path.Combine(AppContext.BaseDirectory, "Assets", "Office"));

    /// <summary>What can be clicked on the voice card in the frame just drawn.</summary>
    public IReadOnlyList<CardTarget> VoiceTargets => VoicePainter.Targets;

    /// <summary>The card as it measures at the resting notch: what the resting shape is sized to.</summary>
    public (double W, double H) VoiceSize(NotchScene s, ID2D1DeviceContext context) => VoicePainter.Measure(context, s.Voice!, s.VoiceUi!, s.OpenH);

    /// <summary>Draws the card with its left edge at <paramref name="x"/> (the top of the window is the top of the card).</summary>
    void DrawVoice(NotchScene s, double x) => VoicePainter.Draw(dc, s.Voice!, s.VoiceUi!, x, 0, s.OpenH, alpha);

    /// <summary>The character a point of a box on the card lands on.</summary>
    public int VoiceIndexAt(VoiceEditor editor, double x, double y, double width, bool wrap, float fontSize) => VoicePainter.TaskIndexAt(editor, x, y, width, wrap, fontSize);

    void DisposeVoice() => voicePainter?.Dispose();
}
