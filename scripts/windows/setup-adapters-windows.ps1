[CmdletBinding()]
param(
    [string]$DependencySource,
    [string]$InstallDirectory
)
$ErrorActionPreference = 'Stop'
if (-not $DependencySource) { $DependencySource = Join-Path $PSScriptRoot 'adapter' }
if (-not $InstallDirectory) { $InstallDirectory = Join-Path $env:LOCALAPPDATA 'Atrio\acp-adapters\0.0.6' }

$dependencyRoot = (Resolve-Path -LiteralPath $DependencySource).Path
$destination = [IO.Path]::GetFullPath($InstallDirectory)
if (-not [IO.Path]::IsPathRooted($destination)) { throw 'Adapter installation directory must be absolute.' }
foreach ($name in @('package.json', 'package-lock.json')) {
    if (-not (Test-Path -LiteralPath (Join-Path $dependencyRoot $name) -PathType Leaf)) {
        throw "Missing adapter dependency manifest: $name"
    }
}
$manifest = Get-Content -LiteralPath (Join-Path $dependencyRoot 'package.json') -Raw | ConvertFrom-Json
if ($manifest.dependencies.'@agentclientprotocol/codex-acp' -ne '1.13.1') {
    throw 'Adapter dependency versions differ from the 0.0.6 Windows pins.'
}

$nodePath = (Get-Command node.exe -ErrorAction Stop).Source
$npmCli = Join-Path (Split-Path -Parent $nodePath) 'node_modules\npm\bin\npm-cli.js'
if (-not (Test-Path -LiteralPath $npmCli -PathType Leaf)) {
    throw 'npm-cli.js was not found beside node.exe. Install a Windows Node distribution that includes npm.'
}
$nodeMajor = [int]((& $nodePath --version).TrimStart('v').Split('.')[0])
if ($nodeMajor -lt 22) { throw 'The pinned official ACP adapters require Node 22 or newer.' }

New-Item -ItemType Directory -Path $destination -Force | Out-Null
foreach ($name in @('package.json', 'package-lock.json')) {
    Copy-Item -LiteralPath (Join-Path $dependencyRoot $name) -Destination (Join-Path $destination $name) -Force
}
Push-Location -LiteralPath $destination
try {
    & $nodePath $npmCli ci --no-audit --no-fund
    if ($LASTEXITCODE -ne 0) { throw "npm ci failed with exit code $LASTEXITCODE" }
    foreach ($package in @('@agentclientprotocol/codex-acp')) {
        $installed = Get-Content -LiteralPath (Join-Path $destination "node_modules/$package/package.json") -Raw | ConvertFrom-Json
        $expected = $manifest.dependencies.$package
        if ($installed.version -ne $expected) { throw "Installed version differs for $package" }
    }
} finally { Pop-Location }

[pscustomobject]@{
    installDirectory = $destination
    nodeExecutable = $nodePath
    codexAdapterVersion = '1.13.1'
    packageLockSha256 = (Get-FileHash -LiteralPath (Join-Path $destination 'package-lock.json') -Algorithm SHA256).Hash.ToLowerInvariant()
    authentication = 'Not changed; provider login is performed separately by the user.'
    note = 'Atrio discovers the default per-user path automatically; custom paths require PIXEL_ACP_ADAPTER_DIR.'
} | ConvertTo-Json
