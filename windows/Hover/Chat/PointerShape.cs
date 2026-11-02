using System.Runtime.InteropServices;
using Hover.Host;

namespace Hover.Chatting;

/// <summary>
/// The pointer's shape in the app window, answered the way the notch answers it: the system asks (WM_SETCURSOR) on every move over the
/// window's client area, and the windows that take that question (the top one and the one XAML draws in) answer with the shape the panels
/// last chose. XAML's own cursor property was not enough: the live check saw the normal shape over a button.
/// </summary>
static class PointerShape
{
    /// <summary>0 the normal shape, 1 the hand, 2 the text cursor, 3 left and right (the values of <c>UiCursor</c>).</summary>
    public static volatile int Shape;

    const uint WM_SETCURSOR = 0x20;

    delegate nint SubclassProc(nint hwnd, uint message, nint wParam, nint lParam, nuint id, nuint data);

    delegate bool EnumProc(nint hwnd, nint data);

    // Kept in a field: the system calls it for as long as the windows live.
    static readonly SubclassProc Keep = Answer;

    [DllImport("comctl32.dll")] static extern bool SetWindowSubclass(nint hwnd, SubclassProc proc, nuint id, nuint data);
    [DllImport("comctl32.dll")] static extern nint DefSubclassProc(nint hwnd, uint message, nint wParam, nint lParam);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(nint parent, EnumProc proc, nint data);
    [DllImport("user32.dll")] static extern nint LoadCursorW(nint instance, nint name);
    [DllImport("user32.dll")] static extern nint SetCursor(nint cursor);

    /// <summary>Takes the window and the windows inside it into the answer. Call it once the window is showing.</summary>
    public static void Apply(nint top)
    {
        if (top == 0) return;
        var ok = SetWindowSubclass(top, Keep, 2, 0);
        var classes = new List<string>();
        EnumChildWindows(top, (child, _) =>
        {
            ok &= SetWindowSubclass(child, Keep, 2, 0);
            classes.Add($"0x{child:X} {ClassOf(child)}");
            return true;
        }, 0);
        // For the checks of the real app (HOVER_UI_DUMP): which windows took the answer.
        if (UiDump.On) HostLog.Line($"pointer shape: top 0x{top:X} {ClassOf(top)} and {classes.Count} windows inside took the answer (all taken: {ok}): {string.Join("; ", classes)}");
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(nint hwnd, char[] text, int max);

    static string ClassOf(nint hwnd)
    {
        var text = new char[128];
        var n = GetClassNameW(hwnd, text, text.Length);
        return new string(text, 0, n);
    }

    static int asked;
    static int lastLogged = -1;

    static nint Id(int shape) => shape switch { 1 => 32649, 2 => 32513, 3 => 32644, _ => 32512 };

    static nint Answer(nint hwnd, uint message, nint wParam, nint lParam, nuint id, nuint data)
    {
        // Over the client area only (the low word is the hit-test code, 1 is HTCLIENT): the title bar and the borders are the system's.
        if (message == WM_SETCURSOR)
        {
            // For the checks of the real app: the first questions, and every change of the shape asked for.
            if (UiDump.On && (asked++ < 12 || Shape != lastLogged))
            {
                lastLogged = Shape;
                HostLog.Line($"pointer shape: WM_SETCURSOR to 0x{hwnd:X} {ClassOf(hwnd)}, hit test {lParam & 0xFFFF}, the panels want shape {Shape}");
            }
            if (Shape != 0 && (lParam & 0xFFFF) == 1)
            {
                SetCursor(LoadCursorW(0, Id(Shape)));
                return 1;
            }
        }
        return DefSubclassProc(hwnd, message, wParam, lParam);
    }
}
