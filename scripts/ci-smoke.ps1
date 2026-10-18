# CI smoke test (Windows): starts Kiln on ConPTY with PowerShell (or a WSL distribution), checks
# that a frame with shell output was drawn, and saves a screenshot of the desktop.
param(
    [Parameter(Mandatory)] [string] $Kiln,
    [string] $Wsl = ""
)
$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force smoke | Out-Null

if ($Wsl) {
    $shell = @("wsl.exe", "--distribution", $Wsl, "--", "sh", "-c", "echo kiln-smoke; uname -a; sleep 20")
    $name = "wsl"
} else {
    $shell = @("powershell.exe", "-NoLogo", "-NoProfile", "-Command", "Write-Output kiln-smoke; Start-Sleep 20")
    $name = "powershell"
}

# 1. Startup: Kiln quits after the first frame that shows shell output.
$out = & $Kiln --bench-startup --cols 100 --rows 30 -- @shell
$out
if (-not ($out -match '^startup_ms=')) { throw "Kiln drew no frame with shell output" }
$out | Out-File "smoke/startup-$name.txt"

# 2. Screenshot of a session.
if ($Wsl) {
    $show = @("wsl.exe", "--distribution", $Wsl, "--", "sh", "-c", 'printf "\033[1mKiln on Windows, WSL\033[0m  $(uname -sr)\n\n"; for i in 0 1 2 3 4 5 6 7; do printf "\033[3${i}m colour$i \033[9${i}m bright$i\033[0m\n"; done; sleep 30')
} else {
    $show = @("powershell.exe", "-NoLogo", "-NoProfile", "-Command", '$PSVersionTable.PSVersion; Get-ChildItem; Start-Sleep 30')
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
