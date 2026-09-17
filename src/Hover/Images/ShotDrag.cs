using System.Collections.Specialized;
using System.IO;
using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using Hover.Core;
using Hover.Deck;

namespace Hover.Images;

/// Starts a drag of a picture out of the tray. Chromium targets (Chrome/Gmail,
/// Discord, ChatGPT in a browser) accept a dropped image *file*, so the file drop
/// leads. The picture bits ride along as a DIB for editors that paste on drop.
///
/// The drag is run with the tray window made briefly activatable: an OLE drag from
/// a no-activate tool window is refused by Chromium, which is what showed the
/// no-drop cursor.
public static class ShotDrag
{
    public static void Start(DependencyObject source, Shot shot)
    {
        try
        {
            var data = BuildData(shot);
            var window = Window.GetWindow(source) as DeckWindow;
            if (window is not null)
                window.WhileActivatable(() => DragDrop.DoDragDrop(source, data, DragDropEffects.Copy));
            else
                DragDrop.DoDragDrop(source, data, DragDropEffects.Copy);
        }
        catch (Exception e)
        {
            Log.Line($"drag-out failed for {shot.Name} — {e.Message}");
        }
    }

    /// The drag payload: a file drop (what Gmail, Discord and browser upload zones
    /// read) plus the picture bits as a DIB. No plain-text or URI — those make a
    /// chat box paste a link instead of attaching the picture.
    public static DataObject BuildData(Shot shot)
    {
        var data = new DataObject();
        data.SetFileDropList(new StringCollection { shot.Path });

        var full = shot.FullSize();
        if (full is not null)
        {
            var dib = ToDib(full);
            if (dib is not null) data.SetData(DataFormats.Dib, dib, autoConvert: true);
        }
        return data;
    }

    /// A device-independent bitmap stream, which is what CF_DIB drop targets read.
    /// A DIB has no file header, so the 14-byte BMP header is stripped off.
    private static MemoryStream? ToDib(BitmapSource image)
    {
        try
        {
            var bgra = new FormatConvertedBitmap(image, PixelFormats.Bgra32, null, 0);
            var encoder = new BmpBitmapEncoder();
            encoder.Frames.Add(BitmapFrame.Create(bgra));
            using var bmp = new MemoryStream();
            encoder.Save(bmp);
            var bytes = bmp.ToArray();
            const int fileHeader = 14;
            if (bytes.Length <= fileHeader) return null;
            return new MemoryStream(bytes, fileHeader, bytes.Length - fileHeader);
        }
        catch (Exception e)
        {
            Log.Line($"DIB build failed — {e.Message}");
            return null;
        }
    }
}
