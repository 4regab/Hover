using System.Runtime.InteropServices;
using System.Text;

namespace Hover.Configuring;

/// <summary>The system's dialogs Settings asks for: a folder, and a VS Code theme file. Both wait for the answer on the thread that asks.</summary>
static class Dialogs
{
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct BrowseInfo
    {
        public nint Owner, Root, DisplayName;
        public string? Title;
        public uint Flags;
        public nint Callback, Param;
        public int Image;
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)] static extern nint SHBrowseForFolderW(ref BrowseInfo info);
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)] static extern bool SHGetPathFromIDListW(nint pidl, StringBuilder path);
    [DllImport("ole32.dll")] static extern void CoTaskMemFree(nint pointer);
    [DllImport("ole32.dll")] static extern int OleInitialize(nint reserved);
    [DllImport("user32.dll")] static extern nint GetActiveWindow();

    const uint ReturnOnlyFileSystemFolders = 0x1, EditBox = 0x10, NewDialogStyle = 0x40;

    /// <summary>A folder the user picked, or null when the dialog was cancelled.</summary>
    public static string? Folder(string title)
    {
        _ = OleInitialize(0);
        var name = Marshal.AllocHGlobal(520 * 2);
        try
        {
            var info = new BrowseInfo { Owner = GetActiveWindow(), DisplayName = name, Title = title, Flags = ReturnOnlyFileSystemFolders | NewDialogStyle | EditBox };
            var pidl = SHBrowseForFolderW(ref info);
            if (pidl == 0) return null;
            try
            {
                var path = new StringBuilder(520);
                return SHGetPathFromIDListW(pidl, path) && path.Length > 0 ? path.ToString() : null;
            }
            finally { CoTaskMemFree(pidl); }
        }
        finally { Marshal.FreeHGlobal(name); }
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct OpenFileName
    {
        public int StructSize;
        public nint Owner, Instance;
        public string Filter;
        public string? CustomFilter;
        public int MaxCustFilter, FilterIndex;
        public nint File;
        public int MaxFile;
        public string? FileTitle;
        public int MaxFileTitle;
        public string? InitialDir, Title;
        public int Flags;
        public short FileOffset, FileExtension;
        public string? DefExt;
        public nint CustData, Hook;
        public string? TemplateName;
        public nint Reserved;
        public int Reserved2, FlagsEx;
    }

    [DllImport("comdlg32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool GetOpenFileNameW(ref OpenFileName ofn);

    const int Explorer = 0x80000, FileMustExist = 0x1000, PathMustExist = 0x800, NoChangeDir = 0x8;

    /// <summary>A theme file the user picked (.json or .jsonc), or null when the dialog was cancelled.</summary>
    public static string? ThemeFile()
    {
        const int chars = 32768;
        var buffer = Marshal.AllocHGlobal(chars * 2);
        try
        {
            Marshal.WriteInt16(buffer, 0, 0);
            var ofn = new OpenFileName
            {
                Filter = "VS Code theme files (*.json;*.jsonc)\0*.json;*.jsonc\0All files\0*.*\0\0",
                FilterIndex = 1, File = buffer, MaxFile = chars, Title = "Import a VS Code theme file",
                Flags = Explorer | FileMustExist | PathMustExist | NoChangeDir, Owner = GetActiveWindow(),
            };
            ofn.StructSize = Marshal.SizeOf(ofn);
            if (!GetOpenFileNameW(ref ofn)) return null;
            var path = Marshal.PtrToStringUni(buffer);
            return string.IsNullOrEmpty(path) ? null : path;
        }
        finally { Marshal.FreeHGlobal(buffer); }
    }
}

/// <summary>Windows' light or dark apps setting (<c>core.SystemDark</c>).</summary>
static class SystemTheme
{
    public static bool IsDark()
    {
        try
        {
            using var key = Microsoft.Win32.Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
            return key?.GetValue("AppsUseLightTheme") is int light ? light == 0 : true;
        }
        catch (Exception e) when (e is System.Security.SecurityException or UnauthorizedAccessException or IOException) { return true; }
    }
}
