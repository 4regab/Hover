using Avalonia.Headless.NUnit;
using Avalonia.Media.Imaging;
using Hover.Interop;
using NUnit.Framework;

namespace Hover.Tests;

/// Windows hands a pasted picture over without its file header. Putting the header
/// back is pure arithmetic, and getting the pixel offset wrong would show every
/// pasted picture as garbage, so it is checked here against real decoding.
public sealed class ClipboardImageTests
{
    /// A DIB: a 40-byte info header, then an optional palette, then the pixels.
    private static byte[] Dib(int width, int height, short bitCount,
        int compression = 0, int coloursUsed = 0, int extraBytes = 0)
    {
        var dib = new byte[40 + extraBytes + width * height * 4];
        BitConverter.GetBytes(40).CopyTo(dib, 0);
        BitConverter.GetBytes(width).CopyTo(dib, 4);
        BitConverter.GetBytes(height).CopyTo(dib, 8);
        BitConverter.GetBytes((short)1).CopyTo(dib, 12);
        BitConverter.GetBytes(bitCount).CopyTo(dib, 14);
        BitConverter.GetBytes(compression).CopyTo(dib, 16);
        BitConverter.GetBytes(coloursUsed).CopyTo(dib, 32);
        return dib;
    }

    [Test]
    public void Header_says_BM_and_reports_the_whole_file_length()
    {
        var dib = Dib(4, 4, 32);
        var bmp = ClipboardImage.WrapAsBmp(dib)!;

        Assert.Multiple(() =>
        {
            Assert.That(bmp[0], Is.EqualTo((byte)'B'));
            Assert.That(bmp[1], Is.EqualTo((byte)'M'));
            Assert.That(bmp.Length, Is.EqualTo(dib.Length + 14));
            Assert.That(BitConverter.ToInt32(bmp, 2), Is.EqualTo(bmp.Length));
        });
    }

    [Test]
    public void Pixels_start_straight_after_the_header_when_there_is_no_palette()
    {
        var bmp = ClipboardImage.WrapAsBmp(Dib(4, 4, 32))!;
        Assert.That(BitConverter.ToInt32(bmp, 10), Is.EqualTo(54)); // 14 + 40
    }

    [Test]
    public void Colour_masks_push_the_pixels_back_twelve_bytes()
    {
        // Compression 3 is BI_BITFIELDS: three 4-byte masks follow the header.
        var bmp = ClipboardImage.WrapAsBmp(Dib(4, 4, 32, compression: 3, extraBytes: 12))!;
        Assert.That(BitConverter.ToInt32(bmp, 10), Is.EqualTo(66)); // 14 + 40 + 12
    }

    [Test]
    public void A_full_palette_is_counted_when_the_file_does_not_declare_one()
    {
        // 8 bits per pixel with coloursUsed left at 0 means all 256 entries are there.
        var bmp = ClipboardImage.WrapAsBmp(Dib(4, 4, 8, extraBytes: 256 * 4))!;
        Assert.That(BitConverter.ToInt32(bmp, 10), Is.EqualTo(14 + 40 + 256 * 4));
    }

    [Test]
    public void A_declared_partial_palette_is_counted_at_its_real_size()
    {
        var bmp = ClipboardImage.WrapAsBmp(Dib(4, 4, 8, coloursUsed: 16, extraBytes: 16 * 4))!;
        Assert.That(BitConverter.ToInt32(bmp, 10), Is.EqualTo(14 + 40 + 16 * 4));
    }

    [Test]
    public void Nonsense_is_refused_rather_than_producing_a_broken_file()
    {
        var tooSmall = new byte[8];
        var sillyHeader = new byte[64];
        BitConverter.GetBytes(9999).CopyTo(sillyHeader, 0);

        Assert.Multiple(() =>
        {
            Assert.That(ClipboardImage.WrapAsBmp(tooSmall), Is.Null);
            Assert.That(ClipboardImage.WrapAsBmp(sillyHeader), Is.Null);
        });
    }

    /// The real proof: a decoder has to accept the result and report the size the DIB
    /// claimed. Arithmetic that merely looks right still fails here.
    [AvaloniaTest]
    public void The_result_is_a_file_a_decoder_actually_reads()
    {
        var dib = Dib(6, 3, 32);
        // Fill the pixels so the image is not rejected as empty.
        for (var i = 40; i < dib.Length; i++) dib[i] = 0x80;

        var bmp = ClipboardImage.WrapAsBmp(dib);
        Assert.That(bmp, Is.Not.Null, "the header could not be built");

        using var stream = new MemoryStream(bmp!);
        Bitmap decoded = null!;
        Assert.DoesNotThrow(() => decoded = new Bitmap(stream),
            "the decoder refused the file, so the header is wrong");

        using (decoded)
        {
            Assert.Multiple(() =>
            {
                Assert.That(decoded.PixelSize.Width, Is.EqualTo(6));
                Assert.That(decoded.PixelSize.Height, Is.EqualTo(3));
            });
        }
    }
}
