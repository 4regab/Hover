using System.IO;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Windows.Threading;
using Hover.Core;
using Drawing = System.Drawing;
using Imaging = System.Drawing.Imaging;

namespace Hover.Owl;

/// The desk's screen panel on Windows: the main display as the agents see it. At rest
/// it is the desktop picture (the screen with no app on it); while an agent tests
/// (computer use) it is the screen itself, four frames a second. The page asks again
/// every few seconds while the panel shows; a feed nobody asks for stops by itself,
/// so a page dropped mid-stream never leaves it running.
internal sealed class ScreenFeed : IDisposable
{
    private const int Width = 1280;
    private readonly Action<string> _post;
    private readonly DispatcherTimer _timer;
    private DateTime _lease;
    private bool _live, _stillSent;
    private string? _still;

    public ScreenFeed(Action<string> post)
    {
        _post = post;
        _timer = new DispatcherTimer(DispatcherPriority.Normal) { Interval = TimeSpan.FromMilliseconds(250) };
        _timer.Tick += (_, _) => Tick();
    }

    /// On or off, and live or the desktop picture. Each call renews the lease.
    public void Ask(bool on, bool live)
    {
        if (!on) { _timer.Stop(); return; }
        _lease = DateTime.UtcNow.AddSeconds(8);
        if (live != _live) _stillSent = false;
        _live = live;
        if (!_timer.IsEnabled) { _stillSent = false; _timer.Start(); }
        Tick();
    }

    private void Tick()
    {
        if (DateTime.UtcNow > _lease) { _timer.Stop(); return; }
        if (_live) Send(Capture(), true);
        else if (!_stillSent) { _stillSent = true; Send(_still ??= Wallpaper(), false); }
    }

    private void Send(string? image, bool live)
    {
        if (image is null) return;
        _post(JsonSerializer.Serialize(new { type = "screen", image, live, access = true }));
    }

    /// The main display now, scaled to Width.
    private static string? Capture()
    {
        try
        {
            var b = System.Windows.Forms.Screen.PrimaryScreen?.Bounds ?? new Drawing.Rectangle(0, 0, 1920, 1080);
            using var full = new Drawing.Bitmap(b.Width, b.Height, Imaging.PixelFormat.Format24bppRgb);
            using (var g = Drawing.Graphics.FromImage(full)) g.CopyFromScreen(b.Left, b.Top, 0, 0, b.Size, Drawing.CopyPixelOperation.SourceCopy);
            return Jpeg(full, 60);
        }
        catch (Exception e) when (e is System.ComponentModel.Win32Exception or ExternalException or ArgumentException)
        {
            // A locked or secure desktop can't be read; the last frame stays.
            return null;
        }
    }

    /// The desktop picture, or the desktop's colour when there is none.
    private static string? Wallpaper()
    {
        var b = System.Windows.Forms.Screen.PrimaryScreen?.Bounds ?? new Drawing.Rectangle(0, 0, 1920, 1080);
        var buf = new System.Text.StringBuilder(520);
        var path = SystemParametersInfo(SPI_GETDESKWALLPAPER, (uint)buf.Capacity, buf, 0) ? buf.ToString() : "";
        // Windows keeps its own copy of the picture it shows.
        var transcoded = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "Microsoft", "Windows", "Themes", "TranscodedWallpaper");
        foreach (var file in new[] { path, transcoded })
        {
            if (string.IsNullOrWhiteSpace(file) || !File.Exists(file)) continue;
            try
            {
                using var src = Drawing.Image.FromFile(file);
                using var frame = new Drawing.Bitmap(b.Width, b.Height);
                using (var g = Drawing.Graphics.FromImage(frame))
                {
                    // Fill, as Windows does by default: cover the screen, centred.
                    var k = Math.Max((double)b.Width / src.Width, (double)b.Height / src.Height);
                    int w = (int)(src.Width * k), h = (int)(src.Height * k);
                    g.InterpolationMode = Drawing.Drawing2D.InterpolationMode.HighQualityBicubic;
                    g.DrawImage(src, (b.Width - w) / 2, (b.Height - h) / 2, w, h);
                }
                return Jpeg(frame, 80);
            }
            catch (Exception e) when (e is OutOfMemoryException or ArgumentException or IOException or ExternalException)
            {
                Log.Line($"screen: couldn't read the desktop picture - {e.Message}");
            }
        }
        var c = GetSysColor(COLOR_DESKTOP);
        using var solid = new Drawing.Bitmap(b.Width / 8, b.Height / 8);
        using (var g = Drawing.Graphics.FromImage(solid)) g.Clear(Drawing.Color.FromArgb((int)(c & 0xFF), (int)((c >> 8) & 0xFF), (int)((c >> 16) & 0xFF)));
        return Jpeg(solid, 80);
    }

    private static string Jpeg(Drawing.Bitmap src, long quality)
    {
        var k = Math.Min(1.0, (double)Width / src.Width);
        using var scaled = k < 1 ? new Drawing.Bitmap(src, (int)(src.Width * k), (int)(src.Height * k)) : (Drawing.Bitmap)src.Clone();
        var codec = Imaging.ImageCodecInfo.GetImageEncoders().First(c => c.FormatID == Imaging.ImageFormat.Jpeg.Guid);
        using var ps = new Imaging.EncoderParameters(1);
        ps.Param[0] = new Imaging.EncoderParameter(Imaging.Encoder.Quality, quality);
        using var mem = new MemoryStream();
        scaled.Save(mem, codec, ps);
        return "data:image/jpeg;base64," + Convert.ToBase64String(mem.ToArray());
    }

    public void Dispose() => _timer.Stop();

    private const uint SPI_GETDESKWALLPAPER = 0x0073;
    private const int COLOR_DESKTOP = 1;

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool SystemParametersInfo(uint action, uint param, System.Text.StringBuilder vparam, uint winIni);

    [DllImport("user32.dll")]
    private static extern uint GetSysColor(int index);
}
