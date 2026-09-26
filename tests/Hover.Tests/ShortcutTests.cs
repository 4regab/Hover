using System.Windows.Input;
using Hover.Core;
using NUnit.Framework;

namespace Hover.Tests;

public sealed class ShortcutTests
{
    [TestCase(ModifierKeys.Control | ModifierKeys.Shift, Key.Back, "Ctrl+Shift+Backspace")]
    [TestCase(ModifierKeys.Control, Key.OemPeriod, "Ctrl+.")]
    [TestCase(ModifierKeys.None, Key.Escape, "Esc")]
    [TestCase(ModifierKeys.None, Key.None, "—")]
    public void Shortcut_formats_for_the_settings_UI(ModifierKeys modifiers, Key key, string expected) =>
        Assert.That(new Shortcut(modifiers, key).ToString(), Is.EqualTo(expected));

    [Test]
    public void Shortcut_equality_uses_key_and_modifiers()
    {
        var shortcut = new Shortcut(ModifierKeys.Control, Key.F);

        Assert.Multiple(() =>
        {
            Assert.That(shortcut, Is.EqualTo(new Shortcut(ModifierKeys.Control, Key.F)));
            Assert.That(shortcut, Is.Not.EqualTo(new Shortcut(ModifierKeys.Alt, Key.F)));
            Assert.That(shortcut.GetHashCode(), Is.EqualTo(new Shortcut(ModifierKeys.Control, Key.F).GetHashCode()));
        });
    }
}
