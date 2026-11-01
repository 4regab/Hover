namespace Hover.Windowing;

/// <summary>
/// <c>--shots</c> for the office: the office is a swap chain, which neither the notch renderer's
/// bitmap nor the window's element tree holds, so its picture is drawn offscreen (Direct3D 11, read
/// back) and put in the place the office goes.
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
        int w = (int)Math.Round(area.W * scale), h = (int)Math.Round(area.H * scale);
        var office = OfficeHost.Current?.Shoot(w, h, scale, cutToNotch: true);
        if (office is null) return;
        Put(bgra, width, height, office, w, h, (int)Math.Round(area.X * scale) - cropLeft, (int)Math.Round(area.Y * scale));
    }

    /// <summary>Puts the office in the dashboard picture, in <paramref name="area"/> (DIPs of the window).</summary>
    internal static void PasteDashboard(byte[] bgra, int width, int height, double scale, OfficeRect area)
    {
        int w = (int)Math.Round(area.W * scale), h = (int)Math.Round(area.H * scale);
        if (w < 1 || h < 1) return;
        var office = OfficeHost.Current?.Shoot(w, h, scale, cutToNotch: false);
        if (office is null) return;
        Put(bgra, width, height, office, w, h, (int)Math.Round(area.X * scale), (int)Math.Round(area.Y * scale));
    }

    /// <summary>Lays a premultiplied picture over an opaque one at (x, y); what falls outside is cut off.</summary>
    static void Put(byte[] bgra, int width, int height, byte[] office, int w, int h, int x, int y)
    {
        for (var row = 0; row < h; row++)
        {
            var dy = y + row;
            if (dy < 0 || dy >= height) continue;
            for (var col = 0; col < w; col++)
            {
                var dx = x + col;
                if (dx < 0 || dx >= width) continue;
                int s = (row * w + col) * 4, d = (dy * width + dx) * 4;
                var keep = 255 - office[s + 3];
                for (var k = 0; k < 3; k++) bgra[d + k] = (byte)Math.Min(office[s + k] + bgra[d + k] * keep / 255, 255);
            }
        }
    }
}
