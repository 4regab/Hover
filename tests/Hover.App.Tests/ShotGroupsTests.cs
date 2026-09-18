using Hover.Images;
using NUnit.Framework;

namespace Hover.Tests;

/// The tray groups pictures by day. The headings have to stay unambiguous, which is
/// the only reason a named weekday stops being used after a week.
public sealed class ShotGroupsTests
{
    private static readonly DateTime Today = new(2026, 9, 18, 14, 0, 0);  // a Friday

    private static Shot At(DateTime taken) => new(@"C:\nowhere\x.png", taken);

    [Test]
    public void Today_and_yesterday_are_named_rather_than_dated()
    {
        Assert.Multiple(() =>
        {
            Assert.That(ShotGroups.LabelFor(Today, Today), Is.EqualTo("Today"));
            Assert.That(ShotGroups.LabelFor(Today.AddDays(-1), Today), Is.EqualTo("Yesterday"));
        });
    }

    [Test]
    public void Days_inside_the_last_week_use_the_weekday_name()
    {
        Assert.That(ShotGroups.LabelFor(Today.AddDays(-3), Today), Is.EqualTo("Tuesday"));
    }

    [Test]
    public void A_week_back_switches_to_a_date_so_the_weekday_cannot_be_mistaken()
    {
        var label = ShotGroups.LabelFor(Today.AddDays(-7), Today);
        Assert.Multiple(() =>
        {
            Assert.That(label, Does.Not.Contain("Friday"));
            Assert.That(label, Does.Contain("September"));
        });
    }

    [Test]
    public void A_date_in_the_future_gets_a_date_rather_than_a_silly_heading()
    {
        // The clock can move backwards, so a file can be newer than "now".
        var label = ShotGroups.LabelFor(Today.AddDays(2), Today);
        Assert.That(label, Does.Contain("September"));
    }

    [Test]
    public void Newest_day_comes_first_and_the_order_inside_a_day_is_kept()
    {
        var shots = new[]
        {
            At(Today.AddHours(-1)),          // today, later
            At(Today.AddHours(-5)),          // today, earlier
            At(Today.AddDays(-1)),           // yesterday
            At(Today.AddDays(-4)),           // Monday
        };

        var groups = ShotGroups.Build(shots, Today);

        Assert.Multiple(() =>
        {
            Assert.That(groups.Select(g => g.Label),
                Is.EqualTo(new[] { "Today", "Yesterday", "Monday" }));
            Assert.That(groups[0].Items, Has.Count.EqualTo(2));
            Assert.That(groups[0].Items[0].Shot.Taken, Is.GreaterThan(groups[0].Items[1].Shot.Taken));
        });
    }

    [Test]
    public void A_time_alone_is_only_shown_for_today()
    {
        var todayItem = new ShotItem(At(new DateTime(2026, 9, 18, 18, 44, 0)), Today);
        var olderItem = new ShotItem(At(new DateTime(2026, 9, 17, 21, 30, 0)), Today);

        Assert.Multiple(() =>
        {
            Assert.That(todayItem.When, Is.EqualTo("18:44"));
            Assert.That(olderItem.When, Is.EqualTo("Thu 21:30"));
        });
    }

    [Test]
    public void A_missing_file_reports_no_size_instead_of_throwing()
    {
        var item = new ShotItem(At(Today), Today);
        Assert.That(item.Size, Is.Empty);
    }

    [Test]
    public void An_empty_tray_produces_no_groups()
    {
        Assert.That(ShotGroups.Build(Array.Empty<Shot>(), Today), Is.Empty);
    }
}
