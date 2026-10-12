# Offline native startup only. No provider, real settings, or saved sessions.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $Executable,
    [string] $OutputDirectory,
    [ValidateRange(10, 120)]
    [int] $TimeoutSeconds = 45
)

$ErrorActionPreference = "Stop"
if (-not $IsWindows) {
    throw "Run this smoke test with PowerShell 7 on Windows."
}
$binary = (Resolve-Path -LiteralPath $Executable).Path
if (-not $OutputDirectory) {
    $OutputDirectory = Join-Path ([System.IO.Path]::GetTempPath()) "hooter-smoke-$([guid]::NewGuid())"
}
# Each run gets a new child directory. Existing output and user data stay intact.
$run = Join-Path $OutputDirectory ([guid]::NewGuid().ToString())
$null = New-Item -ItemType Directory -Path $run -Force
$run = (Resolve-Path -LiteralPath $run).Path
$workspace = Join-Path $run "workspace"
$null = New-Item -ItemType Directory -Path $workspace
$environment = @{
    FLINT_HOME = (Join-Path $run "home")
    FLINT_BP_STATE = (Join-Path $run "state.json")
    FLINT_BP_TIMING = (Join-Path $run "timing.json")
    FLINT_BP_NO_ACTIVATE = "1"
    FLINT_LIVE = "0"
    FLINT_EVAL = "0"
}
$previous = @{}
$process = $null
try {
    foreach ($entry in $environment.GetEnumerator()) {
        $previous[$entry.Key] = [Environment]::GetEnvironmentVariable($entry.Key, "Process")
        [Environment]::SetEnvironmentVariable($entry.Key, $entry.Value, "Process")
    }
    $process = Start-Process -FilePath $binary `
        -ArgumentList "--demo", "--demo-instant", "--exit-after-turn", "--workspace", "`"$workspace`"" `
        -WorkingDirectory $workspace `
        -RedirectStandardOutput (Join-Path $run "stdout.log") `
        -RedirectStandardError (Join-Path $run "stderr.log") -PassThru
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
        throw "Desktop startup did not finish within $TimeoutSeconds seconds. Evidence: $run"
    }
    $process.WaitForExit()
    if ($process.ExitCode -ne 0) {
        throw "Desktop exited with code $($process.ExitCode). Evidence: $run"
    }
    $state = Get-Content -LiteralPath $environment.FLINT_BP_STATE -Raw | ConvertFrom-Json
    $timing = Get-Content -LiteralPath $environment.FLINT_BP_TIMING -Raw | ConvertFrom-Json
    if (-not $state.sessions -or -not $state.sessions[0].turns -or
        $state.sessions[0].turns[-1].end_reason -ne "Completed") {
        throw "The offline demo did not complete. Evidence: $run"
    }
    if (-not $timing -or $timing.first_frame_epoch_ms -le 0 -or
        $timing.main_to_first_frame_ms -lt 0) {
        throw "No first-frame rendering evidence was recorded. Evidence: $run"
    }
    [pscustomobject]@{
        passed = $true
        executable = $binary
        exit_code = $process.ExitCode
        completed_demo = $true
        rendered_first_frame = $true
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $run "result.json")
    Write-Output "Offline Windows desktop smoke test passed. Evidence: $run"
} finally {
    if ($process -and -not $process.HasExited) {
        # Stop only the process started by this test, never other Hooter instances.
        $process.Kill($true)
        $process.WaitForExit()
    }
    foreach ($entry in $previous.GetEnumerator()) {
        [Environment]::SetEnvironmentVariable($entry.Key, $entry.Value, "Process")
    }
}
