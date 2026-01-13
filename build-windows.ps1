# Discord Quest Helper - Build Script
# This script builds and packages the portable version of the application

param(
    [switch]$SkipRunnerBuild,
    [switch]$SkipTauriBuild
)

$ErrorActionPreference = "Stop"

# Get script directory (project root)
$ProjectRoot = $PSScriptRoot
Set-Location $ProjectRoot

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  Discord Quest Helper Build Script" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# Read version from public/version.txt
$VersionFile = Join-Path $ProjectRoot "public\version.txt"
if (-not (Test-Path $VersionFile)) {
    Write-Host "Error: Version file not found at $VersionFile" -ForegroundColor Red
    exit 1
}
$Version = (Get-Content $VersionFile -Raw).Trim()
Write-Host "Version: $Version" -ForegroundColor Green
Write-Host ""

# Define paths
$SrcRunner = Join-Path $ProjectRoot "src-runner"
$SrcTauri = Join-Path $ProjectRoot "src-tauri"
$ReleaseDir = Join-Path $SrcTauri "target\release"
$OutputZip = Join-Path $ProjectRoot "discord-quest-helper-v$Version.zip"
$TempDir = Join-Path $ProjectRoot "build-temp"

# Step 1: Build src-runner
if (-not $SkipRunnerBuild) {
    Write-Host "[1/4] Building src-runner..." -ForegroundColor Yellow
    Push-Location $SrcRunner
    try {
        cargo build --release
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to build src-runner"
        }
        Write-Host "  src-runner build complete." -ForegroundColor Green
    }
    finally {
        Pop-Location
    }
} else {
    Write-Host "[1/4] Skipping src-runner build (--SkipRunnerBuild)" -ForegroundColor DarkGray
}

# Step 2: Build Tauri app
if (-not $SkipTauriBuild) {
    Write-Host "[2/4] Building Tauri application..." -ForegroundColor Yellow
    pnpm tauri:build
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to build Tauri application"
    }
    Write-Host "  Tauri build complete." -ForegroundColor Green
} else {
    Write-Host "[2/4] Skipping Tauri build (--SkipTauriBuild)" -ForegroundColor DarkGray
}

# Step 3: Prepare portable package
Write-Host "[3/4] Preparing portable package..." -ForegroundColor Yellow

# Clean up temp directory if exists
if (Test-Path $TempDir) {
    Remove-Item -Recurse -Force $TempDir
}

# Create directory structure
$PackageDir = Join-Path $TempDir "discord-quest-helper"
New-Item -ItemType Directory -Force -Path $PackageDir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $PackageDir "data") | Out-Null

# Copy files
$MainExe = Join-Path $ReleaseDir "discord-quest-helper.exe"
$DataDir = Join-Path $ReleaseDir "data"

if (-not (Test-Path $MainExe)) {
    throw "Main executable not found: $MainExe"
}

Write-Host "  Copying discord-quest-helper.exe..." -ForegroundColor DarkGray
Copy-Item $MainExe -Destination $PackageDir

if (Test-Path $DataDir) {
    Write-Host "  Copying data directory..." -ForegroundColor DarkGray
    Copy-Item -Path (Join-Path $DataDir "*") -Destination (Join-Path $PackageDir "data") -Recurse
}

Write-Host "  Package structure prepared." -ForegroundColor Green

# Step 4: Create ZIP archive
Write-Host "[4/4] Creating ZIP archive..." -ForegroundColor Yellow

# Remove existing zip if present
if (Test-Path $OutputZip) {
    Remove-Item -Force $OutputZip
}

# Create zip
Compress-Archive -Path $PackageDir -DestinationPath $OutputZip -Force

# Clean up temp directory
Remove-Item -Recurse -Force $TempDir

Write-Host "  ZIP archive created." -ForegroundColor Green
Write-Host ""

# Summary
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  Build Complete!" -ForegroundColor Green
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "Output: $OutputZip" -ForegroundColor White
Write-Host "Size:   $([math]::Round((Get-Item $OutputZip).Length / 1MB, 2)) MB" -ForegroundColor White
Write-Host ""

# List package contents
Write-Host "Package contents:" -ForegroundColor Yellow
$TempExtract = Join-Path $env:TEMP "dqh-verify-$(Get-Random)"
Expand-Archive -Path $OutputZip -DestinationPath $TempExtract -Force
Get-ChildItem -Path $TempExtract -Recurse | ForEach-Object {
    $relativePath = $_.FullName.Replace($TempExtract, "").TrimStart("\")
    if ($_.PSIsContainer) {
        Write-Host "  [DIR]  $relativePath" -ForegroundColor DarkGray
    } else {
        $sizeKB = [math]::Round($_.Length / 1KB, 1)
        Write-Host "  [FILE] $relativePath ($sizeKB KB)" -ForegroundColor White
    }
}
Remove-Item -Recurse -Force $TempExtract

Write-Host ""
Write-Host "Done!" -ForegroundColor Green
