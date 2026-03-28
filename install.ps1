#Requires -Version 5.1
<#
.SYNOPSIS
    Install cship - Claude Code statusline tool for Windows.
.DESCRIPTION
    Downloads the cship binary from GitHub Releases, installs it to
    %LOCALAPPDATA%\Programs\cship\, writes a default cship.toml, and
    registers the statusline in Claude Code's settings.json.
#>
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$REPO    = "stephenleo/cship"
$INSTALL_DIR = Join-Path $env:USERPROFILE ".local\bin"
$BIN     = Join-Path $INSTALL_DIR "cship.exe"
$CONFIG_DIR  = Join-Path $env:USERPROFILE ".config"
$CONFIG_FILE = Join-Path $CONFIG_DIR "cship.toml"
$SETTINGS    = Join-Path $env:USERPROFILE ".claude\settings.json"

# --- Arch detection ---
$arch = $env:PROCESSOR_ARCHITECTURE
# WOW64: 32-bit PowerShell on 64-bit OS reports x86; check redirection variable
if ($arch -eq "x86" -and $env:PROCESSOR_ARCHITEW6432) {
    $arch = $env:PROCESSOR_ARCHITEW6432
}
if ($arch -eq "AMD64") {
    $TARGET = "x86_64-pc-windows-msvc"
} elseif ($arch -eq "ARM64") {
    $TARGET = "aarch64-pc-windows-msvc"
} else {
    Write-Error "Unsupported architecture: $arch"
    exit 1
}

# --- Fetch latest release tag ---
Write-Host "Fetching latest cship release..."
$releaseUrl = "https://api.github.com/repos/$REPO/releases/latest"
$release = Invoke-RestMethod -Uri $releaseUrl -UseBasicParsing
$tag = $release.tag_name
$assetName = "cship-$TARGET.exe"
$downloadUrl = $release.assets |
    Where-Object { $_.name -eq $assetName } |
    Select-Object -ExpandProperty browser_download_url

if (-not $downloadUrl) {
    Write-Error "Asset '$assetName' not found in release $tag. Available assets:`n$($release.assets.name -join "`n")"
    exit 1
}

# --- Download ---
Write-Host "Downloading $assetName ($tag)..."
New-Item -ItemType Directory -Force -Path $INSTALL_DIR | Out-Null
Invoke-WebRequest -Uri $downloadUrl -OutFile $BIN -UseBasicParsing
Write-Host "Installed to: $BIN"

# --- Ensure ~/.local/bin is on PATH ---
$currentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($currentPath -notlike "*$INSTALL_DIR*") {
    [Environment]::SetEnvironmentVariable(
        "PATH",
        "$currentPath;$INSTALL_DIR",
        "User"
    )
    $env:PATH += ";$INSTALL_DIR"
    Write-Host "Added $INSTALL_DIR to your user PATH (effective in new shells)."
}

# --- Write default cship.toml ---
if (-not (Test-Path $CONFIG_FILE)) {
    New-Item -ItemType Directory -Force -Path $CONFIG_DIR | Out-Null
    @'
[cship]
lines = ["$cship.model $cship.cost"]

[cship.model]
disabled = false

[cship.cost]
disabled = false
'@ | Set-Content -Path $CONFIG_FILE -Encoding UTF8
    Write-Host "Config written to: $CONFIG_FILE"
} else {
    Write-Host "Config already exists at $CONFIG_FILE - skipping."
}

# --- Register statusline in Claude Code settings.json ---
$claudeDir = Split-Path $SETTINGS
if (-not (Test-Path $claudeDir)) {
    Write-Host "Claude Code settings directory not found at $claudeDir - skipping settings update."
    Write-Host "Authenticate in Claude Code first, then re-run this script."
} elseif (-not (Test-Path $SETTINGS)) {
    # Create minimal settings.json
    New-Item -ItemType Directory -Force -Path $claudeDir | Out-Null
    '{"statusLine": {"type": "command", "command": "cship"}}' | Set-Content -Path $SETTINGS -Encoding UTF8
    Write-Host "Created settings.json with statusLine entry: $SETTINGS"
} else {
    $json = Get-Content $SETTINGS -Raw | ConvertFrom-Json
    $statusLineValue = [PSCustomObject]@{ type = "command"; command = "cship" }
    if (-not $json.PSObject.Properties["statusLine"]) {
        $json | Add-Member -NotePropertyName "statusLine" -NotePropertyValue $statusLineValue
    } else {
        $json.statusLine = $statusLineValue
    }
    $json | ConvertTo-Json -Depth 100 | Set-Content -Path $SETTINGS -Encoding UTF8
    Write-Host "Updated settings.json with statusLine entry: $SETTINGS"
}

# --- First-run preview ---
Write-Host ""
Write-Host "Running 'cship explain' as a first-run preview..."
& $BIN explain

Write-Host ""
Write-Host "cship $tag installed successfully."
Write-Host "Restart Claude Code for the statusline to take effect."
