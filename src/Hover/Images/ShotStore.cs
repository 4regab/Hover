using System.IO;
using System.Security.Cryptography;
using System.Windows;
using System.Windows.Interop;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using Hover.Core;
using Hover.Interop;

namespace Hover.Images;

/// The pictures behind the image tray.
///
/// Two ways in: a new file appearing in the Shots folder (a saved Snipping Tool
/// capture), and an image landing on the clipboard (a snip that only went to the
/// clipboard, or any copied picture). Both are deduplicated by content hash, so the
/// same picture arriving twice is kept once. Files are plain PNGs on disk, which is
/// what lets one be dragged straight out to Explorer, a browser or a terminal.
public sealed class ShotStore : IDisposable
{
    public static ShotStore Shared { get; } = new();

    private static readonly string[] Extensions = { ".png", ".jpg", ".jpeg", ".bmp", ".gif" };

    private readonly List<Shot> _shots = new();
    private readonly HashSet<string> _hashes = new(StringComparer.Ordinal);
    private FileSystemWatcher? _watcher;
    private HwndSource? _clipboardSource;
    private DispatcherTimer? _rescan;

    /// Raised whenever the set of pictures changed.
    public event EventHandler? Changed;

    /// Newest first — the freshest snip sits at the top of the tray.
    public IReadOnlyList<Shot> Shots => _shots;

    private ShotStore() { }

    /// Called once at startup, on the UI thread, so the clipboard listener has a
    /// dispatcher to marshal back to.
    public void Start()
    {
        ScanFolder();
        StartFolderWatch();
        StartClipboardWatch();
    }

    // MARK: The folder

    private void ScanFolder()
    {
        _shots.Clear();
        _hashes.Clear();
        try
        {
            foreach (var file in Directory.EnumerateFiles(Paths.Shots))
            {
                if (!Extensions.Contains(Path.GetExtension(file).ToLowerInvariant())) continue;
                Add(file, hash: SafeHashFile(file), notify: false);
            }
        }
        catch (Exception e)
        {
            Log.Line($"shot scan failed — {e.Message}");
        }
        Sort();
    }

    private void Add(string path, string? hash, bool notify)
    {
        if (hash is not null && !_hashes.Add(hash)) return;   // a duplicate; skip it
        DateTime taken;
        try { taken = File.GetLastWriteTime(path); }
        catch { taken = DateTime.Now; }
        _shots.Add(new Shot(path, taken));
        if (notify) { Sort(); Notify(); }
    }

    private void Sort() => _shots.Sort((a, b) => b.Taken.CompareTo(a.Taken));

    private void StartFolderWatch()
    {
        try
        {
            _watcher = new FileSystemWatcher(Paths.Shots)
            {
                NotifyFilter = NotifyFilters.FileName | NotifyFilters.LastWrite,
                EnableRaisingEvents = true,
            };
            _watcher.Created += (_, _) => DebouncedRescan();
            _watcher.Deleted += (_, _) => DebouncedRescan();
            _watcher.Renamed += (_, _) => DebouncedRescan();
        }
        catch (Exception e)
        {
            Log.Line($"shot folder watch failed — {e.Message}");
        }
    }

    /// The watcher fires several times for one saved file (create, then writes), and
    /// a screenshot tool may still be writing when the first event arrives. Coalesce
    /// to one rescan a short moment after the last event.
    private void DebouncedRescan()
    {
        void Run()
        {
            _rescan?.Stop();
            _rescan = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(400) };
            _rescan.Tick += (_, _) =>
            {
                _rescan?.Stop();
                _rescan = null;
                ScanFolder();
                Notify();
            };
            _rescan.Start();
        }

        var dispatcher = Application.Current?.Dispatcher;
        if (dispatcher is null || dispatcher.CheckAccess()) Run();
        else dispatcher.BeginInvoke(Run);
    }

    // MARK: The clipboard

    private void StartClipboardWatch()
    {
        try
        {
            // A message-only window: it exists solely to receive clipboard-changed
            // messages. It is never shown.
            var parameters = new HwndSourceParameters("HoverClipboardListener")
            {
                Width = 0,
                Height = 0,
                ParentWindow = new IntPtr(-3), // HWND_MESSAGE
            };
            _clipboardSource = new HwndSource(parameters);
            _clipboardSource.AddHook(ClipboardHook);
            Win32.AddClipboardFormatListener(_clipboardSource.Handle);
        }
        catch (Exception e)
        {
            Log.Line($"clipboard watch failed — {e.Message}");
        }
    }

    private IntPtr ClipboardHook(IntPtr hwnd, int msg, IntPtr wParam, IntPtr lParam, ref bool handled)
    {
        if (msg == Win32.WM_CLIPBOARDUPDATE) CaptureClipboardImage();
        return IntPtr.Zero;
    }

    private void CaptureClipboardImage()
    {
        try
        {
            if (!Clipboard.ContainsImage()) return;
            var image = Clipboard.GetImage();
            if (image is null) return;

            var png = EncodePng(image);
            var hash = Hash(png);
            if (_hashes.Contains(hash)) return;   // already have this exact picture

            var path = Path.Combine(Paths.Shots, $"clip-{Stamp()}.png");
            File.WriteAllBytes(path, png);
            Add(path, hash, notify: true);
            Log.Line($"captured clipboard image {Path.GetFileName(path)}");
        }
        catch (Exception e)
        {
            Log.Line($"clipboard capture failed — {e.Message}");
        }
    }

    // MARK: Mutations the tray asks for

    /// Rename a picture, keeping its extension. Returns the new path or null on
    /// failure or a name clash.
    public string? Rename(Shot shot, string newBaseName)
    {
        try
        {
            var clean = string.Concat(newBaseName.Split(Path.GetInvalidFileNameChars())).Trim();
            if (clean.Length == 0) return null;
            var ext = Path.GetExtension(shot.Path);
            var target = Path.Combine(Paths.Shots, clean + ext);
            if (File.Exists(target)) return null;
            File.Move(shot.Path, target);
            ScanFolder();
            Notify();
            return target;
        }
        catch (Exception e)
        {
            Log.Line($"rename failed for {shot.Name} — {e.Message}");
            return null;
        }
    }

    public void Delete(Shot shot)
    {
        try
        {
            if (File.Exists(shot.Path)) File.Delete(shot.Path);
        }
        catch (Exception e)
        {
            Log.Line($"delete failed for {shot.Name} — {e.Message}");
        }
        ScanFolder();
        Notify();
    }

    // MARK: Bits

    private void Notify() => Application.Current?.Dispatcher.BeginInvoke(
        () => Changed?.Invoke(this, EventArgs.Empty));

    private static byte[] EncodePng(BitmapSource image)
    {
        var encoder = new PngBitmapEncoder();
        encoder.Frames.Add(BitmapFrame.Create(image));
        using var ms = new MemoryStream();
        encoder.Save(ms);
        return ms.ToArray();
    }

    private static string Hash(byte[] bytes) => Convert.ToHexString(SHA256.HashData(bytes));

    private static string? SafeHashFile(string path)
    {
        try { return Hash(File.ReadAllBytes(path)); }
        catch { return null; }
    }

    private static string Stamp() => DateTime.Now.ToString("yyyyMMdd-HHmmss-fff");

    public void Dispose()
    {
        _rescan?.Stop();
        _watcher?.Dispose();
        if (_clipboardSource is not null)
        {
            Win32.RemoveClipboardFormatListener(_clipboardSource.Handle);
            _clipboardSource.RemoveHook(ClipboardHook);
            _clipboardSource.Dispose();
        }
    }
}
