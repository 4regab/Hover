using Xunit;

namespace Hover.Settings.Tests;

/// <summary>The settings folder crossing between the Go app and this one (the Windows job makes the Go folder with <c>hover-data write</c>).</summary>
public class CrossReadTests
{
    static void Copy(string from, string to)
    {
        Directory.CreateDirectory(to);
        foreach (var file in Directory.GetFiles(from)) File.Copy(file, Path.Combine(to, Path.GetFileName(file)));
        foreach (var dir in Directory.GetDirectories(from)) Copy(dir, Path.Combine(to, Path.GetFileName(dir)));
    }

    [WindowsFact]
    public async Task Settings_the_Go_app_wrote_show_on_the_Kiro_page_and_a_change_is_saved_back_as_Go_reads_it()
    {
        var made = Environment.GetEnvironmentVariable("HOVER_FIXTURE_GO");
        Assert.True(Directory.Exists(made), "HOVER_FIXTURE_GO must name the folder hover-data write made (see docs/development/windows.md).");
        using var temp = new TempDir();
        var data = Path.Combine(temp.Path, "go-data");
        Copy(made!, data);
        // hover-data write puts the project folder (<folder>\project) in the Kiro settings.
        var expected = Path.GetFullPath(Path.Combine(made!, "project"));

        var key = Rig.KeyFor(data);
        Assert.Equal(32, key.Length);
        await using (var rig = await Rig.StartAsync(data, key))
        {
            rig.Do(c => c.Open(Section.Kiro));
            Assert.Equal(expected, Path.GetFullPath(rig.Read(c => c.Data.KiroFolder)!), ignoreCase: true);
            // The page shows the folder on its row.
            Assert.Contains(rig.Read(c => c.Blocks).Where(b => b.Kind == BlockKind.Group).SelectMany(b => b.Rows),
                r => r.Label == "Project folder" && r.Sub.Contains(Path.GetFileName(expected), StringComparison.Ordinal));
            // A change made here is written the way Go reads it.
            rig.Do(c => c.Toggled("KiroRetryBusy", true));
            await rig.Until(c => c.Data.KiroRetryBusy, "the change");
        }
        Assert.Contains("\"KiroRetryBusy\": true", File.ReadAllText(Path.Combine(data, "settings.json")));
        Assert.Contains("A long rich conversation", GoTool.Run("dump", data));
        GoTool.Check(data);
    }
}
