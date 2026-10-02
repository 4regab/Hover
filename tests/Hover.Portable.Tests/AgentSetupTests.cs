using Hover.Services;
using NUnit.Framework;

namespace Hover.Tests;

/// What one-click setup decides to run, from what is on PATH. Nothing is installed:
/// only Plan() is called, against a PATH of empty stand-in files.
[TestFixture, NonParallelizable]
public sealed class AgentSetupTests
{
    private string _dir = "";
    private string? _path;

    [SetUp]
    public void SetUp()
    {
        _dir = Path.Combine(Path.GetTempPath(), "hover-setup-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_dir);
        _path = Environment.GetEnvironmentVariable("PATH");
        Environment.SetEnvironmentVariable("PATH", _dir);
    }

    [TearDown]
    public void TearDown()
    {
        Environment.SetEnvironmentVariable("PATH", _path);
        Directory.Delete(_dir, true);
    }

    private void Have(params string[] names)
    {
        foreach (var n in names) File.WriteAllText(Path.Combine(_dir, n), "");
    }

    [Test]
    public void CodexNeedsOnlyItsAdapterWhenTheCliIsThere()
    {
        Have("codex", "npm");
        var plan = AgentSetup.Plan(AgentTool.Codex);
        Assert.That(plan, Has.Count.EqualTo(1));
        Assert.That(plan[0].Command, Does.Contain("@agentclientprotocol/codex-acp").And.Not.Contain("@openai/codex "));
        // Into ~/.local, so no sudo and the bin lands on Hover's PATH.
        Assert.That(plan[0].Command, Does.Contain("--prefix \"$HOME/.local\""));
    }

    [Test]
    public void CodexWithNothingInstallsBothInOneNpmStep()
    {
        Have("npm");
        var plan = AgentSetup.Plan(AgentTool.Codex);
        Assert.That(plan, Has.Count.EqualTo(1));
        Assert.That(plan[0].Command, Does.Contain("@openai/codex").And.Contain("@agentclientprotocol/codex-acp"));
    }

    [Test]
    public void NodeComesFromHomebrewWhenThereIsNoNpm()
    {
        Have("brew");
        var plan = AgentSetup.Plan(AgentTool.Codex);
        Assert.That(plan.Select(s => s.Command), Is.EqualTo(new[] { "brew install node", plan[1].Command }));
    }

    [Test]
    public void KiroAndCursorUseTheirMakersInstallers()
    {
        Assert.That(AgentSetup.Plan(AgentTool.Kiro).Single().Command, Is.EqualTo("curl -fsSL https://cli.kiro.dev/install | bash"));
        Assert.That(AgentSetup.Plan(AgentTool.Cursor).Single().Command, Is.EqualTo("curl -fsS https://cursor.com/install | bash"));
    }

    [Test]
    public void NothingToDoWhenEverythingIsInstalled()
    {
        Have("codex", "codex-acp", "kiro-cli", "cursor-agent", "opencode");
        foreach (var t in Agents.All) Assert.That(AgentSetup.Plan(t), Is.Empty, t.ToString());
    }

    [Test]
    public void EachToolSignsInWithItsOwnCommand()
    {
        Assert.That(AgentSetup.SignInCommand(AgentTool.Codex), Is.EqualTo("codex login"));
        Assert.That(AgentSetup.SignInCommand(AgentTool.Kiro), Is.EqualTo("kiro-cli login"));
        Assert.That(AgentSetup.SignInCommand(AgentTool.Cursor), Is.EqualTo("cursor-agent login"));
    }
}
