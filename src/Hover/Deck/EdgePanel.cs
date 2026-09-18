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
    private readonly double _width;
    private readonly Func<bool> _autoHide;
    private readonly HoverWindow _window;
    private readonly Border _frame;
    private readonly DispatcherTimer _poll;
    private readonly EdgeWake _wake = new();

    private ScreenInfo? _on;

    public bool IsOpen { get; private set; }

    /// Set by the content while it must not be taken away: a note being typed into,
    /// or a menu standing open over the panel.
    public bool Pinned { get; set; }

    public EdgePanel(Edge edge, Control content, double width, Func<bool> autoHide)
    {
        _edge = edge;
        _width = width;
        _autoHide = autoHide;

        _frame = new Border
        {
            Background = new SolidColorBrush(Color.Parse("#1C1C1E")),
            BorderBrush = new SolidColorBrush(Color.Parse("#2E2E30")),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(
                edge == Edge.Right ? 12 : 0, edge == Edge.Right ? 0 : 12,
                edge == Edge.Right ? 0 : 12, edge == Edge.Right ? 12 : 0),
            Padding = new Thickness(8),
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
        // The corners outside the rounded edge would otherwise still take clicks.
        _window.ShapeRounded(w, h, (int)Math.Round(12 * screen.Scale));
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
