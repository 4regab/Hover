using Hover.Core;
using NUnit.Framework;
[assembly: LevelOfParallelism(1)]
namespace Hover.Tests;
[SetUpFixture]
public sealed class TestEnvironment
{
    public static string Root { get; private set; } = "";
    [OneTimeSetUp]
    public void SetUp()
    {
        var sandbox = Environment.GetEnvironmentVariable("HOVER_SANDBOX_ROOT") ?? throw new InvalidOperationException("Run scripts/test-macos.sh; tests require an isolated sandbox.");
        Root = Path.Combine(sandbox, "test-data");
        Directory.CreateDirectory(Root);
        Environment.SetEnvironmentVariable("HOVER_DATA_DIR", Root);
        Crypto.InitializeKey(System.Security.Cryptography.RandomNumberGenerator.GetBytes(32));
    }
}
