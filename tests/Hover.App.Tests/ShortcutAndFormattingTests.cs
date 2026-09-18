using Avalonia.Input;
using Hover.Core;
using NUnit.Framework;

namespace Hover.Tests;

public sealed class ShortcutAndFormattingTests
{
    [TestCase(KeyModifiers.Control | KeyModifiers.Shift, Key.Back, "Ctrl+Shift+Backspace")]
    [TestCase(KeyModifiers.Control, Key.OemPeriod, "Ctrl+.")]
    [TestCase(KeyModifiers.None, Key.Escape, "Esc")]
    [TestCase(KeyModifiers.None, Key.None, "—")]
    public void Shortcut_formats_for_the_settings_UI(KeyModifiers modifiers, Key key, string expected) =>
        Assert.That(new Shortcut(modifiers, key).ToString(), Is.EqualTo(expected));

    [Test]
    public void Shortcut_equality_uses_key_and_modifiers()
    {
        var shortcut = new Shortcut(KeyModifiers.Control, Key.F);

        Assert.Multiple(() =>
        {
            Assert.That(shortcut, Is.EqualTo(new Shortcut(KeyModifiers.Control, Key.F)));
            Assert.That(shortcut, Is.Not.EqualTo(new Shortcut(KeyModifiers.Alt, Key.F)));
            Assert.That(shortcut.GetHashCode(), Is.EqualTo(new Shortcut(KeyModifiers.Control, Key.F).GetHashCode()));
        });
    }

    [Test]
    public void FileStamp_is_filename_safe() =>
        Assert.That(Fmt.FileStamp(new DateTime(2026, 9, 1, 14, 5, 9)), Is.EqualTo("2026-09-01-140509"));

    [Test]
    public void Ago_uses_relative_units_for_recent_dates()
    {
        Assert.Multiple(() =>
        {
            Assert.That(Fmt.Ago(DateTime.Now.AddSeconds(-10)), Is.EqualTo("just now"));
            Assert.That(Fmt.Ago(DateTime.Now.AddMinutes(-5)), Is.EqualTo("5 min ago"));
            Assert.That(Fmt.Ago(DateTime.Now.AddHours(-3)), Is.EqualTo("3 h ago"));
            Assert.That(Fmt.Ago(DateTime.Now.AddDays(-2)), Is.EqualTo("2 d ago"));
        });
    }
}
