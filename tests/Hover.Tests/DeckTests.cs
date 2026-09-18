using Hover.Core;
using Hover.Deck;
using Hover.Interop;
using NUnit.Framework;

namespace Hover.Tests;

public sealed class DeckTests
{
    [Test]
    public void DeckState_exposes_only_an_expanded_note_id()
    {
        var expanded = DeckState.Expanded("note-1");

        Assert.Multiple(() =>
        {
            Assert.That(DeckState.Rest.ExpandedId, Is.Null);
            Assert.That(DeckState.Fan.ExpandedId, Is.Null);
            Assert.That(expanded.ExpandedId, Is.EqualTo("note-1"));
            Assert.That(expanded.ToString(), Is.EqualTo("expanded(note-1)"));
            Assert.That(expanded.Rank, Is.GreaterThan(DeckState.Fan.Rank));
        });
    }

    [TestCase(0)]
    [TestCase(1)]
    [TestCase(14)]
    [TestCase(30)]
    public void PillHeight_is_positive_and_bounded(int count)
    {
        var height = DeckGeom.PillHeight(count, 1.25);

        Assert.That(height, Is.GreaterThan(0));
        Assert.That(height, Is.LessThanOrEqualTo(DeckGeom.PillHeight(1000, 1.25)));
    }

    [Test]
    public void Compact_layout_uses_fixed_chip_spacing()
    {
        var layout = DeckGeom.Layout(800, 4, hasMore: true, DeckStyle.Compact);

        Assert.Multiple(() =>
        {
            Assert.That(layout.Count, Is.EqualTo(4));
            Assert.That(layout.ItemHeight, Is.EqualTo(DeckGeom.ChipHeight));
            Assert.That(layout.Pitch, Is.EqualTo(DeckGeom.ChipHeight + DeckGeom.ChipGap));
            Assert.That(layout.HasMore, Is.True);
        });
    }

    [Test]
    public void Tab_layout_shrinks_to_fit_a_short_panel()
    {
        var roomy = DeckGeom.Layout(1200, 5, false, DeckStyle.Tabs, longestLabel: 90);
        var shortPanel = DeckGeom.Layout(300, 5, false, DeckStyle.Tabs, longestLabel: 90);

        Assert.That(shortPanel.Pitch, Is.LessThan(roomy.Pitch));
    }

    [Test]
    public void Overflow_layout_keeps_readable_tab_pitch()
    {
        var layout = DeckGeom.Layout(300, 20, false, DeckStyle.Tabs,
                                     longestLabel: 70, allowOverflow: true);

        Assert.Multiple(() =>
        {
            Assert.That(layout.Pitch, Is.GreaterThanOrEqualTo(DeckGeom.PitchMin));
            Assert.That(layout.Overflows, Is.True);
        });
    }

    /// The gate that stopped the deck opening every time the pointer crossed the edge
    /// on its way to a scrollbar. Silent breakage here is either a deck that never
    /// opens or one that flaps open and shut, so it is worth a check.
    [Test]
    [NonParallelizable]
    public void Wake_gate_waits_for_the_pointer_to_settle_and_only_fires_once_a_visit()
    {
        Assume.That(Win32.AnyMouseButtonDown, Is.False, "a mouse button is being held");

        var original = Settings.WakeDelayMs;
        try
        {
            Settings.WakeDelayMs = 0;
            var instant = new EdgeWake();
            Assert.Multiple(() =>
            {
                Assert.That(instant.Woke("display-1", inside: true), Is.True, "no wait set, so it opens on arrival");
                Assert.That(instant.Woke("display-1", inside: true), Is.False, "already open for this visit");
                Assert.That(instant.Woke("display-1", inside: false), Is.False, "the pointer left");
                Assert.That(instant.Woke("display-1", inside: true), Is.True, "and came back");
            });

            Settings.WakeDelayMs = 300;
            var waits = new EdgeWake();
            Assert.That(waits.Woke("display-1", inside: true), Is.False, "the wait has not passed");
            Thread.Sleep(350);
            Assert.That(waits.Woke("display-1", inside: true), Is.True, "the pointer rested long enough");

            // Leaving restarts the clock rather than carrying the old arrival forward.
            Assert.That(waits.Woke("display-1", inside: false), Is.False);
            Assert.That(waits.Woke("display-1", inside: true), Is.False, "back at the edge, waiting again");
        }
        finally
        {
            Settings.WakeDelayMs = original;
            Settings.Flush();
        }
    }

    /// The rule that keeps the wake zone off the scrollbar: pinned to the screen edge
    /// on an outer edge, the full band on one the pointer can cross.
    [Test]
    [NonParallelizable]
    public void Wake_band_is_pinned_to_the_edge_only_where_the_pointer_can_stop()
    {
        var original = Settings.WakeAtScreenEdge;
        try
        {
            var screen = new ScreenInfo(IntPtr.Zero, "display-1",
                new Win32.RECT { Left = 0, Top = 0, Right = 1920, Bottom = 1080 },
                new Win32.RECT { Left = 0, Top = 0, Right = 1920, Bottom = 1040 },
                Scale: 1.25);

            Settings.WakeAtScreenEdge = false;
            Assert.That(EdgeWake.WakeBandWidth(screen, onRight: true, 14), Is.EqualTo(18),
                        "off, the band is the preference scaled to device pixels");

            Settings.WakeAtScreenEdge = true;
            Assert.That(EdgeWake.WakeBandWidth(screen, onRight: true, 14),
                        Is.EqualTo(EdgeWake.PinnedPixels), "an outer edge pins to the edge");

            var shared = screen with { NeighbourRight = true };
            Assert.Multiple(() =>
            {
                Assert.That(EdgeWake.WakeBandWidth(shared, onRight: true, 14), Is.EqualTo(18),
                            "the pointer crosses a shared edge, so it keeps the full band");
                Assert.That(EdgeWake.WakeBandWidth(shared, onRight: false, 14),
                            Is.EqualTo(EdgeWake.PinnedPixels), "its other edge is still outer");
            });
        }
        finally
        {
            Settings.WakeAtScreenEdge = original;
            Settings.Flush();
        }
    }
}
