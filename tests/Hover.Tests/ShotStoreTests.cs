using System.IO;
using System.Threading;
using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using Hover.Core;
using Hover.Images;
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
}
