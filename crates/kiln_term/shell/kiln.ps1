# Kiln shell integration for PowerShell (pwsh 7 and Windows PowerShell 5.1).
#
# Marks the prompt, the command and its output (OSC 133) and reports the current folder
# (OSC 7). Kiln loads it with -NoExit -EncodedCommand. With KILN_PROMPT=kiln the prompt is
# Kiln's minimal "❯ " (">" where the console is not UTF-8; Starship gets a one-character config
# instead); otherwise the user's own prompt stays.

if ($global:__KilnIntegrated) { return }
$global:__KilnIntegrated = $true
$global:__KilnRunning = $false
# Whether the last command succeeded, for the prompt's colour (the profile does not count).
$global:__KilnLastOk = $true
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
    $kiln = $env:KILN_PROMPT -eq 'kiln'
    if ($kiln -and $env:KILN_STARSHIP_CONFIG) { $env:STARSHIP_CONFIG = $env:KILN_STARSHIP_CONFIG }
    $esc = [char]27
    $bel = [char]7
    $out = ''
    if ($global:__KilnRunning) {
        $exit = if ($ok) { 0 } elseif ($code) { $code } else { 1 }
        $global:__KilnLastOk = $ok
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
    $out += "$esc]133;A$bel"
    if ($kiln -and -not $env:STARSHIP_SHELL) {
        $good = $global:__KilnLastOk
        $rgb = if ($good) { $env:KILN_PROMPT_OK } else { $env:KILN_PROMPT_FAIL }
        if (-not $rgb) { $rgb = if ($good) { '224;123;57' } else { '184;100;90' } }
        $sym = if ([Console]::OutputEncoding.CodePage -eq 65001) { [string][char]0x276F } else { '>' }
        $out += "$esc[38;2;${rgb}m$sym$esc[0m "
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
