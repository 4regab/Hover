namespace Hover.Voice;

/// <summary>What the card is doing: recording (the voice's level swells the orb), or working on what was said.</summary>
public enum AuraMode { Listening, Working }

/// <summary>
/// Voice's aura, a Siri-style orb on the listening and working cards in place of their words
/// (internal/ui/aura.go). It is drawn on the CPU into a small premultiplied BGRA picture (66 DIPs,
/// about 17,000 pixels at 2x).
///
/// The orb is a dark glass ball with soft blobs of light swirling inside it, a bright rim and a
/// faint glow round it. The blobs' colours are made from the one picked in Settings → Voice (it, the
/// hues either side of it and one further round; a pale tint of it lights the glass). Listening, it
/// swirls and the voice swells it; working on what was said, it swirls slowly and breathes.
///
/// The voice is read against the room: the quietest level heard is taken as the room's noise and
/// the loudest lately as full voice, so speech uses the whole range whatever the microphone's gain.
/// </summary>
public sealed class Aura
{
    const int Blobs = 4;

    readonly record struct Look(float Speed, float Breath, float Period, float Bright);

    static Look LookOf(AuraMode m) => m == AuraMode.Listening ? new(1.1f, 0.025f, 2.4f, 1.0f) : new(0.55f, 0.05f, 2.0f, 0.85f);

    float lastT;
    bool hasLast;
    float phase, speed, breath, period, bright, level;
    // The room's noise and the loudest lately, in the microphone's own 0..1, once heard.
    float floor, peak;
    bool heard, init;

    void Setup()
    {
        if (init) return;
        var l = LookOf(AuraMode.Listening);
        (speed, breath, period, bright) = (l.Speed, l.Breath, l.Period, l.Bright);
        init = true;
    }

    /// <summary>Forgets the last frame: the next one starts at its state's look, not eased into it.</summary>
    public void Reset() => (hasLast, heard) = (false, false);

    static float Clamp(float v, float lo, float hi) => Math.Max(lo, Math.Min(hi, v));

    static void EaseTo(ref float v, float to, float k) => v += (to - v) * k;

    /// <summary>The colour (0..1) with its hue turned by <paramref name="deg"/> degrees, keeping its brightness and saturation.</summary>
    static float[] TurnHue(float[] rgb, float deg)
    {
        var mx = Math.Max(rgb[0], Math.Max(rgb[1], rgb[2]));
        var mn = Math.Min(rgb[0], Math.Min(rgb[1], rgb[2]));
        var c = mx - mn;
        // Grey has no hue to turn: white stays white.
        if (c < 1e-4f) return rgb;
        static double Rem(double a, double b) => a - b * Math.Floor(a / b);
        double h;
        if (mx == rgb[0]) h = Rem((rgb[1] - rgb[2]) / c, 6);
        else if (mx == rgb[1]) h = (rgb[2] - rgb[0]) / c + 2;
        else h = (rgb[0] - rgb[1]) / c + 4;
        h = Rem(h + deg / 60.0, 6);
        var x = c * (float)(1 - Math.Abs(h % 2 - 1));
        float r, g, b;
        switch ((int)h)
        {
            case 0: (r, g, b) = (c, x, 0); break;
            case 1: (r, g, b) = (x, c, 0); break;
            case 2: (r, g, b) = (0, c, x); break;
            case 3: (r, g, b) = (0, x, c); break;
            case 4: (r, g, b) = (x, 0, c); break;
            default: (r, g, b) = (c, 0, x); break;
        }
        return [r + mn, g + mn, b + mn];
    }

    /// <summary>The microphone's level against the room: 0 at the room's noise, 1 at the loudest lately.</summary>
    float AgainstRoom(float raw, float dt)
    {
        if (!heard)
        {
            // Started mid-sentence, the room is taken as no louder than a quiet one.
            floor = Math.Min(raw, 0.3f);
            peak = Math.Min(raw, 0.3f) + 0.3f;
            heard = true;
        }
        dt = Clamp(dt, 0, 0.5f);
        if (raw < floor) floor = raw;
        else floor += (raw - floor) * (1 - (float)Math.Exp(-dt / 12));
        if (raw > peak) peak = raw;
        else peak += (floor + 0.3f - peak) * (1 - (float)Math.Exp(-dt / 3));
        // The gap is never under 0.2 (-12 dB), so a silent room does not blow its hiss up.
        var v = Clamp((raw - floor) / Math.Max(peak - floor, 0.2f), 0, 1);
        // Quiet speech counts for more than loud: the curve lifts the low end.
        return (float)Math.Pow(v, 0.7);
    }

    /// <summary>
    /// One frame at <paramref name="t"/> seconds (the notch's clock), <paramref name="px"/> pixels square, in
    /// <paramref name="rgb"/>. <paramref name="level"/> is the microphone's (0 to 1, listening only).
    /// <paramref name="snap"/>: no easing (animations off, pictures). Returns premultiplied BGRA.
    /// </summary>
    public byte[] Frame(AuraMode mode, float t, float level, int px, bool snap, (byte R, byte G, byte B) rgb)
    {
        Setup();
        var dt = -1f;
        if (hasLast) dt = t - lastT;
        (lastT, hasLast) = (t, true);
        var to = LookOf(mode);
        var lv = 0f;
        if (mode == AuraMode.Listening) lv = AgainstRoom(Clamp(level, 0, 1), Math.Max(dt, 0));
        if (snap || !(dt >= 0 && dt <= 0.5f))
        {
            (speed, breath, period, bright, this.level) = (to.Speed, to.Breath, to.Period, to.Bright, lv);
            phase = t * speed;
        }
        else
        {
            var k = 1 - (float)Math.Exp(-dt / 0.15);
            EaseTo(ref speed, to.Speed, k);
            EaseTo(ref breath, to.Breath, k);
            EaseTo(ref period, to.Period, k);
            EaseTo(ref bright, to.Bright, k);
            // The voice comes up quickly and falls away more slowly, as a meter does.
            var tau = lv > this.level ? 0.04f : 0.25f;
            EaseTo(ref this.level, lv, 1 - (float)Math.Exp(-dt / tau));
            // The voice swirls it faster too.
            phase += dt * speed * (1 + 1.6f * this.level);
        }
        var b = (float)Math.Sin(t / period * 2 * Math.PI);
        return Draw(Math.Max(px, 8), b, [rgb.R / 255f, rgb.G / 255f, rgb.B / 255f]);
    }

    struct Blob
    {
        public float Cx, Cy, Ax, Ay, Long, Short;
        public float[] Col;
        public float Strength;
    }

    // breath is -1..1, where the orb is in its breathing.
    byte[] Draw(int n, float breathNow, float[] color)
    {
        float ph = phase, lv = level;
        static float Sin(float v) => (float)Math.Sin(v);
        static float Cos(float v) => (float)Math.Cos(v);
        static float Exp(float v) => (float)Math.Exp(v);
        // The orb's radius, in the picture's 0.5: the voice swells it, the breath moves it.
        var radius = 0.34f * (1 + 0.13f * lv) * (1 + breath * breathNow);
        // It brightens as it breathes in, and with the voice.
        var bri = bright * (1 + 0.12f * breathNow * breath / 0.05f) * (1 + 0.5f * lv);
        // The family of colours: the picked one, its neighbours either side and one further round
        // for the blobs, and a pale tint for the glass.
        var pale = new float[3];
        for (var i = 0; i < 3; i++) pale[i] = color[i] * 0.45f + 0.55f;
        float[][] cols = [color, TurnHue(color, 38), TurnHue(color, -38), TurnHue(color, 80)];
        // Each blob: centre, its long axis (along the way it moves), its two sizes, its colour and
        // strength. They circle the middle at their own rates, nearer and further in turn, and
        // spread out as the voice rises.
        var blobs = new Blob[Blobs];
        for (var i = 0; i < Blobs; i++)
        {
            float k = i;
            var way = 1f;
            if (i % 2 != 0) way = -0.8f;
            var ang = ph * way * (0.8f + 0.23f * k) + k * 1.7f + 0.3f * k * k;
            var r = radius * (0.44f + 0.14f * Sin(ph * (0.6f + 0.17f * k) + 1.9f * k)) * (1 + 0.35f * lv);
            var lg = radius * (0.58f + 0.10f * Sin(ph * 0.9f + k));
            var sh = radius * (0.17f + 0.04f * Cos(ph * 1.3f + 2 * k));
            var st = 0.9f;
            if (i == 3) st = 0.6f;
            blobs[i] = new Blob { Cx = Cos(ang) * r, Cy = Sin(ang) * r, Ax = -Sin(ang) * way, Ay = Cos(ang) * way, Long = lg, Short = sh, Col = cols[i], Strength = st };
        }
        // The middle turns a little against the rim, which twists the blobs into ribbons.
        var swirl = 1.1f * Sin(ph * 0.37f) + 0.6f * lv;
        var pixels = new byte[n * n * 4];
        // One pixel, in the picture's units: the rim's edge is smoothed over it.
        var pix = 1f / n;
        static byte Q8(float f) => (byte)Clamp((float)Math.Round(f * 255), 0, 255);
        var v = new float[3];
        var o = new float[3];
        for (var y = 0; y < n; y++)
        {
            for (var x = 0; x < n; x++)
            {
                float dx = (x + 0.5f) / n - 0.5f, dy = (y + 0.5f) / n - 0.5f;
                var rho = (float)Math.Sqrt(dx * dx + dy * dy);
                var q = rho / radius;
                // Outside the orb only its glow shows, fading out before the picture's edge.
                var edgeFade = Clamp((0.5f - rho) / 0.1f, 0, 1);
                var over = Math.Max(q - 1, 0) / 0.3f;
                var halo = 0.22f * bri * Exp(-over * over) * edgeFade * edgeFade;
                var inside = Clamp((radius - rho) / pix + 0.5f, 0, 1);
                v[0] = v[1] = v[2] = 0;
                if (inside > 0)
                {
                    var om = Math.Max(1 - q, 0);
                    var s = swirl * om * om;
                    float sn = Sin(s), cs = Cos(s);
                    float rx = dx * cs - dy * sn, ry = dx * sn + dy * cs;
                    foreach (var bl in blobs)
                    {
                        float ox = rx - bl.Cx, oy = ry - bl.Cy;
                        float u = (ox * bl.Ax + oy * bl.Ay) / bl.Long, w = (-ox * bl.Ay + oy * bl.Ax) / bl.Short;
                        var g = bl.Strength * Exp(-(u * u + w * w));
                        for (var c = 0; c < 3; c++) v[c] += g * bl.Col[c];
                    }
                    // The glass: a rim in the pale tint that brightens toward the edge, and a soft
                    // white highlight up and to the left.
                    var rim = 0.45f * (float)Math.Pow(q, 12);
                    float hx = dx / radius + 0.36f, hy = dy / radius + 0.42f;
                    var shine = 0.16f * Exp(-(hx * hx / 0.07f + hy * hy / 0.035f));
                    for (var c = 0; c < 3; c++) v[c] = (v[c] * 1.25f + 0.07f * color[c] + rim * pale[c] + shine) * bri * inside;
                }
                // Light adds up to white where it is brightest, the colour round it.
                for (var c = 0; c < 3; c++) o[c] = 1 - Exp(-(v[c] + halo * (1 - inside) * color[c] * 1.4f));
                // The ball itself is dark glass over whatever is behind (the notch is black).
                var al = Math.Max(Math.Max(o[0], o[1]), Math.Max(o[2], 0.92f * inside));
                var i = (y * n + x) * 4;
                // BGRA, premultiplied: the colour is never above the alpha.
                pixels[i] = Q8(o[2]);
                pixels[i + 1] = Q8(o[1]);
                pixels[i + 2] = Q8(o[0]);
                pixels[i + 3] = Q8(al);
            }
        }
        return pixels;
    }
}
