using Avalonia;
using Avalonia.Headless;

[assembly: AvaloniaTestApplication(typeof(Hover.Tests.AvaloniaEnvironment))]

namespace Hover.Tests;

/// How the test run starts Avalonia.
///
/// Tests marked [AvaloniaTest] run on Avalonia's own thread with this app set up, so
/// they can build controls and draw them. Drawing is left switched on and Skia does
/// the work, because anything touching a bitmap needs the graphics layer up.
///
/// Using the real App also means a broken style file fails the test run rather than
/// only showing up when the app is launched by hand.
public static class AvaloniaEnvironment
{
    public static AppBuilder BuildAvaloniaApp() =>
        AppBuilder.Configure<Hover.App>()
            .UseSkia()
            .UseHeadless(new AvaloniaHeadlessPlatformOptions { UseHeadlessDrawing = false });
}
