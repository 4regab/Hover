using Xunit;

namespace Hover.Office.Tests;

public sealed class OfficeAssemblyTests
{
    [Fact]
    public void The_office_assembly_loads()
    {
        Assert.Equal("Hover.Office", typeof(OfficeAssembly).Assembly.GetName().Name);
    }
}
