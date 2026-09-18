using Hover.Core;

namespace Hover.Interop;

/// Decides when a pointer in a wake zone is the user actually asking for the panel,
/// and how wide that zone is.
///
/// The zone sits on the same strip of screen as a maximised window's scrollbar, so
/// "the pointer is in the zone" on its own is not a request — reaching for the
/// scrollbar says exactly the same thing. Two rules separate the two:
///
///  * the pointer has to *rest* in the zone for `Settings.WakeDelay`, so passing
///    through the edge never opens anything;
///  * a held mouse button resets the clock, so clicking at the edge or dragging a
///    scrollbar down it can never open anything either.
///
/// One instance per manager, keyed by display device. It also carries the
/// fire-once-per-visit latch the managers used to keep themselves: a fan tidies
/// itself away after a few seconds, and without the latch the pointer parked where
/// it was would simply wake it again on the next poll.
public sealed class EdgeWake
{
    private readonly Dictionary<string, DateTime> _since = new();
    private readonly HashSet<string> _woke = new();

    /// How far into the screen a pinned wake zone reaches, in device pixels. Two
    /// rather than one: the pointer lands on the last pixel, and a stray rounding
    /// in either direction should not make the edge unreachable.
    public const int PinnedPixels = 2;

    /// How wide the wake zone is on one screen edge, in device pixels.
    ///
    /// Normally the band from preferences, which is wide enough to hit easily — and
    /// wide enough to cover a scrollbar, which is the whole problem. With "only at
    /// the very edge" on it shrinks to `PinnedPixels`: the pointer then has to be
    /// pushed into the screen edge itself, where it stops dead. A scrollbar thumb
    /// holds the pointer several pixels short of that, so scrolling or clicking one
    /// can no longer wake anything.
    ///
    /// An edge another display butts against keeps the full band: there the pointer
    /// crosses onto the next screen rather than stopping, so there is nothing to
    /// push against.
    public static int WakeBandWidth(ScreenInfo screen, bool onRight, double bandDips)
    {
        var shared = onRight ? screen.NeighbourRight : screen.NeighbourLeft;
        if (Settings.WakeAtScreenEdge && !shared) return PinnedPixels;
        return (int)Math.Round(bandDips * screen.Scale);
    }

    /// Call once per poll per display. True on the single tick the wait is met.
    public bool Woke(string device, bool inside)
    {
        if (!inside)
        {
            _since.Remove(device);
            _woke.Remove(device);
            return false;
        }
        // Already opened for this visit — the pointer has to leave and come back.
        if (_woke.Contains(device)) return false;

        if (Win32.AnyMouseButtonDown)
        {
            _since.Remove(device);
            return false;
        }

        if (!_since.TryGetValue(device, out var since))
        {
            since = DateTime.UtcNow;
            _since[device] = since;
        }
        // Not `>=`: with no delay set this is the arrival tick itself, and the panel
        // should come out on it rather than one poll later.
        if (DateTime.UtcNow - since < Settings.WakeDelay) return false;

        _woke.Add(device);
        return true;
    }

    /// Displays that have gone away leave state behind otherwise.
    public void Forget(string device)
    {
        _since.Remove(device);
        _woke.Remove(device);
    }
}
