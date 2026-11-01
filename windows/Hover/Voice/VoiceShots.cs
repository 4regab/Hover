using Hover.Host;
using Hover.Voice;
using Hover.Windowing;

namespace Hover.Speaking;

/// <summary>
/// <c>--shots</c> for voice: the card in the resting notch at every stage and office size, the
/// menus, the pictures it sends, the aura in violet and the amber glow, named as the Go app's
/// pictures are (voice-default-preview-2x and so on). They are drawn by the card's own painter
/// on the app's graphics, offscreen and read back.
/// </summary>
static class VoiceShots
{
    internal static async Task TakeAsync(string directory, Hover.Chatting.UiCore? panels = null)
    {
        Directory.CreateDirectory(directory);
        // The panels' own graphics when there are panels; else a set of its own.
        var own = panels is null ? Gfx.Create() : null;
        var gfx = panels?.Gfx ?? own!;
        try
        {
            using var painter = new VoiceCardPainter(gfx.Write, gfx.Fonts, gfx.D2D, Path.Combine(AppContext.BaseDirectory, "Assets", "Office"));
            var data = Path.Combine(Path.GetTempPath(), "hover-voice-shots");
            var count = 0;
            foreach (var spec in CardFixtures.All(data))
            {
                var ui = new CardUi { Now = 0.35 };
                spec.Setup?.Invoke(ui);
                var (bgra, w, h) = CardShot.Render(gfx.D2D, gfx.Context, painter, spec.View, ui, spec.Width, spec.Height, spec.Scale, spec.OpenH);
                var file = Path.Combine(directory, spec.Name + ".jpg");
                await Hover.Shots.WriteJpegAsync(bgra, w, h, file);
                count++;
            }
            HostLog.Line($"voice pictures: {count} in {directory}");
        }
        catch (Exception e)
        {
            HostLog.Line($"the voice pictures failed — {e}");
            throw;
        }
        finally
        {
            own?.Dispose();
        }
    }
}
