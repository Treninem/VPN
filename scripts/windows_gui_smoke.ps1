param(
    [Parameter(Mandatory = $true)]
    [string]$ExePath,

    [Parameter(Mandatory = $true)]
    [string]$DiagnosticExePath,

    [ValidateRange(1, 30)]
    [int]$WaitSeconds = 8
)

$ErrorActionPreference = "Stop"

function Stop-AmriProcess([System.Diagnostics.Process]$Process) {
    $Process.Refresh()
    if (-not $Process.HasExited) {
        Stop-Process -Id $Process.Id -Force
        $Process.WaitForExit()
    }
}

$releaseExe = (Resolve-Path $ExePath).Path
$diagnosticExe = (Resolve-Path $DiagnosticExePath).Path

$releaseProcess = Start-Process -FilePath $releaseExe -PassThru
Start-Sleep -Seconds $WaitSeconds
$releaseProcess.Refresh()

if (-not $releaseProcess.HasExited) {
    Stop-AmriProcess $releaseProcess
    Write-Host "AMRI Windows GUI remained alive for $WaitSeconds seconds."
    return
}

$releaseExitCode = $releaseProcess.ExitCode
$token = [Guid]::NewGuid().ToString("N")
$stdout = Join-Path $env:RUNNER_TEMP "amri-gui-smoke-$token.stdout.txt"
$stderr = Join-Path $env:RUNNER_TEMP "amri-gui-smoke-$token.stderr.txt"

try {
    $diagnosticProcess = Start-Process `
        -FilePath $diagnosticExe `
        -PassThru `
        -RedirectStandardOutput $stdout `
        -RedirectStandardError $stderr

    Start-Sleep -Seconds $WaitSeconds
    $diagnosticProcess.Refresh()

    if (-not $diagnosticProcess.HasExited) {
        Stop-AmriProcess $diagnosticProcess
        throw "release GUI exited with code $releaseExitCode while the diagnostic GUI remained alive"
    }

    $diagnosticExitCode = $diagnosticProcess.ExitCode
    $stderrText = if (Test-Path $stderr) { (Get-Content $stderr -Raw).Trim() } else { "" }
    $knownHostedRunnerError = 'Error: OpenGL(PainterError("egui_glow requires opengl 2.0+. "))'

    if (
        $releaseExitCode -eq 1 -and
        $diagnosticExitCode -eq 1 -and
        $stderrText -eq $knownHostedRunnerError
    ) {
        Write-Warning "GitHub hosted Windows runner has no usable OpenGL 2.0 context. AMRI reached eframe renderer initialization; graphical liveness is environment-limited and still requires real-desktop E2E."
        return
    }

    throw "AMRI Windows startup failed: release exit=$releaseExitCode diagnostic exit=$diagnosticExitCode (not the exact known hosted-runner OpenGL limitation)"
}
finally {
    Remove-Item $stdout, $stderr -Force -ErrorAction SilentlyContinue
}
