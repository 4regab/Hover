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

    /// One file as the last scan saw it, keyed by path. Keeping the Shot lets its
    /// decoded thumbnail survive a rescan. Keeping the hash means an unchanged file is
    /// never read again.
    private sealed record Known(Shot Shot, long WriteTicks, long Length, string Hash);

    private readonly Dictionary<string, Known> _known = new(StringComparer.OrdinalIgnoreCase);

    private FileSystemWatcher? _watcher;
    private HwndSource? _clipboardSource;
    private DispatcherTimer? _rescan;
    private DispatcherTimer? _tidy;

    /// Raised whenever the set of pictures changed.
    public event EventHandler? Changed;

    /// Newest first — the freshest snip sits at the top of the tray.
    public IReadOnlyList<Shot> Shots => _shots;

    private ShotStore() { }

    /// Called once at startup, on the UI thread, so the clipboard listener has a
    /// dispatcher to marshal back to.
    public void Start()
    {
        MoveLegacyPictures();
        TidyOldPictures();
        ScanFolder();
        StartFolderWatch();
        StartClipboardWatch();
        StartTidyTimer();
    }

    /// Applies a changed retention without waiting for the timer.
    public void TidyNow()
    {
        if (!TidyOldPictures()) return;
        ScanFolder();
        Notify();
    }

    /// Hover runs from login for days at a time, so retention cannot run only at
    /// startup.
    private void StartTidyTimer()
    {
        _tidy = new DispatcherTimer(DispatcherPriority.Background)
        {
            Interval = TimeSpan.FromHours(6),
        };
        _tidy.Tick += (_, _) => TidyNow();
        _tidy.Start();
    }

    /// Moves pictures older than the retention into the Recycle Bin. Returns true if
    /// any were moved.
    ///
    /// The Recycle Bin rather than File.Delete, because a timer runs this with the user
    /// absent and the result has to be recoverable. Does nothing unless a retention is
    /// set in Settings.
    private static bool TidyOldPictures()
    {
        var days = Settings.ShotRetentionDays;
        if (days <= 0) return false;

        var cutoff = DateTime.Now.AddDays(-days);
        var removed = 0;
        try
        {
            foreach (var file in Directory.EnumerateFiles(Paths.Shots))
            {
                if (!Extensions.Contains(Path.GetExtension(file).ToLowerInvariant())) continue;
                if (File.GetLastWriteTime(file) > cutoff) continue;
                if (Win32.RecycleFile(file)) removed++;
            }
        }
        catch (Exception e)
        {
            Log.Line($"tidying old pictures failed — {e.Message}");
        }

        if (removed > 0)
            Log.Line($"tidied {removed} picture(s) older than {days} days into the Recycle Bin");
        return removed > 0;
    }

    /// Watches the folder currently set and shows its contents. Called after the folder
    /// setting changes.
    public void Rebind()
    {
        _rescan?.Stop();
        _rescan = null;
        _watcher?.Dispose();
        _watcher = null;
        ScanFolder();
        StartFolderWatch();
        Notify();
    }

    /// Moves pictures left in the folder earlier builds used, so the tray is not empty
    /// after an upgrade.
    ///
    /// Runs only while the folder setting is still the default. A folder chosen
    /// explicitly is left alone. No migration flag is needed, because once the old
    /// folder holds no pictures this does nothing.
    private static void MoveLegacyPictures()
    {
        if (!string.IsNullOrWhiteSpace(Settings.ShotsFolder)) return;

        var legacy = Paths.LegacyPictureShots;
        var target = Paths.Shots;
        if (string.Equals(legacy, target, StringComparison.OrdinalIgnoreCase)) return;
        if (!Directory.Exists(legacy)) return;

        var moved = 0;
        try
        {
            foreach (var file in Directory.EnumerateFiles(legacy))
            {
                if (!Extensions.Contains(Path.GetExtension(file).ToLowerInvariant())) continue;
                var destination = Path.Combine(target, Path.GetFileName(file));
                if (File.Exists(destination)) continue;   // already brought across
                File.Move(file, destination);
                moved++;
            }
        }
        catch (Exception e)
        {
            // Files already moved stay moved. The rest are retried on the next launch.
            // Neither path deletes anything.
            Log.Line($"moving old pictures failed — {e.Message}");
        }

        if (moved > 0) Log.Line($"moved {moved} picture(s) from {legacy} to {target}");
    }

    // MARK: The folder

    /// Rebuilds the list from the folder.
    ///
    /// The scan reads only files that are new or changed, judged by last-write time and
    /// length. Hashing every picture on every pass made one new screenshot re-read the
    /// whole folder on the UI thread and rebuild every Shot, which discarded thumbnails
    /// that then had to be decoded again.
    ///
    /// The first scan after launch still reads every file once, because the content
    /// hashes require it. Retention bounds how many files that is.
    private void ScanFolder()
    {
        _shots.Clear();
        _hashes.Clear();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        try
        {
            foreach (var file in Directory.EnumerateFiles(Paths.Shots))
            {
                if (!Extensions.Contains(Path.GetExtension(file).ToLowerInvariant())) continue;

                FileInfo info;
                try
                {
                    info = new FileInfo(file);
                    if (!info.Exists) continue;
                }
                catch { continue; }

                var ticks = info.LastWriteTimeUtc.Ticks;
                var length = info.Length;
                seen.Add(file);

                if (!_known.TryGetValue(file, out var known)
                    || known.WriteTicks != ticks || known.Length != length)
                {
                    var hash = SafeHashFile(file);
                    // A file that cannot be read yet, because a screenshot tool is
                    // still writing it, is shown but not remembered. The next pass then
                    // retries instead of trusting a missing hash permanently.
                    if (hash is null)
                    {
                        _shots.Add(new Shot(file, info.LastWriteTime));
                        continue;
                    }
                    known = new Known(new Shot(file, info.LastWriteTime), ticks, length, hash);
                    _known[file] = known;
                }

                if (!_hashes.Add(known.Hash)) continue;   // a duplicate; skip it
                _shots.Add(known.Shot);
            }
        }
        catch (Exception e)
        {
            Log.Line($"shot scan failed — {e.Message}");
        }

        // Forget files that are gone, so the cache cannot grow without bound.
        foreach (var path in _known.Keys.Where(p => !seen.Contains(p)).ToList()) _known.Remove(path);

        Sort();
    }

    /// Registers a picture this class just wrote. The hash is already known, so the file
    /// is not read back.
    private void Add(string path, string hash)
    {
        if (!_hashes.Add(hash)) return;   // a duplicate; skip it

        DateTime taken;
        long ticks = 0, length = 0;
        try
        {
            var info = new FileInfo(path);
            taken = info.LastWriteTime;
            ticks = info.LastWriteTimeUtc.Ticks;
            length = info.Length;
        }
        catch { taken = DateTime.Now; }

        var shot = new Shot(path, taken);
        _known[path] = new Known(shot, ticks, length, hash);
        _shots.Add(shot);
        Sort();
        Notify();
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
            Add(path, hash);
            Log.Line($"captured clipboard image {Path.GetFileName(path)}");
        }
        catch (Exception e)
        {
            Log.Line($"clipboard capture failed — {e.Message}");
        }
    }

    // MARK: Mutations the tray asks for

    /// Takes a finished PNG — a snip, right now — writes it into the Shots folder and
    /// puts it at the top of the tray. Returns the path, or null if this exact picture
    /// is already here or the write failed. Shares the clipboard route's dedupe, so
    /// snipping the same unchanged window twice keeps one copy.
    public string? SavePng(byte[] png, string prefix)
    {
        try
        {
            var hash = Hash(png);
            if (_hashes.Contains(hash)) return null;

            var path = Path.Combine(Paths.Shots, $"{prefix}-{Stamp()}.png");
            File.WriteAllBytes(path, png);
            Add(path, hash);
            Log.Line($"saved {Path.GetFileName(path)}");
            return path;
        }
        catch (Exception e)
        {
            Log.Line($"saving a picture failed — {e.Message}");
            return null;
        }
    }

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
        _tidy?.Stop();
        _watcher?.Dispose();
        if (_clipboardSource is not null)
        {
            Win32.RemoveClipboardFormatListener(_clipboardSource.Handle);
            _clipboardSource.RemoveHook(ClipboardHook);
            _clipboardSource.Dispose();
        }
    }
}
