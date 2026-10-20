[CmdletBinding()]
param(
    [switch]$Repair,
    [string]$Restore,
    [string]$CodexHome,
    [string]$SqliteHome,
    [string]$CodexBin,
    [ValidateRange(5, 60)][int]$TimeoutSeconds = 30
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$enginePath = Join-Path $PSScriptRoot 'repair-codex-startup.py'
if (-not (Test-Path -LiteralPath $enginePath -PathType Leaf)) {
    throw 'Missing repair-codex-startup.py. Extract the complete repair bundle first.'
}
if ($Repair -and $Restore) { throw 'Choose either -Repair or -Restore.' }

$pythonPath = $null
$pythonPrefix = @()
foreach ($candidate in @('py', 'python3', 'python')) {
    $command = Get-Command $candidate -ErrorAction SilentlyContinue
    if (-not $command) { continue }
    # Do not activate Windows Store execution aliases or install dependencies.
    if ($command.Source -like '*\Microsoft\WindowsApps\*') { continue }
    $candidatePrefix = @()
    if ($candidate -eq 'py') { $candidatePrefix = @('-3') }
    $ErrorActionPreference = 'Continue'
    & $command.Source @candidatePrefix -c 'import sys, sqlite3; sys.exit(0 if sys.version_info >= (3, 9) else 1)' 2>$null
    $versionExit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($versionExit -eq 0) {
        $pythonPath = $command.Source
        $pythonPrefix = $candidatePrefix
        break
    }
}
if (-not $pythonPath) {
    throw 'Python 3.9+ with sqlite3 is required. This script does not install software or change your config.'
}

$engineArguments = @($enginePath, '--timeout', [string]$TimeoutSeconds)
if ($Repair) { $engineArguments += '--repair' }
elseif ($Restore) { $engineArguments += @('--restore', $Restore) }
else { $engineArguments += '--diagnose' }
if ($CodexHome) { $engineArguments += @('--codex-home', $CodexHome) }
if ($SqliteHome) { $engineArguments += @('--sqlite-home', $SqliteHome) }
if ($CodexBin) { $engineArguments += @('--codex-bin', $CodexBin) }

$ErrorActionPreference = 'Continue'
& $pythonPath @pythonPrefix @engineArguments 2>&1 | ForEach-Object { $_.ToString() }
$engineExit = $LASTEXITCODE
exit $engineExit
