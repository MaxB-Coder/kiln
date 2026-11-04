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
#    Kiln with no shell named; --bench-startup quits once the shell reaches its first prompt.
if (-not $Wsl) {
    $empty = Join-Path ([System.IO.Path]::GetTempPath()) "kiln-smoke-empty"
    New-Item -ItemType Directory -Force $empty | Out-Null
    $strippedPath = (($env:PATH -split ';') | Where-Object { $_ -and $_ -notmatch 'PowerShell' }) -join ';'
    foreach ($case in @(
        @{ name = "pwsh7"; hide = $false; expect = 'PowerShell\\7\\pwsh\.exe' },
        @{ name = "windows-powershell"; hide = $true; expect = '(?i)System32\\WindowsPowerShell\\v1\.0\\powershell\.exe' }
    )) {
        $result = Join-Path (Resolve-Path smoke) "default-shell-$($case.name).txt"
        $log = Join-Path (Resolve-Path smoke) "default-shell-$($case.name).log"
        Remove-Item $result, $log -ErrorAction SilentlyContinue
        # The environment set on the process itself: Start-Process rebuilds a child's
        # environment and loses an override such as ProgramFiles.
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = (Resolve-Path $Kiln).Path
        foreach ($arg in @("--bench-startup", "--cols", "80", "--rows", "20")) { $psi.ArgumentList.Add($arg) }
        $psi.UseShellExecute = $false
        $psi.RedirectStandardError = $true
        $psi.Environment["PATH"] = $strippedPath
        # PowerShell 7 hidden: its folders, and (as the runner may still reach it some other way)
        # Kiln's test switch to pass over pwsh.exe.
        if ($case.hide) {
            $psi.Environment["ProgramFiles"] = $empty
            $psi.Environment["LOCALAPPDATA"] = $empty
            $psi.Environment["KILN_TEST_SKIP_SHELLS"] = "pwsh.exe"
        }
        $psi.Environment["KILN_BENCH_FILE"] = $result
        $psi.Environment["RUST_LOG"] = "kiln_term=info,kiln=info"
        $proc = [System.Diagnostics.Process]::Start($psi)
        $stderr = $proc.StandardError.ReadToEndAsync()
        if (-not $proc.WaitForExit(120000)) { $proc.Kill(); $stderr.Result | Set-Content $log; Get-Content $log; throw "Kiln did not finish within 2 minutes ($($case.name))" }
        $stderr.Result | Set-Content $log
        $started = Select-String -Path $log -Pattern 'starting .* arguments' | Select-Object -First 1
        "$($case.name): $((Select-String -Path $log -Pattern 'shell: ' | Select-Object -First 1).Line)"
        "$($case.name): $($started.Line)"
        $out = Get-Content $result -ErrorAction SilentlyContinue
        "$($case.name): $out"
        if (-not ($out -match '^startup_ms=')) { Get-Content $log; throw "the shell reached no prompt ($($case.name))" }
        if (-not ($started.Line -match $case.expect)) { throw "expected $($case.expect) ($($case.name)), got: $($started.Line)" }
        "ok: the default shell reaches a prompt without PowerShell on PATH ($($case.name))"
    }
}

# 5. The sidebar with a real mouse and keyboard (reports from a Windows PC: right-click on a
#    session did nothing; Ctrl+Shift+/ did not open the shortcuts). Input goes through user32 as
#    a person's would; Kiln's log says what happened.
if (-not $Wsl) {
    Add-Type -Namespace KilnInput -Name Win -MemberDefinition @"
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
[DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
[StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
[DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hwnd, ref POINT point);
[DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hwnd);
"@
    $log = Join-Path (Resolve-Path smoke) "sidebar-input.log"
    $env:RUST_LOG = "kiln=info"
    $proc = Start-Process -FilePath (Resolve-Path $Kiln) -ArgumentList @("--fresh", "--cols", "90", "--rows", "24", "--", "powershell.exe", "-NoLogo", "-Command", "Start-Sleep 90") -RedirectStandardError $log -PassThru
    Remove-Item Env:RUST_LOG
    $hwnd = [IntPtr]::Zero
    for ($i = 0; $i -lt 30 -and $hwnd -eq [IntPtr]::Zero; $i++) { Start-Sleep 1; $proc.Refresh(); $hwnd = $proc.MainWindowHandle }
    if ($hwnd -eq [IntPtr]::Zero) { throw "no Kiln window" }
    Start-Sleep 4
    [KilnInput.Win]::SetForegroundWindow($hwnd) | Out-Null
    $scale = [KilnInput.Win]::GetDpiForWindow($hwnd) / 96.0
    # A point in Kiln's window, in logical pixels from its top-left corner.
    $at = {
        param($x, $y)
        $p = New-Object KilnInput.Win+POINT
        $p.X = [int]($x * $scale); $p.Y = [int]($y * $scale)
        [KilnInput.Win]::ClientToScreen($hwnd, [ref]$p) | Out-Null
        [KilnInput.Win]::SetCursorPos($p.X, $p.Y) | Out-Null
        Start-Sleep -Milliseconds 300
    }
    $click = { param($down, $up) [KilnInput.Win]::mouse_event($down, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60; [KilnInput.Win]::mouse_event($up, 0, 0, 0, [UIntPtr]::Zero) }
    $key = { param([byte[]]$keys) foreach ($k in $keys) { [KilnInput.Win]::keybd_event($k, 0, 0, [UIntPtr]::Zero) }; [array]::Reverse($keys); foreach ($k in $keys) { [KilnInput.Win]::keybd_event($k, 0, 2, [UIntPtr]::Zero) }; Start-Sleep -Milliseconds 800 }
    $shot = {
        param($name)
        $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
        $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
        [System.Drawing.Graphics]::FromImage($bitmap).CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
        $bitmap.Save("smoke/$name.png")
    }
    # The terminal first, to give the window the keyboard; then the session's row in the sidebar.
    & $at 500 200; & $click 0x0002 0x0004
    & $at 70 69; & $click 0x0008 0x0010
    Start-Sleep 1; & $shot "windows-sidebar-right-click"
    & $key @(0x1B)
    & $at 70 69; & $click 0x0002 0x0004; Start-Sleep -Milliseconds 80; & $click 0x0002 0x0004
    Start-Sleep 1; & $shot "windows-sidebar-double-click"
    & $key @(0x1B)
    & $at 500 200; & $click 0x0002 0x0004
    & $key @(0x11, 0x10, 0xBF)
    Start-Sleep 1; & $shot "windows-shortcuts-key"
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    Start-Sleep 1
    $lines = Get-Content $log | Select-String -Pattern 'sidebar menu|rename: started|shortcuts: toggled'
    $lines | ForEach-Object { $_.Line }
    if (-not ($lines -match 'sidebar menu: opened')) { throw "a right-click on a session opened no menu" }
    if (-not ($lines -match 'rename: started')) { throw "a double-click on a session started no rename" }
    if (-not ($lines -match 'shortcuts: toggled \(open: true\)')) { throw "Ctrl+Shift+/ did not open the shortcuts" }
    "ok: right-click, double-click and Ctrl+Shift+/ work with real input"
}

# 6. Kiln's flame prompt in PowerShell 7 and Windows PowerShell 5.1 (a report from a Windows PC:
#    the cursor sat under the flame), and the shell's own prompt. Kiln logs where input starts,
#    the cursor's column and the prompt line at each change; "ab" is typed to see where it lands.
if (-not $Wsl) {
    $failures = @()
    foreach ($case in @(
        @{ name = "pwsh7"; skip = ""; prompt = "kiln" },
        @{ name = "windows-powershell"; skip = "pwsh.exe"; prompt = "kiln" },
        @{ name = "windows-powershell-own-prompt"; skip = "pwsh.exe"; prompt = "shell" }
    )) {
        $log = Join-Path (Resolve-Path smoke) "prompt-$($case.name).log"
        $appdata = Join-Path ([System.IO.Path]::GetTempPath()) "kiln-prompt-$($case.name)"
        New-Item -ItemType Directory -Force (Join-Path $appdata "Kiln") | Out-Null
        Set-Content -Path (Join-Path $appdata "Kiln\kiln.toml") -Value @('cursor_blink = false', '[shell]', "prompt = `"$($case.prompt)`"")
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = (Resolve-Path $Kiln).Path
        foreach ($arg in @("--fresh", "--cols", "90", "--rows", "24")) { $psi.ArgumentList.Add($arg) }
        $psi.UseShellExecute = $false
        $psi.RedirectStandardError = $true
        $psi.Environment["APPDATA"] = $appdata
        $psi.Environment["RUST_LOG"] = "kiln=info,kiln_term=info"
        if ($case.skip) { $psi.Environment["KILN_TEST_SKIP_SHELLS"] = $case.skip }
        $proc = [System.Diagnostics.Process]::Start($psi)
        $stderr = $proc.StandardError.ReadToEndAsync()
        $hwnd = [IntPtr]::Zero
        for ($i = 0; $i -lt 30 -and $hwnd -eq [IntPtr]::Zero; $i++) { Start-Sleep 1; $proc.Refresh(); $hwnd = $proc.MainWindowHandle }
        Start-Sleep 6
        [KilnInput.Win]::SetForegroundWindow($hwnd) | Out-Null
        $scale = [KilnInput.Win]::GetDpiForWindow($hwnd) / 96.0
        & $at 500 200; & $click 0x0002 0x0004
        Start-Sleep 1; & $shot "windows-prompt-$($case.name)"
        & $key @(0x41); & $key @(0x42)
        Start-Sleep 1; & $shot "windows-prompt-typed-$($case.name)"
        $proc.Kill()
        $proc.WaitForExit(10000) | Out-Null
        $stderr.Result | Set-Content $log
        $lines = Get-Content $log | Select-String -Pattern 'prompt: ' | ForEach-Object { $_.Line -replace '^\[[^\]]*\] ', '' }
        $lines | ForEach-Object { "$($case.name): $_" }
        if ($case.prompt -eq "kiln") {
            if (-not ($lines -match 'input starts at column 2 ')) { $failures += "$($case.name): input does not start at column 2" }
            if (-not ($lines -match 'cursor at column 2, line ""')) { $failures += "$($case.name): the cursor was never at column 2 on the blank prompt" }
            if (-not ($lines -match 'cursor at column 4, line "  ab"')) { $failures += "$($case.name): typed text is not after the flame" }
        } else {
            if (-not ($lines -match 'line "PS [A-Z]:\\.*>')) { $failures += "$($case.name): no PowerShell prompt shown" }
        }
    }
    if ($failures) { $failures; throw "the prompt is not where Kiln draws it" }
    "ok: the flame prompt and the shell's own prompt in PowerShell 7 and 5.1"
}

# 7. Maximising (a report: after normal → maximised the cursor moved left onto the flame).
#    Windows PowerShell 5.1 with the flame prompt; the window is maximised, restored and
#    maximised again, with Kiln's prompt log and a shot of the input box after each.
if (-not $Wsl) {
    Add-Type -Namespace KilnShow -Name Win -MemberDefinition @"
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int command);
"@
    $log = Join-Path (Resolve-Path smoke) "prompt-maximise.log"
    $appdata = Join-Path ([System.IO.Path]::GetTempPath()) "kiln-prompt-maximise"
    New-Item -ItemType Directory -Force (Join-Path $appdata "Kiln") | Out-Null
    Set-Content -Path (Join-Path $appdata "Kiln\kiln.toml") -Value @('cursor_blink = false', '[window]', 'open = "last"')
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = (Resolve-Path $Kiln).Path
    foreach ($arg in @("--fresh", "--cols", "90", "--rows", "24")) { $psi.ArgumentList.Add($arg) }
    $psi.UseShellExecute = $false
    $psi.RedirectStandardError = $true
    $psi.Environment["APPDATA"] = $appdata
    $psi.Environment["RUST_LOG"] = "kiln=info,kiln_term=info"
    $psi.Environment["KILN_TEST_SKIP_SHELLS"] = "pwsh.exe"
    $proc = [System.Diagnostics.Process]::Start($psi)
    $stderr = $proc.StandardError.ReadToEndAsync()
    $hwnd = [IntPtr]::Zero
    for ($i = 0; $i -lt 30 -and $hwnd -eq [IntPtr]::Zero; $i++) { Start-Sleep 1; $proc.Refresh(); $hwnd = $proc.MainWindowHandle }
    Start-Sleep 6
    [KilnInput.Win]::SetForegroundWindow($hwnd) | Out-Null
    $scale = [KilnInput.Win]::GetDpiForWindow($hwnd) / 96.0
    & $at 500 200; & $click 0x0002 0x0004
    # Long lines above the prompt, wrapped at this width: maximising unwraps them.
    [System.Windows.Forms.SendKeys]::SendWait("1..12 {|} {%} {{}'x' * 150{}}{ENTER}")
    Start-Sleep 3
    Start-Sleep 1; & $shot "windows-resize-0-normal"
    foreach ($step in @(@{ n = 1; cmd = 3; name = "maximised" }, @{ n = 2; cmd = 9; name = "restored" }, @{ n = 3; cmd = 3; name = "maximised-again" })) {
        [KilnShow.Win]::ShowWindow($hwnd, $step.cmd) | Out-Null
        Start-Sleep 3
        & $shot "windows-resize-$($step.n)-$($step.name)"
    }
    $proc.Kill()
    $proc.WaitForExit(10000) | Out-Null
    $stderr.Result | Set-Content $log
    Get-Content $log | Select-String -Pattern 'prompt: ' | ForEach-Object { "maximise: " + ($_.Line -replace '^\[[^\]]*\] ', '') }
}
