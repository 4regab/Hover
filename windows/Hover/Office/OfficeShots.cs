namespace Hover.Windowing;

/// <summary>
/// <c>--shots</c> for the office: the office is a swap chain, which neither the notch renderer's
/// bitmap nor the window's element tree holds, so its picture is drawn offscreen and put in the
/// place the office goes.
/// </summary>
static class OfficeShots
{
    /// <summary>
    /// Puts the office in the notch picture. <paramref name="bgra"/> is the opaque picture
    /// (<paramref name="width"/> wide, cut <paramref name="cropLeft"/> device pixels from the left of the window),
    /// <paramref name="area"/> the office's place in the window in DIPs.
    /// </summary>
    internal static void PasteNotch(byte[] bgra, int width, int height, int cropLeft, double scale, (double X, double Y, double W, double H) area, string name)
    {
    }

    /// <summary>Puts the office in the dashboard picture, in <paramref name="area"/> (DIPs of the window).</summary>
    internal static void PasteDashboard(byte[] bgra, int width, int height, double scale, OfficeRect area)
    {
    }
}
