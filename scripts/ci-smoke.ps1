# CI smoke test (Windows): starts Kiln on ConPTY with PowerShell (or a WSL distribution), checks
# that a frame with shell output was drawn, and saves a screenshot of the desktop.
param(
    [Parameter(Mandatory)] [string] $Kiln,
    [string] $Wsl = ""
)
$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force smoke | Out-Null

if ($Wsl) {
    # --exec: run sh directly. With "--", wsl.exe passes the command through a second shell
    # that expands $variables before sh sees them.
    $shell = @("wsl.exe", "--distribution", $Wsl, "--exec", "sh", "-c", "echo kiln-smoke; uname -a; sleep 20")
    $name = "wsl"
} else {
    $shell = @("powershell.exe", "-NoLogo", "-NoProfile", "-Command", "Write-Output kiln-smoke; Start-Sleep 20")
    $name = "powershell"
}

# 1. Startup: Kiln quits after the first frame that shows shell output. Release builds are
#    GUI-subsystem apps, so wait for the process and read the result from a file.
$result = Join-Path (Resolve-Path smoke) "startup-$name.txt"
Remove-Item $result -ErrorAction SilentlyContinue
$env:KILN_BENCH_FILE = $result
$quoted = $shell | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ } }
$proc = Start-Process -FilePath $Kiln -ArgumentList (@("--bench-startup", "--cols", "100", "--rows", "30", "--") + $quoted) -PassThru
if (-not $proc.WaitForExit(120000)) { Stop-Process -Id $proc.Id -Force; throw "Kiln did not finish within 2 minutes" }
Remove-Item Env:KILN_BENCH_FILE
$out = Get-Content $result -ErrorAction SilentlyContinue
$out
if (-not ($out -match '^startup_ms=')) { throw "Kiln drew no frame with shell output" }

# 2. Screenshot of a session. Start-Process joins arguments into one command line, so the
#    scripts below avoid double quotes.
if ($Wsl) {
    $script = 'printf ''\033[1mKiln on Windows, WSL\033[0m  ''; uname -sr; echo; for i in 0 1 2 3 4 5 6 7; do printf ''\033[3%sm colour%s \033[9%sm bright%s\033[0m\n'' $i $i $i $i; done; sleep 30'
    $show = @("wsl.exe", "--distribution", $Wsl, "--exec", "sh", "-c", "`"$script`"")
} else {
    $show = @("powershell.exe", "-NoLogo", "-NoProfile", "-Command", "`"`$PSVersionTable.PSVersion; Get-ChildItem; Start-Sleep 30`"")
}
$proc = Start-Process -FilePath $Kiln -ArgumentList (@("--cols", "100", "--rows", "30", "--") + $show) -PassThru
Start-Sleep 10
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
$bitmap.Save("smoke/windows-$name.png")
Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue

# 3. The icon setting: with the Beehive design in slate (in a config of its own; not the default
#    design or colour), the window's big icon is that icon: its top edge is blue-grey (more
#    blue than red), where the default's is orange and black's is warm grey.
if (-not $Wsl) {
    $config = Join-Path ([System.IO.Path]::GetTempPath()) "kiln-smoke-config"
    New-Item -ItemType Directory -Force (Join-Path $config "Kiln") | Out-Null
    Set-Content -Path (Join-Path $config "Kiln\kiln.toml") -Value @('icon = "beehive"', '[icon_colours]', 'beehive = "slate"')
    $appdata = $env:APPDATA
    $env:APPDATA = $config
    $proc = Start-Process -FilePath $Kiln -ArgumentList @("--fresh", "--cols", "80", "--rows", "20", "--", "powershell.exe", "-NoLogo", "-Command", "Start-Sleep 30") -PassThru
    $env:APPDATA = $appdata
    Add-Type -Namespace KilnSmoke -Name Win -MemberDefinition @"
[DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr hwnd, uint message, IntPtr wparam, IntPtr lparam);
"@
    $hwnd = [IntPtr]::Zero
    for ($i = 0; $i -lt 30 -and $hwnd -eq [IntPtr]::Zero; $i++) {
        Start-Sleep 1
        $proc.Refresh()
        $hwnd = $proc.MainWindowHandle
    }
    Start-Sleep 3
    # WM_GETICON (0x7F), ICON_BIG (1)
    $hicon = [KilnSmoke.Win]::SendMessage($hwnd, 0x7F, [IntPtr]1, [IntPtr]::Zero)
    if ($hicon -eq [IntPtr]::Zero) { Stop-Process -Id $proc.Id -Force; throw "Kiln's window has no icon" }
    $icon = [System.Drawing.Icon]::FromHandle($hicon).ToBitmap()
    $icon.Save("smoke/windows-window-icon.png")
    $pixel = $icon.GetPixel([int]($icon.Width / 2), 1)
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    "window icon $($icon.Width) px, top edge pixel R=$($pixel.R) G=$($pixel.G) B=$($pixel.B)"
    if ($pixel.B -le $pixel.R) { throw "the window icon is not Beehive in slate" }
    "ok: the window icon follows icon = `"beehive`" in slate"
}

# 4. The default shell without PowerShell on PATH (a real report: "starting powershell.exe: The
#    system could not find the file specified"). Kiln finds it by its full path: PowerShell 7 in
#    Program Files, and with that hidden too, Windows PowerShell in System32. Each run starts
#    Kiln with no shell named and waits for that shell to run as Kiln's child (Kiln's own prompt
#    prints nothing, so there is no output to wait for).
if (-not $Wsl) {
    $empty = Join-Path ([System.IO.Path]::GetTempPath()) "kiln-smoke-empty"
    New-Item -ItemType Directory -Force $empty | Out-Null
    $strippedPath = (($env:PATH -split ';') | Where-Object { $_ -and $_ -notmatch 'PowerShell' }) -join ';'
    $saved = @{ PATH = $env:PATH; ProgramFiles = $env:ProgramFiles; LOCALAPPDATA = $env:LOCALAPPDATA }
    foreach ($case in @(
        @{ name = "pwsh7"; hide = $false; exe = "pwsh.exe"; expect = 'PowerShell\\7\\pwsh\.exe' },
        @{ name = "windows-powershell"; hide = $true; exe = "powershell.exe"; expect = '(?i)System32\\WindowsPowerShell\\v1\.0\\powershell\.exe' }
    )) {
        $log = Join-Path (Resolve-Path smoke) "default-shell-$($case.name).log"
        Remove-Item $log -ErrorAction SilentlyContinue
        $env:PATH = $strippedPath
        if ($case.hide) { $env:ProgramFiles = $empty; $env:LOCALAPPDATA = $empty }
        $env:RUST_LOG = "kiln_term=info,kiln=info"
        $proc = Start-Process -FilePath (Resolve-Path $Kiln) -ArgumentList @("--fresh", "--cols", "80", "--rows", "20") -RedirectStandardError $log -PassThru
        $env:PATH = $saved.PATH; $env:ProgramFiles = $saved.ProgramFiles; $env:LOCALAPPDATA = $saved.LOCALAPPDATA
        Remove-Item Env:RUST_LOG
        $child = $null
        for ($i = 0; $i -lt 60 -and -not $child; $i++) {
            Start-Sleep 1
            $child = Get-CimInstance Win32_Process -Filter "ParentProcessId = $($proc.Id)" |
                Where-Object { $_.Name -eq $case.exe } | Select-Object -First 1
        }
        Start-Sleep 2
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
        $started = Select-String -Path $log -Pattern 'starting .* arguments' | Select-Object -First 1
        "$($case.name): $($started.Line)"
        if (Select-String -Path $log -Pattern 'could not start the shell' -Quiet) { Get-Content $log; throw "the shell did not start ($($case.name))" }
        if (-not $child) { Get-Content $log; throw "no $($case.exe) ran under Kiln ($($case.name))" }
        "$($case.name): $($child.Name) running, from $($child.ExecutablePath)"
        if (-not ($started.Line -match $case.expect)) { throw "expected $($case.expect) ($($case.name)), got: $($started.Line)" }
        "ok: the default shell starts without PowerShell on PATH ($($case.name))"
    }
}
