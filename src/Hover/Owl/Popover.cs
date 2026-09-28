using System.Windows.Controls;

namespace Hover.Owl;

/// Keeps count of the menus open, so the notch does not fold away under one.
internal static class Popover
{
    /// Count of menus open right now; the notch stays open while > 0.
    public static int Open { get; private set; }

    public static void Track(ContextMenu menu)
    {
        menu.Opened += (_, _) => Open++;
        menu.Closed += (_, _) => Open--;
    }
}
