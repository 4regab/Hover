namespace HoverAvalonia.Office;

/// <summary>The office's small textures. The light beam, light patch and glow sprite are ports of main.js's gradients (premultiplied, as Chromium keeps a canvas).
/// The sky / TV / board / clock walls are APPROXIMATIONS: Hover draws them with text and charts through a canvas (canvas.rs, swash); the prototype
/// draws flat shapes only, so these four will visibly differ from Hover's. Not ported: text on the TV, the board's session list, the LED clock digits.</summary>
public static class Textures
{
    public static readonly (int w, int h)[] Sizes = [(128, 96), (208, 118), (480, 280), (96, 44), (4, 64), (64, 64), (64, 64), (1, 1)];
    public static readonly byte[][] Data = new byte[8][];

    static void Put(OfficeScene s, int i, byte[] d) { Data[i] = d; s.TexDirty[i] = true; }
    static void Px(byte[] d, int w, int x, int y, double r, double g, double b, double a = 1)
    {
        if (x < 0 || y < 0 || x >= w || y * w * 4 + x * 4 + 3 >= d.Length) return;
        int o = (y * w + x) * 4; d[o] = (byte)(Math.Clamp(r * a, 0, 1) * 255 + .5); d[o + 1] = (byte)(Math.Clamp(g * a, 0, 1) * 255 + .5); d[o + 2] = (byte)(Math.Clamp(b * a, 0, 1) * 255 + .5); d[o + 3] = (byte)(a * 255 + .5);
    }
    static void Rect(byte[] d, int w, int x0, int y0, int rw, int rh, double r, double g, double b, double a = 1) { for (int y = y0; y < y0 + rh; y++) for (int x = x0; x < x0 + rw; x++) Px(d, w, x, y, r, g, b, a); }

    public static void Reset(OfficeScene s)
    {
        // glowTex: white, alpha 1 at the centre, .4 at 35 %, 0 at the edge.
        var glow = new byte[64 * 64 * 4];
        for (int y = 0; y < 64; y++) for (int x = 0; x < 64; x++)
        {
            double d = Math.Sqrt((x + .5 - 32) * (x + .5 - 32) + (y + .5 - 32) * (y + .5 - 32)) / 32, a = d >= 1 ? 0 : d < .35 ? 1 - .6 * d / .35 : .4 * (1 - (d - .35) / .65);
            Px(glow, 64, x, y, 1, 1, 1, a);
        }
        Put(s, RoomBuilder.TexGlow, glow);
        Put(s, RoomBuilder.TexWhite, [255, 255, 255, 255]);
        // beamTex: vertical white gradient, alpha .9 -> 0.
        var beam = new byte[4 * 64 * 4];
        for (int y = 0; y < 64; y++) for (int x = 0; x < 4; x++) Px(beam, 4, x, y, 1, 1, 1, .9 * (1 - y / 63.0));
        Put(s, RoomBuilder.TexBeam, beam);
        // patchTex: four soft squares (the Rust one blurs by 2 px).
        var patch = new byte[64 * 64 * 4];
        foreach (var (px, py) in new[] { (4, 4), (34, 4), (4, 34), (34, 34) })
            for (int y = py - 3; y < py + 29; y++) for (int x = px - 3; x < px + 29; x++)
            {
                double ex = Math.Min(x - (px - 2), (px + 28) - x), ey = Math.Min(y - (py - 2), (py + 28) - y), a = Math.Clamp(Math.Min(ex, ey) / 4.0, 0, 1);
                if (a > 0) Px(patch, 64, x, y, 1, 1, 1, a);
            }
        Put(s, RoomBuilder.TexPatch, patch);
        Board(s); Clock(s);
    }

    public static void Sky(OfficeScene s)
    {
        var d = new byte[128 * 96 * 4]; bool day = s.Time == TimeOfDay.Day;
        for (int y = 0; y < 96; y++)
        {
            double t = y / 95.0;
            double r = day ? .45 + .35 * t : .04 + .06 * t, g = day ? .68 + .2 * t : .05 + .08 * t, b = day ? .95 : .16 + .12 * t;
            for (int x = 0; x < 128; x++) Px(d, 128, x, y, Rgb2(r), Rgb2(g), Rgb2(b));
        }
        var rnd = new Random(5);
        if (!day) for (int i = 0; i < 26; i++) Px(d, 128, rnd.Next(128), rnd.Next(60), .9, .92, 1);
        else for (int i = 0; i < 3; i++) { int cx = 20 + i * 40, cy = 24 + (i % 2) * 14; Rect(d, 128, cx, cy, 26, 7, 1, 1, 1, .9); Rect(d, 128, cx + 5, cy - 4, 14, 5, 1, 1, 1, .9); }
        Put(s, RoomBuilder.TexSky, d);
    }
    static double Rgb2(double v) => v; // textures are sRGB-encoded 8-bit; the GL internal format decodes them

    public static void Tv(OfficeScene s, double t)
    {
        const int W = 208, H = 118; var d = new byte[W * H * 4];
        Rect(d, W, 0, 0, W, H, .05, .06, .09);
        // Overview approximation: one bar per session, in the bot's colour, length = activity.
        for (int i = 0; i < 6; i++)
        {
            var sess = s.Sessions.Count > i ? s.Sessions[i] : null;
            var c = sess == null ? (0.25, 0.27, 0.33) : Hex(Bot.Roster[i % Bot.Roster.Length].color);
            double len = sess == null ? 0.1 : sess.Sim == SimState.Working ? .5 + .4 * Math.Sin(t * 2 + i) * Math.Sin(t * 2 + i) : sess.Sim == SimState.Completed ? 1 : .25;
            Rect(d, W, 14, 14 + i * 16, (int)(180 * len), 9, c.Item1, c.Item2, c.Item3);
        }
        Put(s, RoomBuilder.TexTv, d);
    }
    static (double, double, double) Hex(uint h) => (((h >> 16) & 255) / 255.0, ((h >> 8) & 255) / 255.0, (h & 255) / 255.0);

    static void Board(OfficeScene s)
    {
        const int W = 480, H = 280; var d = new byte[W * H * 4];
        Rect(d, W, 0, 0, W, H, .09, .08, .11);
        Rect(d, W, 0, 0, W, 36, .15, .13, .19);
        for (int i = 0; i < 5; i++) Rect(d, W, 16, 54 + i * 44, 448, 32, .13, .12, .17);
        Put(s, RoomBuilder.TexBoard, d);
    }
    static void Clock(OfficeScene s)
    {
        const int W = 96, H = 44; var d = new byte[W * H * 4];
        Rect(d, W, 0, 0, W, H, .02, .02, .03);
        for (int i = 0; i < 4; i++) Rect(d, W, 10 + i * 20 + (i >= 2 ? 6 : 0), 8, 14, 28, 1, .48, .16, .9);
        Put(s, RoomBuilder.TexClock, d);
    }
    public static void Board(OfficeScene s, bool _) => Board(s);
}
