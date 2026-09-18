using Microsoft.Data.Sqlite;
using Hover.Core;
using NUnit.Framework;
using System.IO;

namespace Hover.Tests;

/// Naming a note by hand, and the flag that stops a body edit taking the name away
/// again.
[NonParallelizable]
public sealed class NoteTitleTests
{
    private string _database = null!;
    private Store _databaseStore = null!;
    private NoteStore _notes = null!;

    [SetUp]
    public void SetUp()
    {
        _database = Path.Combine(TestEnvironment.Root, $"title-{Guid.NewGuid():N}.db");
        _databaseStore = new Store(_database);
        _notes = new NoteStore(_databaseStore);
    }

    [TearDown]
    public void TearDown()
    {
        _notes.Dispose();
        SqliteConnection.ClearAllPools();
        DeleteIfPresent(_database);
        DeleteIfPresent(_database + "-wal");
        DeleteIfPresent(_database + "-shm");
    }

    // MARK: The pure rules

    [Test]
    public void TitleFor_follows_the_body_until_the_title_is_locked()
    {
        Assert.Multiple(() =>
        {
            Assert.That(Note.TitleFor("# First line\nrest", "old", locked: false),
                Is.EqualTo("First line"));
            Assert.That(Note.TitleFor("# First line\nrest", "Chosen", locked: true),
                Is.EqualTo("Chosen"));
        });
    }

    [TestCase("  Shopping  ", "Shopping")]
    [TestCase("Two\nlines", "Two lines")]
    [TestCase("Tabs\tand\r\nbreaks", "Tabs and breaks")]
    [TestCase("wide    gaps", "wide gaps")]
    [TestCase("   ", "")]
    [TestCase(null, "")]
    public void CleanTitle_flattens_a_typed_name(string? raw, string expected) =>
        Assert.That(Note.CleanTitle(raw), Is.EqualTo(expected));

    [Test]
    public void CleanTitle_caps_a_name_at_the_deck_limit()
    {
        var result = Note.CleanTitle(new string('a', 200));

        Assert.Multiple(() =>
        {
            Assert.That(result, Has.Length.EqualTo(Note.TitleLimit + 1));
            Assert.That(result, Does.EndWith("…"));
        });
    }

    // MARK: The store

    [Test]
    public void SetTitle_names_the_note_and_survives_a_body_edit()
    {
        var note = _notes.Create("# Derived\nbody");

        _notes.SetTitle(note.Id, "Groceries");
        _notes.UpdateBody(note.Id, "# Something else entirely\nmore");

        Assert.Multiple(() =>
        {
            Assert.That(note.Title, Is.EqualTo("Groceries"));
            Assert.That(note.TitleLocked, Is.True);
            Assert.That(note.Body, Is.EqualTo("# Something else entirely\nmore"));
        });
    }

    [Test]
    public void SetTitle_with_an_empty_name_goes_back_to_following_the_body()
    {
        var note = _notes.Create("# Derived\nbody");
        _notes.SetTitle(note.Id, "Groceries");

        _notes.SetTitle(note.Id, "   ");

        Assert.Multiple(() =>
        {
            Assert.That(note.TitleLocked, Is.False);
            Assert.That(note.Title, Is.EqualTo("Derived"));
        });

        _notes.UpdateBody(note.Id, "# Moved on\nbody");
        Assert.That(note.Title, Is.EqualTo("Moved on"));
    }

    [Test]
    public void SetTitle_tidies_the_name_and_notifies_only_on_a_real_change()
    {
        var note = _notes.Create("body");
        var changes = 0;
        _notes.NotesChanged += (_, _) => changes++;

        _notes.SetTitle(note.Id, "  Plan\nfor Friday  ");
        _notes.SetTitle(note.Id, "Plan for Friday");

        Assert.Multiple(() =>
        {
            Assert.That(note.Title, Is.EqualTo("Plan for Friday"));
            Assert.That(changes, Is.EqualTo(1));
        });
    }

    [Test]
    public void SetTitle_ignores_an_unknown_note() =>
        Assert.That(() => _notes.SetTitle("no-such-note", "x"), Throws.Nothing);

    [Test]
    public void A_named_note_reloads_named()
    {
        var note = _notes.Create("# Derived\nbody");
        _notes.SetTitle(note.Id, "Groceries");

        using var reloaded = new Store(_database);
        var fromDisk = reloaded.Load().Single();

        Assert.Multiple(() =>
        {
            Assert.That(fromDisk.Title, Is.EqualTo("Groceries"));
            Assert.That(fromDisk.TitleLocked, Is.True);
        });
    }

    [Test]
    public void An_unnamed_note_reloads_unlocked()
    {
        _notes.Create("# Derived\nbody");

        using var reloaded = new Store(_database);
        var fromDisk = reloaded.Load().Single();

        Assert.Multiple(() =>
        {
            Assert.That(fromDisk.Title, Is.EqualTo("Derived"));
            Assert.That(fromDisk.TitleLocked, Is.False);
        });
    }

    [Test]
    public void A_named_note_is_not_thrown_away_as_an_empty_draft()
    {
        var draft = _notes.Create("");
        _notes.SetTitle(draft.Id, "Groceries");

        Assert.Multiple(() =>
        {
            Assert.That(_notes.DiscardIfEmpty(draft.Id), Is.False);
            Assert.That(_notes.Get(draft.Id), Is.SameAs(draft));
        });
    }

    [Test]
    public void An_unnamed_empty_draft_is_still_thrown_away()
    {
        var draft = _notes.Create("  \n");

        Assert.Multiple(() =>
        {
            Assert.That(_notes.DiscardIfEmpty(draft.Id), Is.True);
            Assert.That(_notes.Get(draft.Id), Is.Null);
        });
    }

    private static void DeleteIfPresent(string path)
    {
        if (File.Exists(path)) File.Delete(path);
    }
}
