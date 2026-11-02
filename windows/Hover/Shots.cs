using Hover.Host;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;

namespace Hover;

/// <summary>
/// <c>--shots DIR</c>: the app draws its own views to JPEG files, as the Go app does to PNG.
/// The pictures come from the window's element tree, so they need no screen capture, which
/// some machines block. Later phases add their views here.
/// </summary>
public static class Shots
{
    /// <summary>No picture is wider than this; a wider view is drawn smaller.</summary>
    public const int MaxWidth = 1400;

    internal static async Task TakeAsync(string directory, DashboardWindow dashboard, Hover.Chatting.UiCore? panels = null)
    {
        Directory.CreateDirectory(directory);
        await Save(dashboard.Root, Path.Combine(directory, "dashboard.jpg"), (pixels, w, h, scale) =>
        {
            // The 3D office is a swap chain, which the element tree's picture does not hold: it is put in its area.
            var origin = dashboard.OfficeArea.TransformToVisual(dashboard.Root).TransformPoint(new Windows.Foundation.Point(0, 0));
            Hover.Windowing.OfficeShots.PasteDashboard(pixels, w, h, scale,
                new Hover.Windowing.OfficeRect(origin.X, origin.Y, dashboard.OfficeArea.ActualWidth, dashboard.OfficeArea.ActualHeight));
            // The panels (the switch, the menu button, the + circle, the bots' tags) are a layer of their own that the element tree's picture
            // leaves out: they are drawn over the office, as the window shows them.
            if (panels is null) return;
            var layer = Hover.Chatting.ChatShots.RenderLayer(panels, (float)dashboard.OfficeArea.ActualWidth, (float)dashboard.OfficeArea.ActualHeight, scale);
            Hover.Chatting.ChatShots.Over(pixels, w, h, (int)Math.Round(origin.X * scale), (int)Math.Round(origin.Y * scale), layer,
                (int)Math.Round(dashboard.OfficeArea.ActualWidth * scale), (int)Math.Round(dashboard.OfficeArea.ActualHeight * scale));
        });
        if (panels is not null) await Hover.Chatting.ChatShots.TakeAsync(directory, panels);
        await Hover.Windowing.NotchShots.TakeAsync(directory, panels);
        await Hover.Speaking.VoiceShots.TakeAsync(directory, panels);
        if (panels is not null) await Hover.Configuring.SettingsShots.TakeAsync(directory, panels);
    }

    static async Task Save(FrameworkElement view, string file, Action<byte[], int, int, double>? after = null)
    {
        // Wait until the view has been laid out and drawn once.
        if (!view.IsLoaded)
        {
            var loaded = new TaskCompletionSource();
            view.Loaded += (_, _) => loaded.TrySetResult();
            await loaded.Task;
        }
        await Task.Delay(400);

        // The size asked of RenderAsync is in the view's own units, and the picture comes out that many times the display scale larger
        // (at 150 % a 1185 wide window made a 1778 wide picture): the limit is on the pixels, so the display scale is counted.
        var display = view.XamlRoot?.RasterizationScale ?? 1.0;
        var shrink = Math.Min(1.0, MaxWidth / Math.Max(1.0, view.ActualWidth * display));
        var width = Math.Max(1, (int)Math.Round(view.ActualWidth * shrink));
        var height = Math.Max(1, (int)Math.Round(view.ActualHeight * shrink));

        var bitmap = new RenderTargetBitmap();
        await bitmap.RenderAsync(view, width, height);
        // Pixels for each of the view's units, as the picture really is (the callback places layers with it).
        var scale = bitmap.PixelWidth / Math.Max(1.0, view.ActualWidth);
        var pixels = await bitmap.GetPixelsAsync();
        var bytes = new byte[pixels.Length];
        using (var reader = DataReader.FromBuffer(pixels)) reader.ReadBytes(bytes);

        after?.Invoke(bytes, bitmap.PixelWidth, bitmap.PixelHeight, scale);
        await WriteJpegAsync(bytes, bitmap.PixelWidth, bitmap.PixelHeight, file);
        HostLog.Line($"picture {file} ({bitmap.PixelWidth} x {bitmap.PixelHeight})");
    }

    /// <summary>Writes opaque BGRA pixels as a JPEG file.</summary>
    public static async Task WriteJpegAsync(byte[] bgra, int width, int height, string file)
    {
        using var stream = new InMemoryRandomAccessStream();
        var encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.JpegEncoderId, stream);
        encoder.SetPixelData(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Ignore, (uint)width, (uint)height, 96, 96, bgra);
        await encoder.FlushAsync();

        stream.Seek(0);
        var jpeg = new byte[stream.Size];
        using (var reader = new DataReader(stream)) { await reader.LoadAsync((uint)stream.Size); reader.ReadBytes(jpeg); }
        await File.WriteAllBytesAsync(file, jpeg);
    }
}
