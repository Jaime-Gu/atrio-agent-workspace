[CmdletBinding()]
param(
    [string]$ExePath,
    [string]$ExpectedVersion = '0.0.6',
    [string]$OutputPath,
    [switch]$BuildOnly
)

$ErrorActionPreference = 'Stop'
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$manifestPath = Join-Path $PSScriptRoot 'mcp-stdio-fixture\Cargo.toml'
$targetDirectory = Join-Path $repoRoot 'work\mcp-stdio-fixture-target.noindex'

# The harness compiles this checkout's MCP bridge and Windows pipe source. It
# never opens a workspace or invokes a Provider. All build output stays in work.
& cargo build --manifest-path $manifestPath --locked --target x86_64-pc-windows-msvc --target-dir $targetDirectory
if ($LASTEXITCODE -ne 0) { throw "MCP stdio fixture build failed ($LASTEXITCODE)" }
$harnessPath = Join-Path $targetDirectory 'x86_64-pc-windows-msvc\debug\verify-gui-mcp.exe'
if ($BuildOnly) {
    Write-Output $harnessPath
    return
}
if ([string]::IsNullOrWhiteSpace($ExePath)) {
    throw 'Pass -ExePath with the installed or packaged production GUI EXE; use -BuildOnly to compile the harness without running it.'
}
$productionExe = (Resolve-Path -LiteralPath $ExePath).ProviderPath
if (-not (Test-Path -LiteralPath $productionExe -PathType Leaf)) { throw 'Production EXE does not exist.' }
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')
    $OutputPath = Join-Path $repoRoot "work\evidence.noindex\mcp-stdio-$stamp.json"
}
$reportPath = [System.IO.Path]::GetFullPath($OutputPath)
[System.IO.Directory]::CreateDirectory([System.IO.Path]::GetDirectoryName($reportPath)) | Out-Null

# Only the supplied absolute executable path is passed to the child. The harness
# sets its child cwd to that EXE's directory, with no development directory,
# development server, configuration or login dependency.
$rawReport = (& $harnessPath $productionExe $ExpectedVersion | Out-String)
if ($LASTEXITCODE -ne 0) { throw "Production MCP stdio verification failed ($LASTEXITCODE)" }
$report = $rawReport | ConvertFrom-Json
if ($report.status -ne 'PASS' -or $report.checks.Count -ne 11) { throw 'Invalid MCP fixture result.' }
if ($report.providerCalls -ne 0 -or $report.workspaceDatabasesOpened -ne 0) { throw 'This fixture must perform zero Provider calls and open zero workspace databases.' }
$report | Add-Member -NotePropertyName executableSha256 -NotePropertyValue ((Get-FileHash -LiteralPath $productionExe -Algorithm SHA256).Hash.ToLowerInvariant())
$report | Add-Member -NotePropertyName verifiedAtUtc -NotePropertyValue ([DateTime]::UtcNow.ToString('o'))
$report | Add-Member -NotePropertyName rustToolchain -NotePropertyValue ((& rustc --version | Out-String).Trim())
$commit = (& git -C $repoRoot rev-parse HEAD | Out-String).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Unable to record fixture source commit.' }
$report | Add-Member -NotePropertyName fixtureSourceCommit -NotePropertyValue $commit
$sourceHashes = [ordered]@{}
foreach ($relative in @('src-tauri/src/mcp_bridge.rs', 'src-tauri/src/platform/windows/ipc.rs', 'src-tauri/src/platform/windows/ipc_tests.rs', 'scripts/windows/mcp-stdio-fixture/src/verify_gui_mcp.rs')) {
    $sourceHashes[$relative] = (Get-FileHash -LiteralPath (Join-Path $repoRoot $relative) -Algorithm SHA256).Hash.ToLowerInvariant()
}
$report | Add-Member -NotePropertyName fixtureSourceSha256 -NotePropertyValue $sourceHashes
$report | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $reportPath -Encoding utf8
Write-Output "PASS: 11 MCP stdio checks; Provider calls 0; workspace databases opened 0. Evidence: $reportPath"
