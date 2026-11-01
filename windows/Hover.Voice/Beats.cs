using System.Reflection;
using Hover.Host;
using NVorbis;

namespace Hover.Voice;

/// <summary>
/// The office's chill beats: a CC0 lofi loop (the Go app's own <c>office-beats.ogg</c>), off until
/// switched on, faded in to 0.32 and out, and silent while no office is in view. It is decoded as
/// it plays (Ogg Vorbis) into the system's output, and the device is let go while silent, so a
/// quiet Hover holds no audio stream. A port of <c>internal/music</c>.
/// </summary>
public sealed class Beats : IDisposable
{
    /// <summary>The loudest the beats get.</summary>
    public const double Full = 0.32;

    /// <summary>
    /// The page's ramp, one step per animation frame: up by 0.02, down by 0.03, within 0 and 0.32,
    /// snapped to the target once within 0.02. Returns the new volume and whether the ramp arrived.
    /// </summary>
    public static (double Volume, bool Arrived) RampStep(double volume, double to)
    {
        var d = to > volume ? 0.02 : -0.03;
        var v = Math.Max(0, Math.Min(Full, volume + d));
        if (Math.Abs(v - to) > 0.02) return (v, false);
        return (to, true);
    }

    readonly object gate = new();
    readonly AudioOpener open;
    readonly Func<Stream> loop;
    bool want, seen = true;
    double ramp;
    IAudioOutput? output;
    long volumeBits;

    /// <param name="want">Whether the music is switched on (office.json's <c>beats</c>).</param>
    /// <param name="open">The sound output; <see cref="AudioOutput.Open"/> in the app.</param>
    /// <param name="loop">The loop's Ogg bytes; <see cref="LoopFile"/> in the app.</param>
    public Beats(bool want, AudioOpener open, Func<Stream> loop)
    {
        this.want = want;
        this.open = open;
        this.loop = loop;
    }

    /// <summary>The loop the app ships: <c>internal/music/office-beats.ogg</c>, linked into this assembly.</summary>
    public static Stream LoopFile() => Assembly.GetExecutingAssembly().GetManifestResourceStream("office-beats.ogg")
        ?? throw new FileNotFoundException("office-beats.ogg is not in the assembly.");

    /// <summary>The button, remembered by the caller.</summary>
    public bool Want { get { lock (gate) return want; } }

    /// <summary>An office is in view.</summary>
    public bool Seen { get { lock (gate) return seen; } }

    /// <summary>The ramp's own volume.</summary>
    public double Volume { get { lock (gate) return ramp; } }

    /// <summary>Whether a sound stream is open now (it is only while the beats are on and in view, or fading out).</summary>
    public bool Playing { get { lock (gate) return output is not null; } }

    /// <summary>What should sound: full volume while wanted and seen.</summary>
    public double Target { get { lock (gate) return TargetLocked(); } }

    double TargetLocked() => want && seen ? Full : 0;

    /// <summary>The button: on or off. False when no output could be opened (the button goes back off).</summary>
    public bool Toggle(bool on)
    {
        lock (gate) want = on;
        return Sync();
    }

    /// <summary>An office came into view or went.</summary>
    public void Follow(bool inView)
    {
        lock (gate) seen = inView;
        Sync();
    }

    bool Sync()
    {
        lock (gate)
        {
            if (TargetLocked() <= 0 || output is not null) return true;
            string? error;
            try
            {
                var decoder = new LoopDecoder(loop());
                var opened = open(Player(decoder));
                error = opened.Error;
                output = opened.Output;
                if (output is null) decoder.Dispose();
            }
            catch (Exception e)
            {
                error = e.Message;
            }
            if (output is null)
            {
                HostLog.Line($"beats: no audio output — {error}");
                want = false;
                return false;
            }
            return true;
        }
    }

    /// <summary>The stream's fill: the loop at the ramp's volume, resampled to the output's rate by the nearest sample, which is enough for a quiet loop in the background.</summary>
    Action<float[]> Player(LoopDecoder decoder)
    {
        double pos = 0;
        var step = (double)decoder.Rate / AudioOutput.Rate;
        return fill =>
        {
            var vol = (float)BitConverter.Int64BitsToDouble(Interlocked.Read(ref volumeBits));
            for (var i = 0; i + 1 < fill.Length; i += AudioOutput.Channels)
            {
                var (l, r) = decoder.At((int)pos);
                pos += step;
                if ((int)pos >= decoder.Buffered) pos -= decoder.Consume((int)pos);
                fill[i] = l * vol;
                fill[i + 1] = r * vol;
            }
        };
    }

    /// <summary>
    /// One animation frame of the fade (about 60 a second). True while it still moves, so the caller
    /// keeps its timer only as long as that.
    /// </summary>
    public bool Frame()
    {
        IAudioOutput? closing = null;
        bool moving;
        lock (gate)
        {
            var to = TargetLocked();
            if (ramp == to) return false;
            var (v, arrived) = RampStep(ramp, to);
            ramp = v;
            Interlocked.Exchange(ref volumeBits, BitConverter.DoubleToInt64Bits(v));
            if (arrived && to == 0)
            {
                // The stream and the device go.
                closing = output;
                output = null;
            }
            moving = !arrived;
        }
        closing?.Dispose();
        return moving;
    }

    public void Dispose()
    {
        IAudioOutput? closing;
        lock (gate)
        {
            closing = output;
            output = null;
            want = false;
        }
        closing?.Dispose();
    }
}

/// <summary>The loop, decoded a block at a time and started again at its end.</summary>
sealed class LoopDecoder : IDisposable
{
    readonly Stream file;
    VorbisReader reader;
    // The decoded frames not yet played, as interleaved stereo floats in data[start..end].
    float[] data = new float[1 << 16];
    int start, end;
    readonly float[] block = new float[4096];

    public LoopDecoder(Stream source)
    {
        var copy = new MemoryStream();
        using (source) source.CopyTo(copy);
        file = copy;
        file.Position = 0;
        reader = new VorbisReader(file, false);
        Rate = reader.SampleRate;
    }

    /// <summary>The loop's own sample rate.</summary>
    public int Rate { get; }

    /// <summary>Frames decoded and not yet played.</summary>
    public int Buffered => (end - start) / 2;

    void Append(float l, float r)
    {
        if (end + 2 > data.Length)
        {
            // Make room by moving what is left to the front; grow only when that is not enough.
            var left = end - start;
            if (start > 0 && left + 2 <= data.Length / 2) Array.Copy(data, start, data, 0, left);
            else
            {
                var bigger = new float[data.Length * 2];
                Array.Copy(data, start, bigger, 0, left);
                data = bigger;
            }
            start = 0;
            end = left;
        }
        data[end++] = l;
        data[end++] = r;
    }

    void Fill(int upTo)
    {
        var empty = 0;
        while (Buffered <= upTo)
        {
            var n = reader.ReadSamples(block, 0, block.Length);
            if (n > 0)
            {
                empty = 0;
                var ch = reader.Channels;
                for (var i = 0; i + ch <= n; i += ch) Append(block[i], ch > 1 ? block[i + 1] : block[i]);
                continue;
            }
            // The end (or a broken page): from the top again. Twice with nothing in between means
            // nothing will come: silence, rather than a spin.
            empty++;
            if (empty >= 2)
            {
                while (Buffered <= upTo) Append(0, 0);
                return;
            }
            try
            {
                reader.Dispose();
                file.Position = 0;
                reader = new VorbisReader(file, false);
            }
            catch
            {
                while (Buffered <= upTo) Append(0, 0);
                return;
            }
        }
    }

    public (float L, float R) At(int i)
    {
        Fill(i);
        var at = start + 2 * i;
        return (data[at], data[at + 1]);
    }

    /// <summary>Drops what has been played; how many frames went.</summary>
    public int Consume(int played)
    {
        var n = Math.Min(played, Buffered);
        start += n * 2;
        return n;
    }

    public void Dispose()
    {
        reader.Dispose();
        file.Dispose();
    }
}

/// <summary>A short two-note chime on the default output (voice's screenshot). Nothing when there is no output.</summary>
public static class Chime
{
    /// <summary>The fill of the chime: 880 Hz then 1320 Hz, 90 ms each, each fading out; quiet, under the music.</summary>
    public static Action<float[]> Fill()
    {
        var n = 0;
        return buf =>
        {
            for (var i = 0; i + 1 < buf.Length; i += AudioOutput.Channels)
            {
                var t = (double)n / AudioOutput.Rate;
                n++;
                double f = 880, k = t / 0.09;
                if (t >= 0.09) { f = 1320; k = (t - 0.09) / 0.09; }
                var v = 0f;
                if (t < 0.18) v = (float)(Math.Sin(t * f * 2 * Math.PI) * 0.12 * (1 - k));
                buf[i] = v;
                buf[i + 1] = v;
            }
        };
    }

    /// <summary>Plays it on its own task; the device is let go when it ends.</summary>
    public static void Play(AudioOpener? open = null)
    {
        open ??= AudioOutput.Open;
        _ = Task.Run(async () =>
        {
            var opened = open(Fill());
            if (opened.Output is null)
            {
                HostLog.Line($"chime: {opened.Error}");
                return;
            }
            await Task.Delay(260).ConfigureAwait(false);
            opened.Output.Dispose();
        });
    }
}
