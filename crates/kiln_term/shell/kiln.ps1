# Kiln shell integration for PowerShell (pwsh 7 and Windows PowerShell 5.1).
#
# Marks the prompt, the command and its output (OSC 133) and reports the current folder
# (OSC 7). Kiln loads it with -NoExit -EncodedCommand. With KILN_PROMPT=kiln the prompt is two
# blank cells, where Kiln draws its flame (Starship gets a config that prints only those);
# otherwise the user's own prompt stays. Before each prompt the mode is read from
# KILN_PROMPT_FILE (Kiln writes it when the setting changes), so switching applies to open
# sessions too.

if ($global:__KilnIntegrated) { return }
$global:__KilnIntegrated = $true
$global:__KilnRunning = $false
$global:__KilnUserPrompt = $function:prompt
# The user's Starship config, to put back when the shell's own prompt is chosen.
$global:__KilnUserStarship = $env:STARSHIP_CONFIG

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
    if ($env:KILN_PROMPT_FILE -and (Test-Path -LiteralPath $env:KILN_PROMPT_FILE)) {
        $mode = (Get-Content -LiteralPath $env:KILN_PROMPT_FILE -Raw -ErrorAction SilentlyContinue)
        if ($mode) { $mode = $mode.Trim() }
        if ($mode -eq 'kiln' -or $mode -eq 'shell') { $env:KILN_PROMPT = $mode }
    }
    $kiln = $env:KILN_PROMPT -eq 'kiln'
    if ($kiln -and $env:KILN_STARSHIP_CONFIG) {
        $env:STARSHIP_CONFIG = $env:KILN_STARSHIP_CONFIG
    } elseif ($env:KILN_STARSHIP_CONFIG -and $env:STARSHIP_CONFIG -eq $env:KILN_STARSHIP_CONFIG) {
        $env:STARSHIP_CONFIG = $global:__KilnUserStarship
    }
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
    # Runtimes on PATH for the input bar and the rail.
    $tools = ''
    foreach ($t in 'node', 'python3', 'python', 'ruby', 'go', 'rustc', 'java', 'deno', 'bun') {
        $cmd = Get-Command $t -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($cmd) { $tools += "$t=$($cmd.Source);" }
    }
    if ($tools -ne $global:__KilnTools) {
        $global:__KilnTools = $tools
        $out += "$esc]1337;KilnTools=$(__KilnEncode $tools)$bel"
    }
    # Whose prompt follows: Kiln draws the flame only over its own.
    $out += "$esc]1337;KilnPrompt=$(if ($kiln) { 'kiln' } else { 'shell' })$bel"
    $out += "$esc]133;A$bel"
    # Starship's own prompt function, not $env:STARSHIP_SHELL (inherited from wherever Kiln was
    # started).
    $starship = "$global:__KilnUserPrompt" -match 'starship'
    if ($kiln -and -not $starship) {
        $out += '  '
    } else {
        $out += & $global:__KilnUserPrompt
    }
    $out += "$esc]133;B$bel"
    $global:LASTEXITCODE = $code
    $out
}

# PSReadLine reads each command line through PSConsoleHostReadLine. Wrap PSReadLine's own
# function (its ReadLine overloads change between versions) and mark the output start once
# the line is read.
if (Get-Command PSConsoleHostReadLine -ErrorAction SilentlyContinue) {
    $global:__KilnReadLine = ${function:PSConsoleHostReadLine}
    function global:PSConsoleHostReadLine {
        $line = & $global:__KilnReadLine
        if ($line -and $line.Trim().Length -gt 0) {
            $global:__KilnRunning = $true
            [Console]::Write("$([char]27)]133;C;cmdline_url=$(__KilnEncode $line)$([char]7)")
        }
        $line
    }
}
