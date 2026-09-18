using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Headless.NUnit;
using Avalonia.Input;
using Avalonia.Threading;
using Hover.Core;
using Hover.Notes;
using NUnit.Framework;

namespace Hover.Tests;

/// The notes panel: opening a note, typing into it, and the note being written away.
///
/// Typing into a note is the one thing in this app that must never lose work, so these
/// go through the real control rather than the store underneath it.
public sealed class NoteDeckTests
{
    private static (Window Window, NoteDeck Deck) Open()
    {
        var deck = new NoteDeck();
        var window = new Window
        {
            Width = 330,
            Height = 600,
            WindowDecorations = Avalonia.Controls.WindowDecorations.None,
            Content = deck,
        };
        window.Show();
        Dispatcher.UIThread.RunJobs();
        return (window, deck);
    }

    [AvaloniaTest]
    public void Typing_into_a_note_is_written_away_when_it_closes()
    {
        var (window, deck) = Open();
        var note = NoteStore.Shared.Create();

        deck.Edit(note);
        Dispatcher.UIThread.RunJobs();   // the editor takes the keyboard on the next pass
        window.KeyTextInput("Milk and bread");
        Dispatcher.UIThread.RunJobs();
        deck.CloseNote();

        var saved = NoteStore.Shared.Get(note.Id);
        Assert.That(saved, Is.Not.Null, "the note was thrown away");
        Assert.That(saved!.Body, Is.EqualTo("Milk and bread"));
        window.Close();
    }

    /// The title follows the first line, so the list shows something recognisable
    /// without the user naming anything.
    [AvaloniaTest]
    public void The_first_line_becomes_the_name_of_the_note()
    {
        var (window, deck) = Open();
        var note = NoteStore.Shared.Create();

        deck.Edit(note);
        Dispatcher.UIThread.RunJobs();   // the editor takes the keyboard on the next pass
        window.KeyTextInput("Shopping");
        Dispatcher.UIThread.RunJobs();
        deck.CloseNote();

        Assert.That(NoteStore.Shared.Get(note.Id)!.DisplayTitle, Is.EqualTo("Shopping"));
        window.Close();
    }

    /// A note opened and left blank is not a note. Otherwise every stray click on the
    /// new-note button would leave an empty row behind.
    [AvaloniaTest]
    public void A_note_left_blank_does_not_stay_in_the_list()
    {
        var (window, deck) = Open();
        var note = NoteStore.Shared.Create();

        deck.Edit(note);
        Dispatcher.UIThread.RunJobs();   // the editor takes the keyboard on the next pass
        deck.CloseNote();

        Assert.That(NoteStore.Shared.Get(note.Id), Is.Null);
        window.Close();
    }

    /// Escape is the way out of a note, and it must save on the way.
    [AvaloniaTest]
    public void Escape_leaves_the_note_and_keeps_what_was_typed()
    {
        var (window, deck) = Open();
        var note = NoteStore.Shared.Create();
        var typing = new List<bool>();
        deck.Typing += (_, on) => typing.Add(on);

        deck.Edit(note);
        Dispatcher.UIThread.RunJobs();   // the editor takes the keyboard on the next pass
        window.KeyTextInput("Ring the dentist");
        Dispatcher.UIThread.RunJobs();
        window.KeyPress(Key.Escape, RawInputModifiers.None, PhysicalKey.Escape, null);
        Dispatcher.UIThread.RunJobs();

        Assert.Multiple(() =>
        {
            Assert.That(NoteStore.Shared.Get(note.Id)?.Body, Is.EqualTo("Ring the dentist"));
            Assert.That(typing, Is.EqualTo(new[] { true, false }),
                "the panel must be told to hold the keyboard and then let it go");
        });
        window.Close();
    }
}
