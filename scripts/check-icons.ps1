# Checks that a Windows .exe carries the app icon from assets/icon/kiln.ico (made by
# scripts/make-icons.sh) as resources, byte for byte.
#   scripts/check-icons.ps1 -Exe target/release/kiln.exe              every image of kiln.ico
#   scripts/check-icons.ps1 -Exe kiln_..._x64-setup.exe -Installer    the installer's icon:
#       NSIS keeps some of the sizes, so each of its images must be one of kiln.ico's
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [string]$Ico = (Join-Path $PSScriptRoot "../assets/icon/kiln.ico"),
    [switch]$Installer
)
$ErrorActionPreference = 'Stop'

# All in C#: PowerShell is slow with large byte arrays and picks the wrong BitConverter overloads.
Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
public static class ExeIcons {
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern IntPtr LoadLibraryEx(string path, IntPtr file, uint flags);
    [DllImport("kernel32.dll")] static extern bool FreeLibrary(IntPtr module);
    delegate bool EnumNames(IntPtr module, IntPtr type, IntPtr name, IntPtr param);
    [DllImport("kernel32.dll")]
    static extern bool EnumResourceNames(IntPtr module, IntPtr type, EnumNames callback, IntPtr param);
    [DllImport("kernel32.dll")] static extern IntPtr FindResource(IntPtr module, IntPtr name, IntPtr type);
    [DllImport("kernel32.dll")] static extern IntPtr LoadResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32.dll")] static extern IntPtr LockResource(IntPtr data);
    [DllImport("kernel32.dll")] static extern uint SizeofResource(IntPtr module, IntPtr resource);

    static string Hash(byte[] bytes) {
        using (var sha = SHA256.Create()) return Convert.ToHexString(sha.ComputeHash(bytes));
    }

    // The data of every RT_ICON (3) resource in the file.
    static List<string> ExeImages(string path) {
        // LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE: read, never run.
        IntPtr module = LoadLibraryEx(path, IntPtr.Zero, 0x22);
        if (module == IntPtr.Zero) throw new Exception("cannot open " + path);
        var images = new List<string>();
        EnumResourceNames(module, (IntPtr)3, (m, type, name, p) => {
            IntPtr resource = FindResource(m, name, type);
            int size = (int)SizeofResource(m, resource);
            var bytes = new byte[size];
            Marshal.Copy(LockResource(LoadResource(m, resource)), bytes, 0, size);
            images.Add(Hash(bytes));
            return true;
        }, IntPtr.Zero);
        FreeLibrary(module);
        return images;
    }

    // The images in an .ico: a directory of 16-byte entries (size at +8, offset at +12).
    static List<string> IcoImages(string path) {
        byte[] ico = File.ReadAllBytes(path);
        int count = BitConverter.ToUInt16(ico, 4);
        var images = new List<string>();
        for (int i = 0; i < count; i++) {
            int entry = 6 + 16 * i;
            int size = (int)BitConverter.ToUInt32(ico, entry + 8);
            int offset = (int)BitConverter.ToUInt32(ico, entry + 12);
            images.Add(Hash(ico.Skip(offset).Take(size).ToArray()));
        }
        return images;
    }

    public static string Check(string exe, string ico, bool installer) {
        var wanted = IcoImages(ico);
        var found = ExeImages(exe);
        if (found.Count == 0) throw new Exception(exe + " has no icon");
        int foreign = found.Count(h => !wanted.Contains(h));
        if (foreign > 0)
            throw new Exception(exe + " has " + foreign + " icon image(s) not from kiln.ico");
        int missing = wanted.Count(h => !found.Contains(h));
        if (!installer && missing > 0)
            throw new Exception(exe + " lacks " + missing + " of kiln.ico's " + wanted.Count + " images");
        return "ok: " + exe + " carries kiln.ico (" + found.Count + " of its " + wanted.Count + " images)";
    }
}
"@

[ExeIcons]::Check((Resolve-Path $Exe).Path, (Resolve-Path $Ico).Path, $Installer.IsPresent)
