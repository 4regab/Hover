using System.Windows.Threading;
using Hover.Core;
using Hover.Interop;

namespace Hover.Images;

/// One image tray per display, woken by the pointer reaching the tray's edge. A
/// thin sibling of DeckManager — same polling idea, none of the note machinery.
public sealed class ImageStripManager : IDisposable
{
    private readonly Dictionary<string, ImageStripController> _strips = new();
    private readonly EdgeWake _wake = new();
    private readonly DispatcherTimer _poll;
    private string _layoutSignature = "";
    private DateTime _lastDisplayCheck = DateTime.Now;
    private static readonly TimeSpan DisplayCheckEvery = TimeSpan.FromSeconds(2);

    public ImageStripManager()
    {
        var screens = Screens.All();
        _layoutSignature = Signature(screens);
        Rebuild(screens);
        _poll = new DispatcherTimer(DispatcherPriority.Background)
        {
            Interval = TimeSpan.FromMilliseconds(90),
        };
        _poll.Tick += (_, _) => Tick();
        _poll.Start();
    }

    private static string Signature(List<ScreenInfo> screens) =>
        string.Join("|", screens.Select(s =>
            $"{s.Device}:{s.Bounds.Left},{s.Bounds.Top},{s.Bounds.Right},{s.Bounds.Bottom}@{s.Scale}"));

    private void Tick()
    {
        if (DateTime.Now - _lastDisplayCheck > DisplayCheckEvery)
        {
            _lastDisplayCheck = DateTime.Now;
            var screens = Screens.All();
            var signature = Signature(screens);
            if (signature != _layoutSignature)
            {
                _layoutSignature = signature;
                Rebuild(screens);
                return;
            }
        }

        // Same gate as the notes deck: the pointer has to rest on the edge, with no
        // button held, before the tray comes out.
        var p = Screens.Cursor;
        foreach (var strip in _strips.Values)
            if (_wake.Woke(strip.Device, strip.EdgeStrip.Contains(p)))
                strip.PointerEntered();
    }

    public void Rebuild(List<ScreenInfo>? screens = null)
    {
        screens ??= Screens.All();
        var live = screens.ToDictionary(s => s.Device);

        foreach (var device in _strips.Keys.Where(d => !live.ContainsKey(d)).ToList())
        {
            _strips[device].Dispose();
            _strips.Remove(device);
            _wake.Forget(device);
        }
        foreach (var (device, screen) in live)
        {
            if (_strips.TryGetValue(device, out var existing)) existing.UpdateScreen(screen);
            else _strips[device] = new ImageStripController(screen) { Manager = this };
        }
    }

    /// Only one tray open at a time — the one the pointer entered.
    public void StripDidActivate(ImageStripController active)
    {
        foreach (var s in _strips.Values)
            if (!ReferenceEquals(s, active)) s.Collapse();
    }

    /// Shut every tray. Asked for before a snip, so the tray is not in the picture.
    public void CollapseAll()
    {
        foreach (var s in _strips.Values) s.Collapse();
    }

    /// Re-place every tray after a preference change (the tray edge follows the note
    /// deck edge, so flipping the deck flips the tray).
    public void RefreshAll()
    {
        foreach (var s in _strips.Values)
        {
            s.RefreshLevel();
            s.Layout();
            s.Redraw();
        }
    }

    public void Dispose()
    {
        _poll.Stop();
        foreach (var s in _strips.Values) s.Dispose();
        _strips.Clear();
    }
}
