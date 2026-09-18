using Avalonia.Media.Imaging;
using Avalonia.Threading;
using Hover.Core;

namespace Hover.Images;

/// Snip, then mark up, then save. One place, so every way in behaves the same.
public static class Snip
{
    /// Photographs the screen, lets the user choose a box, opens the mark-up window, and
    /// saves what comes out into the tray.
    ///
    /// Saving happens only if the user asks for it. Escape in either window leaves
    /// nothing behind, which is why the picture is never written before the editor.
    public static void Begin()
    {
        SnipOverlay.Start(picture => Dispatcher.UIThread.Post(() => MarkUp(picture)));
    }

    /// Opens the mark-up window on a picture that is already in hand.
    public static void MarkUp(Bitmap picture)
    {
        var editor = new AnnotateWindow(picture);
        editor.Saved += (_, png) =>
        {
            if (ShotStore.Shared.SavePng(png, "snip") is null)
                Log.Line("the snip matched a picture already in the tray");
        };
        editor.Show();
        editor.Activate();
    }
}
