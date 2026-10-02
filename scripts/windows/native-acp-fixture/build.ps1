[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
if (-not $IsWindows -or [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'X64') {
    throw 'This synthetic acceptance fixture targets Windows x64 only.'
}
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$manifestPath = Join-Path $PSScriptRoot 'Cargo.toml'
$targetDirectory = Join-Path $repoRoot 'work\native-acp-fixture-target.noindex'
& cargo build --manifest-path $manifestPath --locked --release --target x86_64-pc-windows-msvc --target-dir $targetDirectory
if ($LASTEXITCODE -ne 0) { throw "Synthetic ACP fixture build failed ($LASTEXITCODE)" }
$fixturePath = Join-Path $targetDirectory 'x86_64-pc-windows-msvc\release\windows-acp-fixture.exe'
& $fixturePath --version
if ($LASTEXITCODE -ne 0) { throw 'Synthetic ACP fixture version probe failed.' }
& $fixturePath acp --check
if ($LASTEXITCODE -ne 0) { throw 'Synthetic ACP fixture dependency probe failed.' }
Write-Output $fixturePath
