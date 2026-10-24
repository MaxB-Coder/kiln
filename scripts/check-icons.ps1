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

Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
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

    // The data of every RT_ICON (3) resource in the file.
    public static List<byte[]> Images(string path) {
        // LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE: read, never run.
        IntPtr module = LoadLibraryEx(path, IntPtr.Zero, 0x22);
        if (module == IntPtr.Zero) throw new Exception("cannot open " + path);
        var images = new List<byte[]>();
        EnumResourceNames(module, (IntPtr)3, (m, type, name, p) => {
            IntPtr resource = FindResource(m, name, type);
            int size = (int)SizeofResource(m, resource);
            var bytes = new byte[size];
            Marshal.Copy(LockResource(LoadResource(m, resource)), bytes, 0, size);
            images.Add(bytes);
            return true;
        }, IntPtr.Zero);
        FreeLibrary(module);
        return images;
    }
}
"@

# The images in the .ico: a directory of 16-byte entries (size at +8, offset at +12).
$ico = [System.IO.File]::ReadAllBytes((Resolve-Path $Ico))
$count = [BitConverter]::ToUInt16($ico, 4)
$expected = @()
for ($i = 0; $i -lt $count; $i++) {
    $entry = 6 + 16 * $i
    $size = [BitConverter]::ToUInt32($ico, $entry + 8)
    $offset = [BitConverter]::ToUInt32($ico, $entry + 12)
    $expected += , ([byte[]]$ico[$offset..($offset + $size - 1)])
}
$hex = { param($bytes) [BitConverter]::ToString([System.Security.Cryptography.SHA256]::Create().ComputeHash($bytes)) }
$wanted = $expected | ForEach-Object { & $hex $_ }

$found = [ExeIcons]::Images((Resolve-Path $Exe).Path) | ForEach-Object { & $hex $_ }
if (-not $found) { throw "check-icons: $Exe has no icon" }
$foreign = @($found | Where-Object { $wanted -notcontains $_ })
if ($foreign.Count -gt 0) { throw "check-icons: $Exe has $($foreign.Count) icon image(s) not from kiln.ico" }
if (-not $Installer) {
    $missing = @($wanted | Where-Object { $found -notcontains $_ })
    if ($missing.Count -gt 0) { throw "check-icons: $Exe lacks $($missing.Count) of kiln.ico's $count images" }
}
"ok: $Exe carries kiln.ico ($(@($found).Count) of its $count images)"
