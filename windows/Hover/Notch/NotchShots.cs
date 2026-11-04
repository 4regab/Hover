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

    sealed record Shot(string Name, double Scale, int Height, Island Island, CardData? Card = null, double T = 1.3, double Openness = 0, uint Glow = 0, Action<Hover.Chat.Ui.OfficeUi>? Panels = null);

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
        // A long command breaks at any character and the box grows with it; the path over a change is dim.
        var longCommand = "npm install three@0.171.0 --save-exact --prefix ./packages/renderer/very/long/path/that/has/no/spaces/at/all --registry https://registry.example.com/";
        yield return new Shot("notch-rest-card-long-2x", 2, 260, Make([asking], card: true), NpmCard with { Command = longCommand }, 0.4, Glow: 0xffb340);
        yield return new Shot("notch-rest-card-diff-2x", 2, 240, Make([asking], card: true), NpmCard with
        {
            Title = "Wants to edit a file", Command = "", Path = "src/components/LayoutManager.tsx", Reason = "Changes 2 lines", Allow = "Allow",
            Preview = [new PreviewLine("function layout(rows) {", 0), new PreviewLine("  return rows.map(old);", -1), new PreviewLine("  return rows.map(next);", 1)],
        }, 0.4, Glow: 0xffb340);
        yield return new Shot("notch-open-office", 1, 480, Make([]), Openness: 1, Panels: ui =>
        {
            ui.Accept(Hover.Chatting.ChatShots.State(Hover.Chatting.ChatShots.Busy(), Hover.Chatting.ChatShots.Finished()));
            ui.ChatView = false;
            ui.CloseChat();
        });
        var asking3 = Hover.Chatting.ChatShots.Session(3, "kiro", "Upgrade three.js to 0.171", "waiting", 2, Hover.Chatting.ChatShots.Turn("Upgrade three.js to 0.171", "", "waiting", 1)) with { Ask = Hover.Chatting.ChatShots.Permission() };
        yield return new Shot("notch-open-ask", 1, 480, Make([]), Openness: 1, Panels: ui =>
        {
            ui.Accept(Hover.Chatting.ChatShots.State(Hover.Chatting.ChatShots.Busy(), Hover.Chatting.ChatShots.Finished(), asking3));
            ui.ChatView = false;
            ui.CloseChat();
        });
        yield return new Shot("notch-open-chat", 1, 480, Make([]), Openness: 1, Panels: ui =>
        {
            ui.Accept(Hover.Chatting.ChatShots.State(Hover.Chatting.ChatShots.Busy(), Hover.Chatting.ChatShots.Finished(), asking3));
            ui.ChatView = false;
            ui.OpenChat(3);
        });
        yield return new Shot("notch-open-new-task", 1, 480, Make([]), Openness: 1, Panels: ui =>
        {
            ui.Accept(Hover.Chatting.ChatShots.State(Hover.Chatting.ChatShots.Busy(), Hover.Chatting.ChatShots.Finished()));
            ui.ChatView = false;
            ui.CloseChat();
            ui.ClearTask();
            ui.Key(Hover.Chat.Ui.UiKey.Escape, Hover.Chat.Ui.UiMods.None);
            ui.Click("fab");
            ui.Char("Tidy the imports again");
        });

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

    internal static async Task TakeAsync(string directory, Hover.Chatting.UiCore? panels = null)
    {
        Directory.CreateDirectory(directory);
        try
        {
            // The panels' own graphics when there are panels: their brushes and fonts belong to that device.
            var own = panels is null ? Gfx.Create() : null;
            var gfx = panels?.Gfx ?? own!;
            using var renderer = new NotchRenderer(gfx);
            foreach (var shot in Fixtures())
            {
                var file = Path.Combine(directory, shot.Name + ".jpg");
                if (shot.Panels is { } setup && panels is not null)
                {
                    panels.FreezeState = true;
                    // The first-task note is not part of these pictures (the data folder of --shots has never seen it).
                    panels.Ui.Accept(new Hover.Host.Protocol.PreferencesEvent { NoticeSeen = true });
                    setup(panels.Ui);
                    // The Markdown and the colours come from the backend; wait for them.
                    for (var i = 0; i < 40; i++)
                    {
                        Hover.Chatting.ChatShots.RenderLayer(panels, 1120, 440, shot.Scale);
                        if (!panels.Ui.Waiting && panels.Ui.Pending == 0 && !panels.Ui.Dirty) break;
                        await Task.Delay(100);
                    }
                }
                var (bgra, w, h) = Draw(gfx, renderer, shot, shot.Panels is not null ? panels : null);
                await Hover.Shots.WriteJpegAsync(bgra, w, h, file);
                HostLog.Line($"picture {file} ({w} x {h})");
            }
            if (panels is not null) panels.FreezeState = false;
            own?.Dispose();
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
    static (byte[] Pixels, int W, int H) Draw(Gfx gfx, NotchRenderer renderer, Shot shot, Hover.Chatting.UiCore? panels)
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
        // The open notch's office is not drawn by the notch renderer: the 3D office is put in its area.
        if (shot.Openness > 0)
            OfficeShots.PasteNotch(output, cw, ph, x0, shot.Scale, scene.OfficeArea(WindowWidth), shot.Name);
        // The office's panels go over the office.
        if (shot.Openness > 0 && panels is not null && shot.Panels is not null)
        {
            var (ax, ay, aw, ah) = scene.OfficeArea(WindowWidth);
            var layer = Hover.Chatting.ChatShots.RenderLayer(panels, (float)aw, (float)ah, shot.Scale);
            int lw = (int)Math.Round(aw * shot.Scale), lh = (int)Math.Round(ah * shot.Scale);
            Hover.Chatting.ChatShots.Over(output, cw, ph, (int)Math.Round(ax * shot.Scale) - x0, (int)Math.Round(ay * shot.Scale), layer, lw, lh);
        }
        return (output, cw, ph);
    }
}
