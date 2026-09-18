using Microsoft.Data.Sqlite;
using NUnit.Framework;
using System.IO;

[assembly: LevelOfParallelism(1)]

namespace Hover.Tests;

[SetUpFixture]
public sealed class TestEnvironment
{
    private static string? _root;

    public static string Root => _root ?? throw new InvalidOperationException("Test environment is not ready");

    [OneTimeSetUp]
    public void SetUp()
    {
        _root = Path.Combine(Path.GetTempPath(), "Hover.Tests", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_root);
        Environment.SetEnvironmentVariable("HOVER_DATA_DIR", _root);
        Environment.SetEnvironmentVariable("HOVER_SHOTS_DIR", Path.Combine(_root, "shots"));
    }

    [OneTimeTearDown]
    public void TearDown()
    {
        // Anything that used the shared store holds the database open, and a locked
        // file cannot be deleted.
        Hover.Core.NoteStore.Shared.Dispose();
        SqliteConnection.ClearAllPools();
        Environment.SetEnvironmentVariable("HOVER_DATA_DIR", null);
        Environment.SetEnvironmentVariable("HOVER_SHOTS_DIR", null);
        if (_root is not null && Directory.Exists(_root))
            Directory.Delete(_root, recursive: true);
    }
}
