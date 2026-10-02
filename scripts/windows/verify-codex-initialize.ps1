[CmdletBinding()]
param(
    [string]$CliPath,
    [string]$RuntimeDirectory,
    [string]$ReportPath,
    [switch]$BuildOnly
)
$ErrorActionPreference = 'Stop'
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$targetDirectory = Join-Path $repoRoot 'work\codex-initialize-fixture-target.noindex'
$manifest = Join-Path $PSScriptRoot 'codex-initialize-fixture\Cargo.toml'
& cargo build --manifest-path $manifest --locked --target x86_64-pc-windows-msvc --target-dir $targetDirectory
if ($LASTEXITCODE -ne 0) { throw "Initialize-only fixture build failed ($LASTEXITCODE)" }
$harness = Join-Path $targetDirectory 'x86_64-pc-windows-msvc\debug\verify-codex-initialize.exe'
if ($BuildOnly) { Write-Output $harness; return }
if ([string]::IsNullOrWhiteSpace($CliPath)) { throw 'Pass -CliPath with the official local Codex EXE absolute path. No credentials are accepted by this fixture.' }
$cli = (Resolve-Path -LiteralPath $CliPath).ProviderPath
if ([string]::IsNullOrWhiteSpace($RuntimeDirectory)) { $RuntimeDirectory = Join-Path $repoRoot 'work\resources.noindex\agents\codex' }
$runtime = (Resolve-Path -LiteralPath $RuntimeDirectory).ProviderPath
$runtimeLockPath = Join-Path $repoRoot 'scripts\runtime\codex-runtime.windows-x64.lock.json'
$runtimeLock = Get-Content -LiteralPath $runtimeLockPath -Raw | ConvertFrom-Json
$runtimeManifestPath = Join-Path $runtime 'manifest.json'
$manifestHash = (Get-FileHash -LiteralPath $runtimeManifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($manifestHash -ne $runtimeLock.manifestSha256) { throw 'Runtime manifest differs from the Windows lock.' }
foreach ($entry in $runtimeLock.files) {
    $filePath = Join-Path $runtime $entry.path
    $item = Get-Item -LiteralPath $filePath
    if ($item.Length -ne $entry.bytes -or (Get-FileHash -LiteralPath $filePath -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.sha256) {
        throw "Runtime lock mismatch: $($entry.path)"
    }
}
$stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')
$isolation = Join-Path $repoRoot "work\evidence.noindex\codex-initialize-isolated-$stamp"
if ([string]::IsNullOrWhiteSpace($ReportPath)) { $ReportPath = Join-Path $repoRoot "work\evidence.noindex\codex-initialize-$stamp.json" }
$reportFile = [System.IO.Path]::GetFullPath($ReportPath)
[System.IO.Directory]::CreateDirectory([System.IO.Path]::GetDirectoryName($reportFile)) | Out-Null
$priorCommit = [Environment]::GetEnvironmentVariable('ATRIO_FIXTURE_SOURCE_COMMIT', 'Process')
try {
    $env:ATRIO_FIXTURE_SOURCE_COMMIT = (& git -C $repoRoot rev-parse HEAD | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Cannot record source commit.' }
    # No session, prompt, model, authentication or Host tool request is sent.
    # The Rust harness sets fresh CODEX_HOME/HOME/USERPROFILE and a private Job.
    $raw = (& $harness $runtime $cli $isolation | Out-String)
    $fixtureExit = $LASTEXITCODE
    if ([string]::IsNullOrWhiteSpace($raw)) { throw "Initialize-only fixture failed before a report ($fixtureExit)." }
    $report = $raw | ConvertFrom-Json
    $report | Add-Member -NotePropertyName runtimeLockSha256 -NotePropertyValue ((Get-FileHash -LiteralPath $runtimeLockPath -Algorithm SHA256).Hash.ToLowerInvariant())
    $report | Add-Member -NotePropertyName runtimeManifestSha256 -NotePropertyValue $manifestHash
    $report | Add-Member -NotePropertyName cliSha256 -NotePropertyValue ((Get-FileHash -LiteralPath $cli -Algorithm SHA256).Hash.ToLowerInvariant())
    $report | Add-Member -NotePropertyName processSourceSha256 -NotePropertyValue ((Get-FileHash -LiteralPath (Join-Path $repoRoot 'src-tauri\src\platform\windows\process.rs') -Algorithm SHA256).Hash.ToLowerInvariant())
    $report | Add-Member -NotePropertyName fixtureSourceSha256 -NotePropertyValue ((Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'codex-initialize-fixture\src\main.rs') -Algorithm SHA256).Hash.ToLowerInvariant())
    $report | Add-Member -NotePropertyName completedAtUtc -NotePropertyValue ([DateTime]::UtcNow.ToString('o'))
    $report | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $reportFile -Encoding utf8
    if ($fixtureExit -ne 0 -or $report.status -ne 'PASS') { throw "Actual ACP initialize failed; evidence preserved: $reportFile" }
    if ($report.modelCalls -ne 0 -or $report.newSessionRequests -ne 0 -or $report.promptRequests -ne 0 -or $report.fileToolCalls -ne 0 -or -not $report.cleanup.confirmed) { throw 'Unexpected fixture scope or unconfirmed process cleanup.' }
    Write-Output "PASS: actual ACP initialize only; model/session/prompt/file calls 0; owned Job reaped. Evidence: $reportFile"
} finally {
    [Environment]::SetEnvironmentVariable('ATRIO_FIXTURE_SOURCE_COMMIT', $priorCommit, 'Process')
}