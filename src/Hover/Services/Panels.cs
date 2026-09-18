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
    private static bool _watching;

    /// The notes panel, so the tray menu can open a new note in it.
    public static NoteDeck? Deck => _deck;

    public static void Install()
    {
        var notesEdge = Settings.DeckOnLeftEdge ? Edge.Left : Edge.Right;
        var shotsEdge = notesEdge == Edge.Left ? Edge.Right : Edge.Left;

        _deck = new NoteDeck(notesEdge == Edge.Right);
        _notes = new EdgePanel(notesEdge, _deck, Notes.DeckGeom.RestingWidth,
                               () => Settings.AutoHideNotes, opaque: false);
        // The deck knows how much room it needs: tabs only, tabs and a hover card, or
        // tabs and an open note.
        _deck.WidthWanted += (_, width) => _notes?.SetWidth(width);
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
        _shots = new EdgePanel(shotsEdge, trayScroll, 300, () => Settings.AutoHideShots);

        _tray.DeleteRequested += (_, shot) => ShotStore.Shared.Delete(shot);
        _tray.SnipRequested += (_, _) => Snip.Begin();
        _tray.MarkUpRequested += (_, shot) => MarkUp(shot);

        // The picture watcher is started once for the life of the app. Panels can be
        // built again when the edges are swapped; the folder does not need re-watching.
        if (!_watching)
        {
            ShotStore.Shared.Changed += OnShotsChanged;
            ShotStore.Shared.Start();
            _watching = true;
        }
        ReloadShots();
    }

    private static void OnShotsChanged(object? sender, EventArgs e) =>
        Dispatcher.UIThread.Post(ReloadShots);

    /// Opens the notes panel with a fresh note ready to type into. Used by the tray
    /// menu and the global shortcut, which are the only ways in when no panel is showing.
    public static void NewNote()
    {
        if (_notes is null || _deck is null) return;
        var screen = Interop.Screens.At(Interop.Screens.Cursor);
        if (screen is not null) _notes.Open(screen);
        _deck.Edit(NoteStore.Shared.Create());
    }

    /// Opens the notes panel on its list, without making a note.
    public static void ShowNotes()
    {
        if (_notes is null) return;
        var screen = Interop.Screens.At(Interop.Screens.Cursor);
        if (screen is not null) _notes.Open(screen);
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

    /// Throws both panels away and builds them again. Used after the edges are swapped
    /// in Settings, which is the one change a running panel cannot absorb.
    public static void Rebuild()
    {
        Close();
        Install();
    }

    private static void Close()
    {
        _notes?.Dispose();
        _shots?.Dispose();
        _notes = null;
        _shots = null;
        _deck = null;
        _tray = null;
    }

    public static void Dispose()
    {
        if (_watching)
        {
            ShotStore.Shared.Changed -= OnShotsChanged;
            _watching = false;
        }
        Close();
    }
}
