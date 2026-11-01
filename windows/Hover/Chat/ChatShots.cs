using System.Runtime.InteropServices;
using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host;
using Hover.Host.Protocol;
using Hover.Windowing;
using Vortice.Direct2D1;
using Vortice.DXGI;
using AlphaMode = Vortice.DCommon.AlphaMode;
using PixelFormat = Vortice.DCommon.PixelFormat;

namespace Hover.Chatting;

/// <summary>
/// <c>--shots</c> for the panels: the chat view, the start screen, the new-task box, the questions and
/// the menus, drawn by the same code the windows use onto a bitmap that is read back (no screen
/// capture). The Markdown, the code colours and the numbered changes come from the real backend, so
/// the chat is drawn as it is drawn live. Names follow the Go app's pictures.
/// </summary>
static class ChatShots
{
    const uint Back = 0x0c0b0e;

    static long Ms(int minutesAgo = 0) => DateTimeOffset.UtcNow.AddMinutes(-minutesAgo).ToUnixTimeMilliseconds();

    // MARK: Fixtures

    static StepInfo Read(string name, string dir) => new() { K = "read", Verb = "Read", Name = name, Dir = dir, Status = "completed" };

    static StepInfo Thought(string text) => new() { K = "thought", Verb = "Thinking", Status = "completed", Out = text, Ms = 18000 };

    static StepInfo Run(string cmd, string output, int exit) => new() { K = "run", Verb = "Ran", Cmd = cmd, Status = exit == 0 ? "completed" : "failed", Out = output, Exit = exit, Ms = 2400 };

    const string Diff = "--- a/app/imports.ts\n+++ b/app/imports.ts\n@@ -1,6 +1,7 @@\n import { a } from './a';\n-import { b } from './b';\n-import { c } from './c';\n+import { c } from './c';\n+import { b } from './b';\n import { d } from './d';\n+import { e } from './e';\n \n export const all = [a, b, c, d];\n";

    static StepInfo Edit(string name, string dir, string diff = Diff, int add = 3, int del = 2) => new() { K = "edit", Verb = "Edited", Name = name, Dir = dir, Status = "completed", Diff = diff, Add = add, Del = del };

    const string Answer = """
        I sorted the imports in `app/imports.ts` and added the missing one.

        ## What changed

        - **Order:** `c` now comes before `b`, as the style guide asks.
        - **New import:** `e` is used by `all` further down.
        - Everything still builds: [the run's log](https://example.com/log).

        | File | Added | Removed |
        |---|---:|---:|
        | `app/imports.ts` | 3 | 2 |
        | `app/index.ts` | 0 | 0 |

        ```ts
        import { c } from './c';
        import { b } from './b';

        export const all = [a, b, c, d, e];
        ```

        ```mermaid
        flowchart LR
          A[Sort imports] --> B{Builds?}
          B -- yes --> C[Done]
          B -- no --> D[Fix]
        ```
        """;

    internal static SessionInfo Session(int id, string tool, string title, string stage, int bot, params TurnInfo[] turns) => new()
    {
        Id = id, Key = id.ToString("x").PadRight(32, 'c'), Tool = tool, Bot = bot, Title = title, Folder = @"C:\work\project", Stage = stage, Turns = [.. turns],
    };

    internal static TurnInfo Turn(string prompt, string answer, string stage, int minutesAgo, params StepInfo[] steps) => new()
    {
        Prompt = prompt, Answer = answer, Stage = stage, T0 = Ms(minutesAgo), Steps = [.. steps], Took = stage == "done" ? 187000 : null,
    };

    internal static AskInfo Permission() => new()
    {
        Id = "n1", Kind = "execute", Title = "Wants to run a command", Command = "npm install three@0.171.0", Allow = "Run",
        Reason = "Installs packages or uses the network",
    };

    internal static StateEvent State(params SessionInfo[] sessions) => new()
    {
        CanStart = true, Tool = "kiro", Folder = @"C:\work\project", MaxRunning = 4,
        Tools =
        [
            new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true, Access = "full", Model = "auto", Models = [new ModelInfo { Id = "auto", Name = "Auto" }, new ModelInfo { Id = "opus", Name = "Opus 5.5", Levels = ["Low", "High"] }] },
            new ToolInfo { Id = "codex", Name = "Codex", Ready = true, Access = "risky", Models = [new ModelInfo { Id = "gpt", Name = "GPT" }] },
            new ToolInfo { Id = "claude", Name = "Claude Code", Ready = true, Access = "risky" },
        ],
        Sessions = [.. sessions],
        History =
        [
            new HistoryRow { Key = "old1", Tool = "claude", Title = "Make a release build and publish it", Folder = @"C:\work\project", At = Ms(1) },
            new HistoryRow { Key = "old2", Tool = "codex", Title = "Desk fixture done: open the card", Folder = @"C:\work\project", At = Ms(3) },
        ],
    };

    internal static SessionInfo Busy() => Session(1, "kiro", "Desk fixture busy: make refresh() skip views that are clean", "working", 2,
        Turn("Now make it hold when the taskbar is at the top too, and check all three monitors.", "", "working", 2,
            Read("win.rs", "apps/hover/src"), Thought("and draw over the taskbar, since the window is topmost anyway.\n\nDrawing over the taskbar hides the clock on some setups. Starting under it is safer.\n\nThere are three monitors to check, and Linux may have the same bug.")));

    internal static SessionInfo Finished() => Session(2, "kiro", "Tidy the imports again", "done", 2,
        Turn("Tidy the imports again", Answer, "done", 3,
            Read("imports.ts", "app"), Run("npm test", "✓ 14 files sorted\nTests: 42 passed, 42 total", 0), Edit("imports.ts", "app")) with { Restore = true, Again = true, Credits = 0.09 });

    // MARK: Taking the pictures

    public static async Task TakeAsync(string directory, UiCore core)
    {
        var ui = core.Ui;
        core.FreezeState = true;
        // Paints once so the places on the screen are known before a click.
        void S(int h = 720) => RenderLayer(core, 1200, h, 1);
        void Esc() => ui.Key(UiKey.Escape, UiMods.None);
        try
        {
            // The chat view, as the Go app's window shows it.
            await Shot(core, directory, "chat-view-chat", 1200, 720, () => { ui.Accept(State(Busy(), Finished())); ui.ChatView = true; ui.OpenChat(1); });
            await Shot(core, directory, "chat-view-home", 1200, 720, () => { ui.CloseChat(); ui.ChatView = true; });
            await Shot(core, directory, "chat-view-home-typed", 1200, 720, () => { S(); ui.Click("taskText"); ui.Char("Explain the whole notch, once more."); });
            await Shot(core, directory, "chat-view-home-agent-menu", 1200, 720, () => { S(); ui.Click("pillAgent"); });
            await Shot(core, directory, "chat-view-home-access-menu", 1200, 720, () => { Esc(); S(); ui.Click("pillAccess"); });
            await Shot(core, directory, "chat-view-home-project-menu", 1200, 720, () => { Esc(); S(); ui.Click("pillFolder"); });
            await Shot(core, directory, "chat-view-home-where-menu", 1200, 720, () => { Esc(); S(); ui.Click("pillWhere"); });
            await Shot(core, directory, "chat-view-header-menu", 1200, 720, () => { Esc(); ui.OpenChat(1); S(); ui.Click("more"); });
            await Shot(core, directory, "chat-view-header-renaming", 1200, 720, () => { Esc(); S(); ui.Click("rename"); });
            await Shot(core, directory, "chat-view-chat-rest-draft", 1200, 720, () => { Esc(); Esc(); ui.CloseChat(); ui.OpenChat(1); S(); ui.Click("compose"); S(); ui.Char("Please check the second monitor too."); });

            // A finished turn: Markdown, a table, code, a flowchart, the changes, the acts.
            await Shot(core, directory, "chat-default-done", 1200, 720, () => { ui.CloseChat(); ui.OpenChat(2); });
            await Shot(core, directory, "chat-default-done-top", 1200, 720, () => ui.ScrollToTop());
            await Shot(core, directory, "chat-default-done-timeline-diff", 1200, 720, () => ui.OpenFirstChange());
            await Shot(core, directory, "chat-default-rewind-confirm", 1200, 720, () => ui.AskRestore());

            // A question at the end of the chat, and a queued reply.
            var asking = Session(3, "kiro", "Install three.js", "waiting", 2, Turn("Install three.js 0.171", "", "waiting", 1, Read("package.json", "."))) with { Ask = Permission() };
            await Shot(core, directory, "chat-default-ask", 1200, 720, () => { Esc(); ui.Accept(State(Busy(), Finished(), asking)); ui.OpenChat(3); });
            var queued = Session(4, "kiro", "A long run", "working", 3,
                Turn("Refactor the settings page", "Working through the files.", "working", 2, Read("settings.rs", "src")),
                Turn("And then add a test for it", "", "done", 0) with { Queued = true, Uid = "u1" });
            await Shot(core, directory, "chat-default-queued", 1200, 720, () => { Esc(); ui.Accept(State(Busy(), Finished(), asking, queued)); ui.OpenChat(4); });

            // The office with the new-task circle and box.
            await Shot(core, directory, "new-task-box", 1200, 720, () => { Esc(); ui.ChatView = false; ui.CloseChat(); ui.ClearTask(); ui.Accept(State(Busy(), Finished())); S(); ui.Click("fab"); S(); ui.Char("The whole notch, explained once more."); }, office: true);
            await Shot(core, directory, "office-fab-rest", 1200, 720, () => { ui.Key(UiKey.Escape, UiMods.None); ui.Key(UiKey.Escape, UiMods.None); }, office: true);

            // The questions over the bots, and in the drawer.
            await Shot(core, directory, "office-ask-over", 1200, 480, () => { Esc(); Esc(); ui.Accept(State(Busy(), Finished(), asking)); ui.ChatView = false; ui.CloseChat(); }, office: true);
            await Shot(core, directory, "office-ask-chat", 1200, 480, () => { ui.OpenChat(3); }, office: true);

            // The nearby inputs: very long answers, a huge change, a broken flowchart and picture, two questions.
            var broken = Session(5, "kiro", "Odd content", "done", 1, Turn("Show odd content", "A picture that cannot load:\n\n![the logo](missing.png)\n\n```mermaid\nflowchart LR\n  A[One] -->\n```\n\nAnd text after.", "done", 1));
            await Shot(core, directory, "chat-extra-bad-diagram-and-image", 1200, 720, () => { ui.Accept(State(Busy(), Finished(), asking, queued, broken)); ui.ChatView = true; ui.OpenChat(5); });
            // A picture that loads: a file in the session's folder, shown at its own size.
            var pictures = Directory.CreateTempSubdirectory("hover-shots-files.").FullName;
            WritePng(Path.Combine(pictures, "gradient.png"), 360, 140);
            var withPicture = Session(9, "kiro", "A picture", "done", 5, Turn("Show me a picture", "Here it is:\n\n![a gradient](gradient.png)\n\nThe size is its own.", "done", 1)) with { Folder = pictures };
            await Shot(core, directory, "chat-extra-picture", 1200, 720, () => { ui.Accept(State(Busy(), Finished(), asking, queued, broken, withPicture)); ui.OpenChat(9); });
            var longText = string.Join("\n\n", Enumerable.Range(1, 10_000).Select(i => $"Line {i} of an answer that goes on and on and on."));
            var longSession = Session(6, "kiro", "Print everything", "done", 1, Turn("Print everything", longText, "done", 1));
            await Shot(core, directory, "chat-extra-ten-thousand-lines", 1200, 720, () => { ui.Accept(State(Busy(), Finished(), asking, queued, broken, withPicture, longSession)); ui.OpenChat(6); }, rounds: 60);
            var hugeDiff = "--- a/big.ts\n+++ b/big.ts\n@@ -1,3000 +1,3000 @@\n" + string.Join("\n", Enumerable.Range(1, 3000).Select(i => (i % 3 == 0 ? "+" : i % 3 == 1 ? "-" : " ") + $"const value{i} = {i};"));
            var hugeSession = Session(7, "kiro", "A huge change", "done", 1, Turn("Rewrite it all", "Rewritten.", "done", 1, Edit("big.ts", "src", hugeDiff, 1000, 1000)));
            await Shot(core, directory, "chat-extra-huge-diff", 1200, 720, () => { ui.Accept(State(Busy(), Finished(), asking, queued, broken, withPicture, longSession, hugeSession)); ui.OpenChat(7); S(); ui.OpenFirstChange(); }, rounds: 60);
            var second = Session(8, "codex", "Another question", "waiting", 4, Turn("Delete the build folder", "", "waiting", 1)) with { Ask = Permission() with { Id = "n2", Command = "rm -rf build", Title = "Wants to delete files", Danger = true, Allow = "Delete", Reason = "Deletes files" } };
            await Shot(core, directory, "office-ask-two", 1200, 480, () => { ui.Accept(State(asking, second)); ui.CloseChat(); ui.ChatView = false; }, office: true);
        }
        catch (Exception e)
        {
            HostLog.Line($"the panel pictures failed — {e}");
            throw;
        }
        finally { core.FreezeState = false; }
    }

    /// <summary>A small PNG with a colour gradient, written by hand (stored zlib blocks), so a picture can be shown without a file in the repo.</summary>
    static void WritePng(string file, int w, int h)
    {
        var raw = new byte[h * (1 + w * 3)];
        for (var y = 0; y < h; y++)
        {
            var row = y * (1 + w * 3);
            for (var x = 0; x < w; x++)
            {
                raw[row + 1 + x * 3] = (byte)(40 + 180 * x / w);
                raw[row + 2 + x * 3] = (byte)(70 + 120 * y / h);
                raw[row + 3 + x * 3] = (byte)(220 - 150 * x / w);
            }
        }
        using var z = new MemoryStream();
        z.WriteByte(0x78);
        z.WriteByte(0x01);
        for (var at = 0; at < raw.Length; at += 65535)
        {
            var n = Math.Min(65535, raw.Length - at);
            z.WriteByte((byte)(at + n >= raw.Length ? 1 : 0));
            z.Write([(byte)n, (byte)(n >> 8), (byte)~n, (byte)(~n >> 8)]);
            z.Write(raw, at, n);
        }
        uint a = 1, b = 0;
        foreach (var v in raw) { a = (a + v) % 65521; b = (b + a) % 65521; }
        z.Write([(byte)(b >> 8), (byte)b, (byte)(a >> 8), (byte)a]);
        using var png = new MemoryStream();
        png.Write([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        void Chunk(string kind, byte[] data)
        {
            Span<byte> len = stackalloc byte[4];
            System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(len, (uint)data.Length);
            png.Write(len);
            var body = System.Text.Encoding.ASCII.GetBytes(kind).Concat(data).ToArray();
            png.Write(body);
            uint crc = 0xFFFFFFFF;
            foreach (var v in body)
            {
                crc ^= v;
                for (var k = 0; k < 8; k++) crc = (crc & 1) != 0 ? 0xEDB88320 ^ (crc >> 1) : crc >> 1;
            }
            System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(len, ~crc);
            png.Write(len);
        }
        var ihdr = new byte[13];
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(ihdr, (uint)w);
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(ihdr.AsSpan(4), (uint)h);
        ihdr[8] = 8;
        ihdr[9] = 2;
        Chunk("IHDR", ihdr);
        Chunk("IDAT", z.ToArray());
        Chunk("IEND", []);
        File.WriteAllBytes(file, png.ToArray());
    }

    static async Task Shot(UiCore core, string directory, string name, int w, int h, Action setup, bool office = false, int rounds = 40)
    {
        var ui = core.Ui;
        setup();
        byte[] pixels = [];
        for (var round = 0; round < rounds; round++)
        {
            pixels = Capture(core, w, h, office);
            if (!ui.Waiting && ui.Pending == 0 && !ui.Dirty) break;
            await Task.Delay(100);
        }
        var file = Path.Combine(directory, name + ".jpg");
        await Hover.Shots.WriteJpegAsync(pixels, w, h, file);
        HostLog.Line($"picture {file} ({w} x {h})");
    }

    /// <summary>Draws the panels at 1x over the background (the office, in an office view) and returns opaque BGRA pixels.</summary>
    static byte[] Capture(UiCore core, int w, int h, bool office)
    {
        var layer = RenderLayer(core, w, h, 1);
        var output = new byte[w * h * 4];
        for (var i = 0; i < output.Length; i += 4)
        {
            output[i] = (byte)(Back & 0xff);
            output[i + 1] = (byte)((Back >> 8) & 0xff);
            output[i + 2] = (byte)((Back >> 16) & 0xff);
            output[i + 3] = 255;
        }
        // In an office view the 3D office is put in under the panels.
        if (office && !core.Ui.ChatView) OfficeShots.PasteDashboard(output, w, h, 1, new OfficeRect(0, 0, w, h));
        Over(output, w, h, 0, 0, layer, w, h);
        return output;
    }

    /// <summary>Draws the panels offscreen onto a see-through bitmap and returns its premultiplied BGRA pixels.</summary>
    public static byte[] RenderLayer(UiCore core, float w, float h, double scale)
    {
        int pw = (int)Math.Round(w * scale), ph = (int)Math.Round(h * scale);
        var dc = core.Gfx.Context;
        var format = new PixelFormat(Format.B8G8R8A8_UNorm, AlphaMode.Premultiplied);
        using var target = dc.CreateBitmap(new Vortice.Mathematics.SizeI(pw, ph), IntPtr.Zero, 0, new BitmapProperties1(format, 96, 96, BitmapOptions.Target));
        dc.Target = target;
        dc.SetDpi((float)(96 * scale), (float)(96 * scale));
        dc.BeginDraw();
        try
        {
            dc.Clear(null);
            core.Paint(dc, w, h);
        }
        finally
        {
            dc.EndDraw().CheckError();
            dc.Target = null;
        }
        using var cpu = dc.CreateBitmap(new Vortice.Mathematics.SizeI(pw, ph), IntPtr.Zero, 0, new BitmapProperties1(format, 96, 96, BitmapOptions.CpuRead | BitmapOptions.CannotDraw));
        cpu.CopyFromBitmap(target).CheckError();
        var mapped = cpu.Map(MapOptions.Read);
        var bytes = new byte[pw * ph * 4];
        try
        {
            for (var y = 0; y < ph; y++) Marshal.Copy(mapped.Bits + y * (int)mapped.Pitch, bytes, y * pw * 4, pw * 4);
        }
        finally { cpu.Unmap(); }
        return bytes;
    }

    /// <summary>Lays a premultiplied picture over an opaque one at (x, y); what falls outside is cut off.</summary>
    public static void Over(byte[] dst, int dw, int dh, int x0, int y0, byte[] src, int sw, int sh)
    {
        for (var y = 0; y < sh; y++)
        {
            var dy = y + y0;
            if (dy < 0 || dy >= dh) continue;
            for (var x = 0; x < sw; x++)
            {
                var dx = x + x0;
                if (dx < 0 || dx >= dw) continue;
                int s = (y * sw + x) * 4, d = (dy * dw + dx) * 4;
                int a = src[s + 3];
                if (a == 0) continue;
                dst[d] = (byte)Math.Min(src[s] + dst[d] * (255 - a) / 255, 255);
                dst[d + 1] = (byte)Math.Min(src[s + 1] + dst[d + 1] * (255 - a) / 255, 255);
                dst[d + 2] = (byte)Math.Min(src[s + 2] + dst[d + 2] * (255 - a) / 255, 255);
                dst[d + 3] = 255;
            }
        }
    }
}
