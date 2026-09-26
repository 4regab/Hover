using Hover.Core;
using NUnit.Framework;

namespace Hover.Tests;

[NonParallelizable]
public sealed class CryptoTests
{
    [Test]
    public void Crypto_round_trips_unicode_and_uses_a_fresh_nonce()
    {
        const string text = "Привет 👋\nsecret note";

        var first = Crypto.Seal(text);
        var second = Crypto.Seal(text);

        Assert.Multiple(() =>
        {
            Assert.That(Crypto.Open(first), Is.EqualTo(text));
            Assert.That(Crypto.Open(second), Is.EqualTo(text));
            Assert.That(second, Is.Not.EqualTo(first));
            Assert.That(first, Is.Not.EqualTo(System.Text.Encoding.UTF8.GetBytes(text)));
        });
    }

    [Test]
    public void Crypto_rejects_missing_short_and_tampered_payloads()
    {
        var tampered = Crypto.Seal("do not alter");
        tampered[^1] ^= 0x01;

        Assert.Multiple(() =>
        {
            Assert.That(Crypto.Open(null), Is.Empty);
            Assert.That(Crypto.Open(new byte[8]), Is.Empty);
            Assert.That(Crypto.Open(tampered), Is.Empty);
        });
    }
}
