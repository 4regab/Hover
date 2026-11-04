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

    // The system's own folder picker (IFileOpenDialog), the one the Go app uses (internal/platform/win/pick.go): the Explorer-style window
    // with the address bar and the search, not the small tree the old SHBrowseForFolder shows.
    [ComImport, Guid("DC1C5A9C-E88A-4DDE-A5A1-60F82A20AEF7")] class FileOpenDialog { }

    [ComImport, Guid("43826d1e-e718-42ee-bc55-a1e261c37bfe"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IShellItem
    {
        void BindToHandler(nint context, ref Guid handler, ref Guid iid, out nint result);
        void GetParent(out IShellItem parent);
        void GetDisplayName(uint form, [MarshalAs(UnmanagedType.LPWStr)] out string name);
    }

    // The slots are in the interface's own order (IModalWindow, then IFileDialog); only the ones used have real signatures.
    [ComImport, Guid("d57c7288-d4ad-4768-be02-9d969532d960"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IFileOpenDialog
    {
        [PreserveSig] int Show(nint owner);
        void SetFileTypes(uint count, nint specs);
        void SetFileTypeIndex(uint index);
        void GetFileTypeIndex(out uint index);
        void Advise(nint events, out uint cookie);
        void Unadvise(uint cookie);
        void SetOptions(uint options);
        void GetOptions(out uint options);
        void SetDefaultFolder(IShellItem item);
        void SetFolder(IShellItem item);
        void GetFolder(out IShellItem item);
        void GetCurrentSelection(out IShellItem item);
        void SetFileName([MarshalAs(UnmanagedType.LPWStr)] string name);
        void GetFileName([MarshalAs(UnmanagedType.LPWStr)] out string name);
        void SetTitle([MarshalAs(UnmanagedType.LPWStr)] string title);
        void SetOkButtonLabel([MarshalAs(UnmanagedType.LPWStr)] string text);
        void SetFileNameLabel([MarshalAs(UnmanagedType.LPWStr)] string text);
        void GetResult(out IShellItem item);
    }

    const uint PickFolders = 0x20, ForceFileSystem = 0x40, FileSystemPath = 0x80058000;

    /// <summary>The window the dialog belongs to: the one with the keyboard, else the one in front, so that it opens over the notch and not under it.</summary>
    static nint Owner()
    {
        var active = GetActiveWindow();
        return active != 0 ? active : GetForegroundWindow();
    }

    [DllImport("user32.dll")] static extern nint GetForegroundWindow();

    /// <summary>A folder the user picked, or null when the dialog was cancelled.</summary>
    public static string? Folder(string title)
    {
        _ = OleInitialize(0);
        try
        {
            var dialog = (IFileOpenDialog)new FileOpenDialog();
            try
            {
                dialog.GetOptions(out var options);
                dialog.SetOptions(options | PickFolders | ForceFileSystem);
                dialog.SetTitle(title);
                // Cancelled is an HRESULT, not a failure.
                if (dialog.Show(Owner()) < 0) return null;
                dialog.GetResult(out var item);
                try
                {
                    item.GetDisplayName(FileSystemPath, out var path);
                    return string.IsNullOrEmpty(path) ? null : path;
                }
                finally { Marshal.ReleaseComObject(item); }
            }
            finally { Marshal.ReleaseComObject(dialog); }
        }
        catch (Exception e) when (e is COMException or InvalidCastException or MarshalDirectiveException)
        {
            // No picker on this system (a stripped-down Windows): the older folder window still works.
            Hover.Host.HostLog.Line($"folder dialog: the system's picker could not be made, using the older one — {e.Message}");
            return FolderOlder(title);
        }
    }

    static string? FolderOlder(string title)
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
            // With no value Windows is light (the Go app reads it so: core.SystemLook).
            return key?.GetValue("AppsUseLightTheme") is int light && light == 0;
        }
        catch (Exception e) when (e is System.Security.SecurityException or UnauthorizedAccessException or IOException) { return false; }
    }
}
