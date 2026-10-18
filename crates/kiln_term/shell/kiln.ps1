# Kiln shell integration for PowerShell (pwsh 7 and Windows PowerShell 5.1).
#
# Marks the prompt, the command and its output (OSC 133) and reports the current folder
# (OSC 7). Kiln loads it with -NoExit -Command; it keeps the user's own prompt.

if ($global:__KilnIntegrated) { return }
$global:__KilnIntegrated = $true
$global:__KilnRunning = $false
$global:__KilnUserPrompt = $function:prompt

function global:__KilnEncode([string] $s) {
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($s)
    $sb = [System.Text.StringBuilder]::new()
    foreach ($b in $bytes) {
        $c = [char]$b
        if (($b -lt 128) -and ([char]::IsLetterOrDigit($c) -or '/._~-'.Contains($c))) {
            [void]$sb.Append($c)
        } else {
            [void]$sb.Append(('%{0:X2}' -f $b))
        }
    }
    $sb.ToString()
}

function global:prompt {
    $ok = $?
    $code = $global:LASTEXITCODE
    $esc = [char]27
    $bel = [char]7
    $out = ''
    if ($global:__KilnRunning) {
        $exit = if ($ok) { 0 } elseif ($code) { $code } else { 1 }
        $out += "$esc]133;D;$exit$bel"
        $global:__KilnRunning = $false
    }
    $loc = $executionContext.SessionState.Path.CurrentLocation
    if ($loc.Provider.Name -eq 'FileSystem') {
        $path = $loc.ProviderPath -replace '\\', '/'
        if (-not $path.StartsWith('/')) { $path = '/' + $path }
        $out += "$esc]7;file://$([Environment]::MachineName)$(__KilnEncode $path)$bel"
    }
    $out += "$esc]133;A$bel"
    $out += & $global:__KilnUserPrompt
    $out += "$esc]133;B$bel"
    $global:LASTEXITCODE = $code
    $out
}

# PSReadLine calls this to read each command line; mark the output start once it returns.
if (Get-Module -Name PSReadLine) {
    function global:PSConsoleHostReadLine {
        $line = [Microsoft.PowerShell.PSConsoleReadLine]::ReadLine($Host.Runspace, $ExecutionContext)
        if ($line.Trim().Length -gt 0) {
            $global:__KilnRunning = $true
            [Console]::Write("$([char]27)]133;C;cmdline_url=$(__KilnEncode $line)$([char]7)")
        }
        $line
    }
}
