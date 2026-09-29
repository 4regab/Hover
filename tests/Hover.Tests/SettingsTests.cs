using System.IO;
using System.Windows.Input;
using Hover.Core;
using NUnit.Framework;

namespace Hover.Tests;

[NonParallelizable]
public sealed class SettingsTests
{
    [Test]
    public void Flush_writes_readable_JSON_with_the_shortcut_and_notch_items()
    {
        var shortcut = Settings.ScWorkspace;
        var items = Settings.NotchItems;
        try
        {
            Settings.ScWorkspace = new Shortcut(ModifierKeys.Control | ModifierKeys.Shift, Key.H);
            // "clock" and "timer" were notch items once; old settings files may still hold them.
            Settings.NotchItems = new[] { NotchItem.Kiro, "bogus", "clock", "timer", NotchItem.Claude };
            Settings.Flush();

            var json = File.ReadAllText(Paths.SettingsFile);
            Assert.Multiple(() =>
            {
                Assert.That(json, Does.Contain("\"ScWorkspace\""));
                Assert.That(json, Does.Contain("\"Key\": \"H\""));
                Assert.That(json, Does.Contain("\"NotchItems\""));
                // Unknown ids are dropped, and the canonical order wins over the order given.
                Assert.That(Settings.NotchItems, Is.EqualTo(new[] { NotchItem.Claude, NotchItem.Kiro }));
            });
        }
        finally
        {
            Settings.ScWorkspace = shortcut;
            Settings.NotchItems = items;
            Settings.Flush();
        }
    }

    [Test]
    public void Kiros_folder_and_the_note_are_saved_and_a_blank_folder_is_none()
    {
        var folder = Settings.KiroFolder;
        var seen = Settings.KiroNoticeSeen;
        try
        {
            Settings.KiroFolder = @"C:\Projects\Hover";
            Settings.KiroNoticeSeen = true;
            Settings.Flush();
            var json = File.ReadAllText(Paths.SettingsFile);
            Assert.Multiple(() =>
            {
                Assert.That(json, Does.Contain("\"KiroFolder\": \"C:\\\\Projects\\\\Hover\""));
                Assert.That(json, Does.Contain("\"KiroNoticeSeen\": true"));
            });
            Settings.KiroFolder = "   ";
            Assert.That(Settings.KiroFolder, Is.Null);
        }
        finally
        {
            Settings.KiroFolder = folder;
            Settings.KiroNoticeSeen = seen;
            Settings.Flush();
        }
    }

    [Test]
    public void Each_agents_approval_is_kept_and_asking_is_opt_in()
    {
        var kiro = Settings.AgentOptions(Services.AgentTool.Kiro);
        var codex = Settings.AgentOptions(Services.AgentTool.Codex);
        try
        {
            Settings.SetAgentOptions(Services.AgentTool.Kiro, kiro with { Approval = Services.AgentApproval.Risky });
            Settings.SetAgentOptions(Services.AgentTool.Codex, codex with { Approval = Services.AgentApproval.Always });
            Settings.Flush();
            var json = File.ReadAllText(Paths.SettingsFile);
            Assert.Multiple(() =>
            {
                Assert.That(Settings.AgentOptions(Services.AgentTool.Kiro).Approval, Is.EqualTo(Services.AgentApproval.Risky));
                Assert.That(Settings.AgentOptions(Services.AgentTool.Codex).Approval, Is.EqualTo(Services.AgentApproval.Always));
                Assert.That(new Services.AgentOptions().Approval, Is.EqualTo(Services.AgentApproval.Autopilot), "asking is opt-in");
                Assert.That(json, Does.Contain("\"KiroApproval\": \"Risky\""));
            });
        }
        finally
        {
            Settings.SetAgentOptions(Services.AgentTool.Kiro, kiro);
            Settings.SetAgentOptions(Services.AgentTool.Codex, codex);
            Settings.Flush();
        }
    }

    [Test]
    public void The_old_planner_is_removed_and_the_key_the_history_needs_stays()
    {
        var key = Path.Combine(Paths.Support, "note.key");
        var hadKey = File.Exists(key);
        if (!hadKey) File.WriteAllText(key, "k");
        foreach (var f in new[] { "planner.dat", "planner.dat.tmp", "planner.dat.unreadable-20260101000000" })
            File.WriteAllText(Path.Combine(Paths.Support, f), "x");
        Owl.OwlApp.DropPlanner();
        Assert.Multiple(() =>
        {
            Assert.That(Directory.EnumerateFiles(Paths.Support, "planner.dat*"), Is.Empty);
            Assert.That(File.Exists(key), Is.True);
        });
        if (!hadKey) File.Delete(key);
    }

    [Test]
    public void Notch_items_toggle_one_at_a_time()
    {
        var items = Settings.NotchItems;
        try
        {
            Settings.NotchItems = new[] { NotchItem.Kiro };
            Settings.SetNotchItem(NotchItem.Codex, true);
            Assert.That(Settings.HasNotchItem(NotchItem.Codex), Is.True);
            Settings.SetNotchItem(NotchItem.Kiro, false);
            Assert.That(Settings.NotchItems, Is.EqualTo(new[] { NotchItem.Codex }));
        }
        finally
        {
            Settings.NotchItems = items;
            Settings.Flush();
        }
    }
}
