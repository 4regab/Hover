using Avalonia;
using Avalonia.Controls;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Threading;
using Hover.Core;
using Hover.Interop;

namespace Hover.Deck;

public enum Edge
{
    Left,
    Right,
}

/// A panel that lives on a screen edge and comes out when the pointer rests there.
///
/// One of these per half of the app: notes on one edge, screenshots on the other.
/// Everything about *where* a panel sits and *when* it shows is here, so the notes
/// deck and the screenshot tray only have to be ordinary controls.
///
/// The pointer is polled rather than hooked, which needs no special permission and
/// cannot wedge the mouse if this code throws. `EdgeWake` decides whether a pointer
/// in the edge band is really a request; see the reasoning there.
///
/// ponytail: one window that moves to whichever screen woke it, rather than one
/// window per display as the old app had. On two monitors the panel therefore cannot
/// be open on both at once — which nobody can do with one pointer anyway. The upgrade
/// path is a window per `ScreenInfo`, keyed by device name.
public sealed class EdgePanel : IDisposable
{
    private const int PollMs = 90;

    /// How far outside the panel the pointer may wander before the panel folds away.
    /// Without the margin, a pointer a pixel off the panel while reaching for a tab
    /// would shut it.
    private const double Slack = 26;

    private readonly Edge _edge;
    private double _width;
    private readonly Func<bool> _autoHide;
    private readonly HoverWindow _window;
    private readonly Border _frame;
    private readonly DispatcherTimer _poll;
    private readonly EdgeWake _wake = new();

    private ScreenInfo? _on;
    private IReadOnlyList<Rect> _parts = Array.Empty<Rect>();

    /// How far into the panel the pointer counts as being "on the deck".
    ///
    /// A panel is as wide as the widest sheet it can draw, but leaving the deck means
    /// leaving the strip along the screen edge — not leaving the whole window. Without
    /// this, a panel wide enough for an open note would never close.
    public double LiveStrip { get; set; }

    public bool IsOpen { get; private set; }

    /// The screen the panel is out on, and where it sits there in device pixels. The
    /// hover card is a separate window and needs both to place itself.
    public ScreenInfo? On => _on;

    public Win32.RECT Where
    {
        get
        {
            if (_on is not { } screen) return default;
            var (x, y, w, h) = Rect(screen);
            return new Win32.RECT { Left = x, Top = y, Right = x + w, Bottom = y + h };
        }
    }

    /// Set by the content while it must not be taken away: a note being typed into,
    /// or a menu standing open over the panel.
    public bool Pinned { get; set; }

    public EdgePanel(Edge edge, Control content, double width, Func<bool> autoHide,
                     bool opaque = true)
    {
        _edge = edge;
        _width = width;
        _autoHide = autoHide;

        _frame = new Border
        {
            // The notes deck paints its own paper and floats on the desktop, so it asks
            // for no panel behind it. The screenshot tray is a dark card and does.
            Background = opaque ? new SolidColorBrush(Color.Parse("#1C1C1E")) : null,
            BorderBrush = opaque ? new SolidColorBrush(Color.Parse("#2E2E30")) : null,
            BorderThickness = new Thickness(opaque ? 1 : 0),
            CornerRadius = new CornerRadius(
                edge == Edge.Right ? 12 : 0, edge == Edge.Right ? 0 : 12,
                edge == Edge.Right ? 0 : 12, edge == Edge.Right ? 12 : 0),
            Padding = new Thickness(opaque ? 8 : 0),
            Child = content,
            HorizontalAlignment = Avalonia.Layout.HorizontalAlignment.Stretch,
            VerticalAlignment = Avalonia.Layout.VerticalAlignment.Stretch,
        };

        _window = new HoverWindow { Content = _frame };

        _poll = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(PollMs) };
        _poll.Tick += (_, _) => Look();
        _poll.Start();
    }

    /// Lets an open note take the keyboard. A resting panel never does.
    public void AcceptKeys(bool value) => _window.SetAcceptsKeys(value);

    /// Changes how wide the panel is.
    ///
    /// The notes deck asks for this: narrow while it is only showing tabs, wider for a
    /// hover card, wider again for an open note. Every pixel of an open panel is a pixel
    /// the mouse cannot click through, so the panel is kept no wider than it has to be.
    public void SetWidth(double dips)
    {
        if (Math.Abs(_width - dips) < 0.5) return;
        _width = dips;
        if (IsOpen && _on is not null) Open(_on);
    }

    /// Tells Windows which parts of the panel are really there.
    ///
    /// A panel is as wide as the widest thing it can show, but most of the time it only
    /// paints a strip along the screen edge. Without a shape, the empty part would still
    /// swallow clicks meant for the window underneath. Rectangles are in layout units,
    /// measured from the panel's own top-left corner.
    public void SetParts(IReadOnlyList<Rect> parts)
    {
        _parts = parts;
        Shape();
    }

    private void Shape()
    {
        if (_on is not { } screen) return;
        var scale = screen.Scale;
        var (_, _, w, h) = Rect(screen);

        if (_parts.Count == 0)
        {
            _window.ShapeRounded(w, h, (int)Math.Round(12 * scale));
            return;
        }

        var pixels = _parts.Select(p => new Win32.RECT
        {
            Left = (int)Math.Floor(p.X * scale),
            Top = (int)Math.Floor(p.Y * scale),
            Right = (int)Math.Ceiling(p.Right * scale),
            Bottom = (int)Math.Ceiling(p.Bottom * scale),
        }).ToList();
        _window.ShapeParts(pixels, (int)Math.Round(10 * scale));
    }

    /// Runs a drag with the window briefly allowed to activate, which is what other
    /// apps demand of a drag source.
    public void WhileDragging(Action body) => _window.WhileActivatable(body);

    private void Look()
    {
        try
        {
            var cursor = Interop.Screens.Cursor;
            if (IsOpen) Maybe_Close(cursor);
            else Maybe_Open(cursor);
        }
        catch (Exception e)
        {
            // A throw here would stop the timer and the panel would never open again.
            Log.Line($"edge poll failed — {e.Message}");
        }
    }

    private void Maybe_Open(Win32.POINT cursor)
    {
        var screen = Interop.Screens.At(cursor);
        if (screen is null) return;

        var onRight = _edge == Edge.Right;
        var band = EdgeWake.WakeBandWidth(screen, onRight, Settings.EdgeWidth);
        var inside = onRight
            ? cursor.X >= screen.Bounds.Right - band
            : cursor.X <= screen.Bounds.Left + band;
        // Along the working area only, so the taskbar corner is not a wake zone.
        inside &= cursor.Y >= screen.Work.Top && cursor.Y <= screen.Work.Bottom;

        if (!_wake.Woke(screen.Device, inside)) return;
        Open(screen);
    }

    private void Maybe_Close(Win32.POINT cursor)
    {
        // Read every time, so turning auto-hide off in Settings takes effect at once.
        if (Pinned || !_autoHide()) return;
        if (_on is not { } screen) return;

        var (x, y, w, h) = Rect(screen);
        var slack = (int)Math.Round(Slack * screen.Scale);

        // Leaving the deck means leaving the parts that are actually drawn, not the
        // window, which is as wide as the widest sheet the panel can show.
        if (_parts.Count > 0)
        {
            foreach (var part in _parts)
            {
                var left = x + (int)Math.Floor(part.X * screen.Scale) - slack;
                var top = y + (int)Math.Floor(part.Y * screen.Scale) - slack;
                var right = x + (int)Math.Ceiling(part.Right * screen.Scale) + slack;
                var bottom = y + (int)Math.Ceiling(part.Bottom * screen.Scale) + slack;
                if (cursor.X >= left && cursor.X <= right &&
                    cursor.Y >= top && cursor.Y <= bottom) return;
            }
            Close();
            return;
        }

        var near = cursor.X >= x - slack && cursor.X <= x + w + slack &&
                   cursor.Y >= y - slack && cursor.Y <= y + h + slack;
        if (near) return;
        Close();
    }

    /// Brings the panel out on one screen, sized to the content.
    public void Open(ScreenInfo screen)
    {
        _on = screen;
        var (x, y, w, h) = Rect(screen);

        _window.Width = _width;
        _window.Height = h / screen.Scale;
        _window.Show();
        _window.PlaceDevice(x, y, w, h);
        Shape();
        _window.Raise();
        IsOpen = true;
    }

    public void Close()
    {
        if (!IsOpen) return;
        IsOpen = false;
        Pinned = false;
        AcceptKeys(false);
        _window.Hide();
    }

    /// The panel fills the height of the working area, hugging one edge of it.
    private (int X, int Y, int W, int H) Rect(ScreenInfo screen)
    {
        var w = (int)Math.Round(_width * screen.Scale);
        var h = screen.Work.Height;
        var x = _edge == Edge.Right ? screen.Work.Right - w : screen.Work.Left;
        return (x, screen.Work.Top, w, h);
    }

    public void Dispose()
    {
        _poll.Stop();
        _window.Close();
    }
}
