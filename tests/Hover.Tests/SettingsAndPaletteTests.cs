using Avalonia.Input;
using Avalonia.Media;
using Hover.Core;
using NUnit.Framework;
using System.IO;

namespace Hover.Tests;

[NonParallelizable]
public sealed class SettingsAndPaletteTests
{
    [Test]
    public void FontSize_is_clamped_to_the_supported_range()
    {
        var original = Settings.NoteFontSize;
        try
        {
            Settings.NoteFontSize = -100;
            Assert.That(Settings.NoteFontSize, Is.EqualTo(Settings.FontMin));

            Settings.NoteFontSize = 100;
            Assert.That(Settings.NoteFontSize, Is.EqualTo(Settings.FontMax));
        }
        finally
        {
            Settings.NoteFontSize = original;
            Settings.Flush();
        }
    }

    [Test]
    public void Auto_hide_is_on_by_default_and_both_panels_toggle_independently()
    {
        var originalNotes = Settings.AutoHideNotes;
        var originalShots = Settings.AutoHideShots;
        try
        {
            Assert.Multiple(() =>
            {
                Assert.That(originalNotes, Is.True, "a fresh install hides the deck by itself");
                Assert.That(originalShots, Is.True, "a fresh install hides the tray by itself");
            });

            Settings.AutoHideNotes = false;
            Assert.Multiple(() =>
            {
                Assert.That(Settings.AutoHideNotes, Is.False);
                Assert.That(Settings.AutoHideShots, Is.True, "the tray is not dragged along");
            });

            Settings.AutoHideShots = false;
            Settings.AutoHideNotes = true;
            Assert.Multiple(() =>
            {
                Assert.That(Settings.AutoHideNotes, Is.True);
                Assert.That(Settings.AutoHideShots, Is.False);
            });
        }
        finally
        {
            Settings.AutoHideNotes = originalNotes;
            Settings.AutoHideShots = originalShots;
            Settings.Flush();
        }
    }

    [Test]
    public void Invalid_edge_width_falls_back_to_the_standard_width()
    {
        var original = Settings.EdgeWidth;
        try
        {
            Settings.EdgeWidth = -1;
            Assert.That(Settings.EdgeWidth, Is.EqualTo(14));
        }
        finally
        {
            Settings.EdgeWidth = original;
            Settings.Flush();
        }
    }

    [Test]
    public void Flush_writes_readable_JSON_with_enum_and_shortcut_values()
    {
        var originalStyle = Settings.DeckStyle;
        var originalShortcut = Settings.ScFind;
        try
        {
            Settings.DeckStyle = DeckStyle.Compact;
            Settings.ScFind = new Shortcut(KeyModifiers.Control | KeyModifiers.Shift, Key.F);

            Settings.Flush();
            var json = File.ReadAllText(Paths.SettingsFile);

            Assert.Multiple(() =>
            {
                Assert.That(json, Does.Contain("\"DeckStyle\": \"Compact\""));
                Assert.That(json, Does.Contain("\"Modifiers\": \"Control, Shift\""));
                Assert.That(json, Does.Contain("\"Key\": \"F\""));
            });
        }
        finally
        {
            Settings.DeckStyle = originalStyle;
            Settings.ScFind = originalShortcut;
            Settings.Flush();
        }
    }

    [Test]
    public void Palette_index_wraps_in_both_directions()
    {
        Assert.Multiple(() =>
        {
            Assert.That(NoteColor.At(NoteColor.All.Count), Is.SameAs(NoteColor.At(0)));
            Assert.That(NoteColor.At(-1), Is.SameAs(NoteColor.All[^1]));
        });
    }

    [Test]
    public void Tint_clamps_alpha_and_preserves_RGB()
    {
        var color = Color.FromRgb(10, 20, 30);
        var transparent = (SolidColorBrush)NoteColor.Tint(color, -1);
        var opaque = (SolidColorBrush)NoteColor.Tint(color, 2);

        Assert.Multiple(() =>
        {
            Assert.That(transparent.Color, Is.EqualTo(Color.FromArgb(0, 10, 20, 30)));
            Assert.That(opaque.Color, Is.EqualTo(Color.FromArgb(255, 10, 20, 30)));
            // No frozen-brush check: Avalonia has no such thing. It was a WPF
            // performance detail, not something the app's behaviour depends on.
        });
    }
}
