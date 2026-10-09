<#
Native, modern folder picker -- the same Explorer-style dialog most current
apps use, with real "Select Folder" / "Cancel" buttons (search box, sidebar,
recent locations). This is Windows' IFileOpenDialog with FOS_PICKFOLDERS.

Selects the original Red Faction Guerrilla install containing RFG.exe and
build\pc\cache. Invalid folders prompt the user to try again.
Prints only the valid chosen path to stdout, or nothing if cancelled.
Exit codes: 0 = selected, 2 = cancelled, 1 = picker error.

Run via: powershell -NoProfile -STA -ExecutionPolicy Bypass -File Extractors\pick_folder.ps1
#>

$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

namespace RfgFolderPicker {
    [ComImport, Guid("DC1C5A9C-E88A-4dde-A5A1-60F82A20AEF7")]
    internal class FileOpenDialogRCW { }

    [ComImport, Guid("43826D1E-E718-42EE-BC55-A1E261C37BFE"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    internal interface IShellItem {
        void BindToHandler();
        void GetParent();
        void GetDisplayName(int sigdnName, out IntPtr ppszName);
        void GetAttributes();
        void Compare();
    }

    // Vtable order matters more than names here -- these mirror IModalWindow
    // + IFileDialog + IFileOpenDialog from the Windows SDK's ShObjIdl_core.h
    // exactly, in declaration order. Methods this script never calls are
    // still declared (as bare `void`, ignoring their real parameters) purely
    // to hold their vtable slot -- skip one and every call after it lands on
    // the wrong native method.
    [ComImport, Guid("d57c7288-d4ad-4768-be02-9d969532d960"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    internal interface IFileOpenDialog {
        [PreserveSig] int Show(IntPtr parent);          // IModalWindow
        void SetFileTypes();                             // IFileDialog...
        void SetFileTypeIndex();
        void GetFileTypeIndex();
        void Advise();
        void Unadvise();
        void SetOptions(uint fos);
        void GetOptions();
        void SetDefaultFolder();
        void SetFolder();
        void GetFolder();
        void GetCurrentSelection();
        void SetFileName();
        void GetFileName();
        void SetTitle([MarshalAs(UnmanagedType.LPWStr)] string title);
        void SetOkButtonLabel();
        void SetFileNameLabel();
        void GetResult(out IShellItem ppsi);
        void AddPlace();
        void SetDefaultExtension();
        void Close();
        void SetClientGuid();
        void ClearClientData();
        void SetFilter();
        void GetResults();                                // IFileOpenDialog
        void GetSelectedItems();
    }

    public static class FolderPicker {
        private const uint FOS_PICKFOLDERS = 0x20;
        private const uint FOS_FORCEFILESYSTEM = 0x40;
        private const uint FOS_PATHMUSTEXIST = 0x800;
        private const uint FOS_NOCHANGEDIR = 0x8;
        private const int ERROR_CANCELLED = unchecked((int)0x800704C7);
        private const int SIGDN_FILESYSPATH = unchecked((int)0x80058000);

        // Returns the picked path, or null if the dialog was cancelled.
        public static string Pick(string title) {
            var dialog = (IFileOpenDialog)new FileOpenDialogRCW();
            IShellItem item = null;
            try {
            dialog.SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR);
            dialog.SetTitle(title);
            int hr = dialog.Show(IntPtr.Zero);
            if (hr == ERROR_CANCELLED) return null;
            Marshal.ThrowExceptionForHR(hr);

            dialog.GetResult(out item);
            IntPtr pszPath;
            item.GetDisplayName(SIGDN_FILESYSPATH, out pszPath);
            try {
                return Marshal.PtrToStringUni(pszPath);
            } finally {
                Marshal.FreeCoTaskMem(pszPath);
            }
            } finally {
                if (item != null) Marshal.ReleaseComObject(item);
                Marshal.ReleaseComObject(dialog);
            }
        }
    }
}
"@

try {
    Add-Type -AssemblyName System.Windows.Forms
    while ($true) {
        $gameFolder = [RfgFolderPicker.FolderPicker]::Pick('Select your Red Faction Guerrilla install folder (containing RFG.exe)')
        if (-not $gameFolder) { exit 2 }
        $exe = Join-Path $gameFolder 'RFG.exe'
        $cache = Join-Path $gameFolder 'build\pc\cache'
        if ((Test-Path -LiteralPath $exe -PathType Leaf) -and
            (Test-Path -LiteralPath $cache -PathType Container)) {
            Write-Output $gameFolder
            exit 0
        }
        [void][System.Windows.Forms.MessageBox]::Show(
            'Choose the original Red Faction Guerrilla folder containing RFG.exe and build\pc\cache. Re-Mars-tered is not supported.',
            'Red Faction Guerrilla install not found',
            [System.Windows.Forms.MessageBoxButtons]::OK,
            [System.Windows.Forms.MessageBoxIcon]::Warning)
    }
} catch {
    [Console]::Error.WriteLine('Could not open the game folder picker: ' + $_.Exception.Message)
    exit 1
}
