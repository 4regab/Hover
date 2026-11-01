using System.Runtime.InteropServices;
using Hover.Host;
using Hover.Host.Protocol;
using Hover.Notch;
using Vortice.Direct2D1;
using Vortice.DXGI;
using Vortice.Mathematics;
using AlphaMode = Vortice.DCommon.AlphaMode;
using PixelFormat = Vortice.DCommon.PixelFormat;
using Size = Hover.Notch.Size;

namespace Hover.Windowing;

/// <summary>
/// <c>--shots</c> for the notch: its states drawn by the same renderer the window uses, onto a
/// bitmap that is read back (no screen capture), laid over a desktop colour because the notch is
/// see-through, and written as JPEG. The names follow the Go app's pictures (notch-rest-pill-2x
/// and so on) so the two can be put side by side.
/// </summary>
static class NotchShots
{
    /// <summary>The desktop colour the Go pictures lay the notch over.</summary>
    const uint Desk = 0x3a4a5e;

    static readonly Size Open = new(1120, 440);
    static readonly double WindowWidth = Open.W + 2 * Geometry.Pad;

    sealed record Shot(string Name, double Scale, int Height, Island Island, CardData? Card = null, double T = 1.3, double Openness = 0, uint Glow = 0);

    static SessionInfo Session(int id, string tool, string stage, string verb, string obj = "", AskInfo? ask = null, double? took = null) => new()
    {
        Id = id, Tool = tool, Stage = stage, Ask = ask, Activity = new ActivityInfo { Verb = verb, Obj = obj },
        Turns = [new TurnInfo { T0 = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds(), Took = took }],
    };

    // The quotas of the Go pictures: Claude 38 (green), Kiro 82 (amber), Codex with no reading, Cursor 95 (red).
    static readonly Dictionary<string, QuotaValue> Readings = new()
    {
        ["claude"] = new QuotaValue { Ok = true, Used = 38 },
        ["kiro"] = new QuotaValue { Ok = true, Used = 82 },
        ["cursor"] = new QuotaValue { Ok = true, Used = 95 },
    };

    static Island Make(IReadOnlyList<SessionInfo> sessions, Unseen? unseen = null, bool card = false, string[]? on = null,
        Dictionary<string, QuotaValue>? readings = null, int speaker = 0)
    {
        on ??= Rest.QuotaIds;
        readings ??= Readings;
        var now = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        return Rest.Make(id => on.Contains(id), id => readings.GetValueOrDefault(id), sessions, unseen, speaker, now, card);
    }

    static readonly AskInfo NpmInstall = new()
    {
        Id = "n1", Kind = "execute", Title = "Wants to run a command", Line = "Wants to run npm install",
        Command = "npm install three@0.171.0", Reason = "Installs packages or uses the network", Allow = "Run",
    };

    static CardData NpmCard => new("kiro", "Wants to run a command", "project · Tidy the imports again", "", "npm install three@0.171.0", "", [],
        "Installs packages or uses the network", false, "Run");

    static IEnumerable<Shot> Fixtures()
    {
        // Two agents at work: Kiro just waking up, Codex behind it.
        yield return new Shot("notch-rest-pill-2x", 2, 60, Make([Session(1, "kiro", "waking", "Waking up"), Session(2, "codex", "working", "Reading", "npm test")]), T: 1.3);
        // An end nobody has seen: the tool's mark with its badge and the task.
        yield return new Shot("notch-rest-done-2x", 2, 60, Make([], new Unseen(2, "codex", EndState.Completed, "Look for dead code", 187)));
        // Two agents at work, the second one Cursor.
        yield return new Shot("notch-rest-working-2x", 2, 60, Make([Session(1, "kiro", "waking", "Waking up"), Session(2, "cursor", "working", "Running", "npm test")]), T: 0.4);
        // What an agent does, in the verb and the object: Editing refresh.ts.
        yield return new Shot("notch-rest-activity-2x", 2, 60, Make([Session(1, "kiro", "working", "Editing", "refresh.ts"), Session(2, "cursor", "working", "Running", "npm test")]), T: 0.4);
        var asking = Session(1, "kiro", "waiting", "Reading", ask: NpmInstall);
        yield return new Shot("notch-rest-ask-2x", 2, 60, Make([asking]), T: 0.4, Glow: 0xffb340);
        yield return new Shot("notch-rest-card-2x", 2, 220, Make([asking], card: true), NpmCard, 0.4, Glow: 0xffb340);
        yield return new Shot("notch-open-office", 1, 480, Make([]), Openness: 1);

        // The nearby inputs: nothing to show, a quota with no reading and a failed one, a very long
        // text, several questions, a failed end, and a screen at 150 %.
        yield return new Shot("notch-rest-empty-2x", 2, 60, Make([], on: []));
        yield return new Shot("notch-rest-quotas-only-2x", 2, 60,
            Make([], readings: new() { ["codex"] = new QuotaValue { Ok = false, Detail = "not signed in" }, ["kiro"] = new QuotaValue { Ok = true, Used = 100 } }));
        yield return new Shot("notch-rest-long-text-2x", 2, 60,
            Make([Session(1, "claude", "working", "Editing", "src/components/dashboard/very-long-folder-name/another-long-folder/refresh-the-whole-account-settings-page.tsx")]), T: 0.7);
        var many = Session(1, "codex", "waiting", "Reading", ask: NpmInstall with { More = 2 });
        yield return new Shot("notch-rest-ask-many-2x", 2, 60, Make([many]), T: 0.4, Glow: 0xffb340);
        yield return new Shot("notch-rest-failed-2x", 2, 60, Make([], new Unseen(1, "kiro", EndState.Failed, "Update the changelog", 45)), Glow: 0xff453a);
        yield return new Shot("notch-rest-pill-150", 1.5, 60, Make([Session(1, "kiro", "working", "Thinking")]), T: 0.9);
    }

    public static async Task TakeAsync(string directory)
    {
        Directory.CreateDirectory(directory);
        try
        {
            using var gfx = Gfx.Create();
            using var renderer = new NotchRenderer(gfx);
            foreach (var shot in Fixtures())
            {
                var file = Path.Combine(directory, shot.Name + ".jpg");
                var (bgra, w, h) = Draw(gfx, renderer, shot);
                await Hover.Shots.WriteJpegAsync(bgra, w, h, file);
                HostLog.Line($"picture {file} ({w} x {h})");
            }
        }
        catch (Exception e)
        {
            HostLog.Line($"the notch pictures failed — {e}");
            throw;
        }
    }

    static NotchScene Scene(NotchRenderer renderer, Gfx gfx, Shot shot)
    {
        var scene = new NotchScene { Island = shot.Island, Card = shot.Card, Kind = (RestKind)(int)shot.Island.Kind, Glow = shot.Glow, T = shot.T, Dark = true, Panel = 0x000000 };
        // The resting size comes from what the island or the card measures.
        scene.RestH = Geometry.PillHeight;
        var rest = shot.Island.Kind switch
        {
            IslandKind.Pill => Geometry.RestSize(RestKind.Pill, renderer.PillWidth(scene, gfx.Context), 0),
            IslandKind.Card when shot.Card is not null => CardRest(renderer, scene, gfx),
            _ => new Size(0, 0),
        };
        var frame = Geometry.FrameAt(shot.Openness, rest, Open);
        var shape = new NotchShape(frame, (WindowWidth - frame.W) / 2, WindowWidth, shot.Openness, true, shot.Openness > 0.001, rest);
        scene.ApplyShape(shape, Open);
        return scene;
    }

    static Size CardRest(NotchRenderer renderer, NotchScene scene, Gfx gfx)
    {
        var (w, h) = renderer.CardSize(scene, gfx.Context);
        return Geometry.RestSize(RestKind.Card, w, h);
    }

    /// <summary>Draws the shot offscreen and returns opaque BGRA pixels over the desktop colour, cut to at most 1400 wide.</summary>
    static (byte[] Pixels, int W, int H) Draw(Gfx gfx, NotchRenderer renderer, Shot shot)
    {
        var scene = Scene(renderer, gfx, shot);
        int pw = (int)Math.Round(WindowWidth * shot.Scale), ph = (int)Math.Round(shot.Height * shot.Scale);
        var dc = gfx.Context;
        var format = new PixelFormat(Format.B8G8R8A8_UNorm, AlphaMode.Premultiplied);
        using var target = dc.CreateBitmap(new SizeI(pw, ph), IntPtr.Zero, 0, new BitmapProperties1(format, 96, 96, BitmapOptions.Target));
        dc.Target = target;
        dc.SetDpi((float)(96 * shot.Scale), (float)(96 * shot.Scale));
        dc.BeginDraw();
        try { renderer.Draw(dc, scene, WindowWidth); }
        finally
        {
            dc.EndDraw().CheckError();
            dc.Target = null;
        }

        // Read the pixels back through a bitmap the CPU may map.
        using var cpu = dc.CreateBitmap(new SizeI(pw, ph), IntPtr.Zero, 0, new BitmapProperties1(format, 96, 96, BitmapOptions.CpuRead | BitmapOptions.CannotDraw));
        cpu.CopyFromBitmap(target).CheckError();
        var mapped = cpu.Map(MapOptions.Read);
        var src = new byte[mapped.Pitch * ph];
        try { Marshal.Copy(mapped.Bits, src, 0, src.Length); }
        finally { cpu.Unmap(); }

        // Premultiplied colour over an opaque back is colour + back * (1 - alpha).
        int cw = Math.Min(pw, Hover.Shots.MaxWidth), x0 = (pw - cw) / 2;
        byte b0 = (byte)(Desk & 0xff), g0 = (byte)((Desk >> 8) & 0xff), r0 = (byte)((Desk >> 16) & 0xff);
        var output = new byte[cw * ph * 4];
        for (var y = 0; y < ph; y++)
        {
            for (var x = 0; x < cw; x++)
            {
                var i = y * mapped.Pitch + (x0 + x) * 4;
                var o = (y * cw + x) * 4;
                int a = src[i + 3];
                output[o] = (byte)Math.Min(src[i] + b0 * (255 - a) / 255, 255);
                output[o + 1] = (byte)Math.Min(src[i + 1] + g0 * (255 - a) / 255, 255);
                output[o + 2] = (byte)Math.Min(src[i + 2] + r0 * (255 - a) / 255, 255);
                output[o + 3] = 255;
            }
        }
        return (output, cw, ph);
    }
}
