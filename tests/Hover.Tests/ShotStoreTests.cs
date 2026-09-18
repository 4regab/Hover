using System.IO;
using System.Threading;
using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using Hover.Core;
using Hover.Images;
using Hover.Interop;
using NUnit.Framework;

namespace Hover.Tests;

[Apartment(ApartmentState.STA)]
[NonParallelizable]
public sealed class ShotStoreTests
{
    [OneTimeSetUp]
    public void StartWpf()
    {
        if (Application.Current is null) _ = new Application();
    }

    [SetUp, TearDown]
    public void ClearShots()
    {
        if (Directory.Exists(Paths.Shots))
            foreach (var file in Directory.EnumerateFiles(Paths.Shots))
                File.Delete(file);
    }

    [Test]
    public void A_saved_shot_thumbnails_and_renames_and_deletes()
    {
        var path = Path.Combine(Paths.Shots, "shot1.png");
        File.WriteAllBytes(path, Png(4, 4));

        var shot = new Shot(path, File.GetLastWriteTime(path));

        Assert.Multiple(() =>
        {
            Assert.That(shot.Thumbnail(32), Is.Not.Null);
            Assert.That(shot.FullSize(), Is.Not.Null);
            Assert.That(shot.Name, Is.EqualTo("shot1.png"));
        });

        var renamed = ShotStore.Shared.Rename(shot, "renamed");
        Assert.That(renamed, Is.Not.Null);
        Assert.That(File.Exists(renamed!), Is.True);
        Assert.That(Path.GetFileName(renamed!), Is.EqualTo("renamed.png"));

        ShotStore.Shared.Delete(new Shot(renamed!, DateTime.Now));
        Assert.That(File.Exists(renamed!), Is.False);
    }

    [Test]
    public void Drag_payload_is_a_real_file_and_no_plain_text()
    {
        var path = Path.Combine(Paths.Shots, "drag.png");
        File.WriteAllBytes(path, Png(6, 6));
        var shot = new Shot(path, DateTime.Now);

        var data = ShotDrag.BuildData(shot);

        Assert.Multiple(() =>
        {
            Assert.That(data.ContainsFileDropList(), Is.True);
            Assert.That(data.GetFileDropList()[0], Is.EqualTo(path));
            Assert.That(File.Exists(data.GetFileDropList()[0]!), Is.True);
            // No plain text or URI — those make chat boxes paste a link.
            Assert.That(data.ContainsText(), Is.False);
        });
    }

    private static byte[] Png(int w, int h)
    {
        var pixels = new byte[w * h * 4];
        for (var i = 0; i < pixels.Length; i++) pixels[i] = (byte)(i * 7);
        var bmp = BitmapSource.Create(w, h, 96, 96, PixelFormats.Bgra32, null, pixels, w * 4);
        var encoder = new PngBitmapEncoder();
        encoder.Frames.Add(BitmapFrame.Create(bmp));
        using var ms = new MemoryStream();
        encoder.Save(ms);
        return ms.ToArray();
    }

    /// The box a snip drag makes. Either corner can be the one you started from, and
    /// neither may leave the desktop — a drag that runs off the edge stops at it.
    [Test]
    public void Snip_box_is_corner_order_blind_and_stays_on_the_desktop()
    {
        var from = new Win32.POINT { X = 900, Y = 600 };
        var to = new Win32.POINT { X = 300, Y = 200 };

        var dragged = SnipOverlay.Box(from, to, 0, 0, 1920, 1080);
        var other = SnipOverlay.Box(to, from, 0, 0, 1920, 1080);

        Assert.Multiple(() =>
        {
            Assert.That(dragged.Left, Is.EqualTo(300));
            Assert.That(dragged.Top, Is.EqualTo(200));
            Assert.That(dragged.Width, Is.EqualTo(600));
            Assert.That(dragged.Height, Is.EqualTo(400));
            Assert.That(other.Left, Is.EqualTo(dragged.Left), "the other way round is the same box");
            Assert.That(other.Width, Is.EqualTo(dragged.Width));
            Assert.That(other.Height, Is.EqualTo(dragged.Height));
        });

        // Off the edge, on a desktop whose origin is negative: a second display placed
        // to the left of the main one.
        var off = SnipOverlay.Box(new Win32.POINT { X = -4000, Y = -4000 },
                                  new Win32.POINT { X = 9000, Y = 9000 },
                                  -1920, 0, 1920, 1080);
        Assert.Multiple(() =>
        {
            Assert.That(off.Left, Is.EqualTo(-1920));
            Assert.That(off.Top, Is.EqualTo(0));
            Assert.That(off.Right, Is.EqualTo(1920));
            Assert.That(off.Bottom, Is.EqualTo(1080));
        });
    }
}
