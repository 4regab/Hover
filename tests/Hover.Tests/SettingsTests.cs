using System.IO;
using System.Windows.Input;
using Hover.Core;
using NUnit.Framework;

namespace Hover.Tests;

[NonParallelizable]
public sealed class SettingsTests
{
    [Test]
    public void Flush_writes_readable_JSON_with_the_shortcut_notch_items_and_cards()
    {
        var shortcut = Settings.ScWorkspace;
        var items = Settings.NotchItems;
        var cards = Settings.Cards;
        try
        {
            Settings.ScWorkspace = new Shortcut(ModifierKeys.Control | ModifierKeys.Shift, Key.H);
            // "clock" was a notch item once; old settings files may still hold it.
            Settings.NotchItems = new[] { NotchItem.Kiro, "bogus", "clock", NotchItem.Timer };
            Settings.Cards = CardLayout.Show(CardLayout.Default, CardLayout.Shots, false);
            Settings.Flush();

            var json = File.ReadAllText(Paths.SettingsFile);
            Assert.Multiple(() =>
            {
                Assert.That(json, Does.Contain("\"ScWorkspace\""));
                Assert.That(json, Does.Contain("\"Key\": \"H\""));
                Assert.That(json, Does.Contain("\"NotchItems\""));
                Assert.That(json, Does.Contain("\"Cards\""));
                // Unknown ids are dropped, and the canonical order wins over the order given.
                Assert.That(Settings.NotchItems, Is.EqualTo(new[] { NotchItem.Timer, NotchItem.Kiro }));
                Assert.That(Settings.Cards.Single(c => c.Id == CardLayout.Shots).Visible, Is.False);
            });
        }
        finally
        {
            Settings.ScWorkspace = shortcut;
            Settings.NotchItems = items;
            Settings.Cards = cards;
            Settings.Flush();
        }
    }

    [Test]
    public void Kiros_folder_and_the_note_are_saved_and_a_blank_folder_is_none()
    {
        var folder = Settings.KiroFolder;
        var seen = Settings.KiroNoticeSeen;
        try
        {
            Settings.KiroFolder = @"C:\Projects\Hover";
            Settings.KiroNoticeSeen = true;
            Settings.Flush();
            var json = File.ReadAllText(Paths.SettingsFile);
            Assert.Multiple(() =>
            {
                Assert.That(json, Does.Contain("\"KiroFolder\": \"C:\\\\Projects\\\\Hover\""));
                Assert.That(json, Does.Contain("\"KiroNoticeSeen\": true"));
            });
            Settings.KiroFolder = "   ";
            Assert.That(Settings.KiroFolder, Is.Null);
        }
        finally
        {
            Settings.KiroFolder = folder;
            Settings.KiroNoticeSeen = seen;
            Settings.Flush();
        }
    }

    [Test]
    public void Notch_items_toggle_one_at_a_time()
    {
        var items = Settings.NotchItems;
        try
        {
            Settings.NotchItems = new[] { NotchItem.Timer };
            Settings.SetNotchItem(NotchItem.Codex, true);
            Assert.That(Settings.HasNotchItem(NotchItem.Codex), Is.True);
            Settings.SetNotchItem(NotchItem.Timer, false);
            Assert.That(Settings.NotchItems, Is.EqualTo(new[] { NotchItem.Codex }));
        }
        finally
        {
            Settings.NotchItems = items;
            Settings.Flush();
        }
    }
}
