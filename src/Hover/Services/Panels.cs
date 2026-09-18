using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Threading;
using Hover.Core;
using Hover.Deck;
using Hover.Images;
using Hover.Notes;

namespace Hover.Services;

/// The two edge panels, and everything they are wired to.
///
/// Notes on one edge, screenshots on the other; which side notes take is a preference,
/// and the screenshots always take the opposite one so the two can never land on top
/// of each other.
public static class Panels
{
    private static EdgePanel? _notes;
    private static EdgePanel? _shots;
    private static NoteDeck? _deck;
    private static ShotTray? _tray;

    /// The notes panel, so the tray menu can open a new note in it.
    public static NoteDeck? Deck => _deck;

    public static void Install()
    {
        var notesEdge = Settings.DeckOnLeftEdge ? Edge.Left : Edge.Right;
        var shotsEdge = notesEdge == Edge.Left ? Edge.Right : Edge.Left;

        _deck = new NoteDeck();
        _notes = new EdgePanel(notesEdge, _deck, 330, Settings.AutoHideNotes);
        // A note being typed into must not be whipped away, and it needs the keyboard.
        _deck.Typing += (_, typing) =>
        {
            if (_notes is null) return;
            _notes.Pinned = typing;
            _notes.AcceptKeys(typing);
        };

        _tray = new ShotTray();
        var trayScroll = new ScrollViewer
        {
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
            Content = _tray,
        };
        _shots = new EdgePanel(shotsEdge, trayScroll, 300, Settings.AutoHideShots);

        _tray.DeleteRequested += (_, shot) => ShotStore.Shared.Delete(shot);
        _tray.SnipRequested += (_, _) => Snip.Begin();
        _tray.MarkUpRequested += (_, shot) => MarkUp(shot);
        _tray.DragRequested += (_, shot) => _shots?.WhileDragging(() => { });
        ShotStore.Shared.Changed += (_, _) => Dispatcher.UIThread.Post(ReloadShots);

        ShotStore.Shared.Start();
        ReloadShots();
    }

    /// Opens the notes panel with a fresh note ready to type into. Used by the tray
    /// menu, which is the only way in when no panel is showing.
    public static void NewNote()
    {
        if (_notes is null || _deck is null) return;
        var screen = Interop.Screens.At(Interop.Screens.Cursor);
        if (screen is not null) _notes.Open(screen);
        _deck.Edit(NoteStore.Shared.Create());
    }

    /// Opens the screenshots panel without waiting for the pointer to reach the edge.
    public static void ShowShots()
    {
        if (_shots is null) return;
        var screen = Interop.Screens.At(Interop.Screens.Cursor);
        if (screen is not null) _shots.Open(screen);
    }

    private static void ReloadShots() => _tray?.Show(ShotStore.Shared.Shots);

    /// Opens a picture already in the tray for drawing on. Saving adds an edited copy
    /// rather than writing over the original.
    private static void MarkUp(Shot shot)
    {
        var picture = shot.FullSize();
        if (picture is null)
        {
            Log.Line($"could not open {shot.Name} for drawing on");
            return;
        }
        Snip.MarkUp(picture);
    }

    public static void Dispose()
    {
        _notes?.Dispose();
        _shots?.Dispose();
    }
}
