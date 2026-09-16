using Hover.Core;
using Hover.Services;
using NUnit.Framework;

namespace Hover.Tests;

public sealed class TransferTests
{
    [Test]
    public void StickyNote_round_trips_the_portable_archive_fields()
    {
        var source = new Note
        {
            Id = "portable",
            Title = "Title",
            Body = "☐ body",
            Color = 3,
            Created = new DateTime(2025, 1, 2, 3, 4, 5),
            Modified = new DateTime(2026, 6, 7, 8, 9, 10),
            Archived = true,
            Pinned = true,
            Order = 4.5,
        };

        var portable = new StickyNote(source);
        var restored = portable.ToNote();

        Assert.Multiple(() =>
        {
            Assert.That(portable.ColorName, Is.EqualTo(source.Palette.Name));
            Assert.That(restored.Id, Is.EqualTo(source.Id));
            Assert.That(restored.Title, Is.EqualTo(source.Title));
            Assert.That(restored.Body, Is.EqualTo(source.Body));
            Assert.That(restored.Color, Is.EqualTo(source.Color));
            Assert.That(restored.Created, Is.EqualTo(source.Created));
            Assert.That(restored.Modified, Is.EqualTo(source.Modified));
            Assert.That(restored.Archived, Is.True);
            Assert.That(restored.Order, Is.EqualTo(source.Order));
            Assert.That(restored.Pinned, Is.False, "the shared archive format has no pinned field");
        });
    }

    [Test]
    public void StickyNote_carries_a_hand_written_title_through_an_export()
    {
        var named = new StickyNote(new Note { Title = "Groceries", Body = "milk", TitleLocked = true });
        var derived = new StickyNote(new Note { Title = "milk", Body = "milk" });

        Assert.Multiple(() =>
        {
            Assert.That(named.ToNote().TitleLocked, Is.True);
            Assert.That(named.ToNote().Title, Is.EqualTo("Groceries"));
            Assert.That(derived.ToNote().TitleLocked, Is.False);
        });
    }

    [Test]
    public void An_import_without_a_title_is_never_treated_as_hand_written()
    {
        // A file written by another tool can claim the flag while carrying no title.
        var odd = new StickyNote { Title = "", Body = "# Derived\nrest", TitleLocked = true };

        var restored = odd.ToNote();

        Assert.Multiple(() =>
        {
            Assert.That(restored.Title, Is.EqualTo("Derived"));
            Assert.That(restored.TitleLocked, Is.False);
        });
    }

    [Test]
    public void StickyNote_derives_a_missing_title_on_import()
    {
        var restored = new StickyNote { Body = "# Imported\nBody" }.ToNote();

        Assert.That(restored.Title, Is.EqualTo("Imported"));
    }
}
