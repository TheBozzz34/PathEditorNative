[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [switch]$SkipInstaller
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$cargoToml = Join-Path $root "Cargo.toml"
if (-not (Test-Path $cargoToml)) {
    throw "Cargo.toml not found at $cargoToml"
}

$versionMatch = Select-String -Path $cargoToml -Pattern '^\s*version\s*=\s*"([^"]+)"' | Select-Object -First 1
$appVersion = if ($versionMatch -and $versionMatch.Matches.Count -gt 0) {
    $versionMatch.Matches[0].Groups[1].Value
} else {
    "0.1.0"
}

$crateExe = Join-Path $root "target\release\path_editor_native.exe"
$distRoot = Join-Path $root "dist"
$portableDir = Join-Path $distRoot "PathEditorNative"
$portableExe = Join-Path $portableDir "PathEditorNative.exe"
$zipPath = Join-Path $distRoot "PathEditorNative-$appVersion-win64.zip"
$shaPath = "$zipPath.sha256.txt"

Write-Host "Packaging PathEditorNative v$appVersion" -ForegroundColor Cyan

if (-not $SkipBuild) {
    Write-Host "Building release binary..." -ForegroundColor Yellow
    cargo build --release
}

if (-not (Test-Path $crateExe)) {
    throw "Release executable not found at $crateExe"
}

Write-Host "Creating portable folder..." -ForegroundColor Yellow
New-Item -ItemType Directory -Force -Path $portableDir | Out-Null
Copy-Item -Path $crateExe -Destination $portableExe -Force
Copy-Item -Path (Join-Path $root "packaging\PORTABLE_README.txt") -Destination (Join-Path $portableDir "README.txt") -Force

if (Test-Path $zipPath) {
    Remove-Item -Path $zipPath -Force
}
if (Test-Path $shaPath) {
    Remove-Item -Path $shaPath -Force
}

Write-Host "Creating zip package..." -ForegroundColor Yellow
Compress-Archive -Path "$portableDir\*" -DestinationPath $zipPath -CompressionLevel Optimal

$zipHash = (Get-FileHash -Path $zipPath -Algorithm SHA256).Hash
@(
    "File: $(Split-Path -Leaf $zipPath)"
    "SHA256: $zipHash"
) | Set-Content -Path $shaPath -Encoding utf8

Write-Host "Portable package created:" -ForegroundColor Green
Write-Host "  $zipPath"
Write-Host "  $shaPath"

if ($SkipInstaller) {
    Write-Host "Installer build skipped by flag." -ForegroundColor DarkYellow
    exit 0
}

$isccCmd = Get-Command iscc -ErrorAction SilentlyContinue
if (-not $isccCmd) {
    $fallbacks = @(
        "${Env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$Env:ProgramFiles\Inno Setup 6\ISCC.exe"
    )
    foreach ($candidate in $fallbacks) {
        if (Test-Path $candidate) {
            $isccCmd = @{ Source = $candidate }
            break
        }
    }
}

if (-not $isccCmd) {
    Write-Warning "Inno Setup compiler (ISCC.exe) not found. Install Inno Setup 6 and re-run scripts\\package.ps1."
    exit 0
}

$issFile = Join-Path $root "installer\path_editor_native.iss"
if (-not (Test-Path $issFile)) {
    throw "Installer script not found at $issFile"
}

Write-Host "Building installer with Inno Setup..." -ForegroundColor Yellow
& $isccCmd.Source "/DAppVersion=$appVersion" $issFile

Write-Host "Installer build complete. Output is in dist\\." -ForegroundColor Green
