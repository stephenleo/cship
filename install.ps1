#Requires -Version 5.1
<#
.SYNOPSIS
    Install cship - Claude Code statusline tool for Windows.
.DESCRIPTION
    Downloads the cship binary from GitHub Releases, installs it to
    %USERPROFILE%\.local\bin\, writes a default cship.toml, and
    registers the statusline in Claude Code's settings.json.
.PARAMETER Yes
    Non-interactive mode: accept all prompts automatically.
#>
param([switch]$Yes)
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

# --- Uninstall any existing cship ---
$existingCship = Get-Command cship -ErrorAction SilentlyContinue
if ($existingCship) {
    Write-Host "Existing cship found — running uninstall to clean up before upgrade..."
    & $existingCship.Source uninstall
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

# --- Starship detection and optional install ---
$INTERACTIVE = [Environment]::UserInteractive -and -not [Console]::IsInputRedirected
if (-not (Get-Command starship -ErrorAction SilentlyContinue)) {
    $winget = Get-Command winget -ErrorAction SilentlyContinue
    if (-not $winget) {
        Write-Host "Starship not found, and winget is not available on this system."
        Write-Host "Install Starship manually from https://starship.rs/ then re-run this script."
        Write-Host "Native cship modules will still work without Starship."
    } else {
        $installStarship = {
            & $winget.Source install --id Starship.Starship --accept-source-agreements --accept-package-agreements
            if ($LASTEXITCODE -ne 0) {
                Write-Host "Starship installation via winget failed (exit code $LASTEXITCODE). Install manually from https://starship.rs/"
            }
        }
        if ($Yes) {
            & $installStarship
        } elseif ($INTERACTIVE) {
            $answer = Read-Host "Starship not found. Install Starship? (required for passthrough modules) [Y/n]"
            if ($answer -match '^[Nn]') {
                Write-Host "Skipping Starship install. Native cship modules will still work."
            } else {
                & $installStarship
            }
        } else {
            Write-Host "Skipping Starship install (non-interactive). Re-run with -Yes or install manually from https://starship.rs/"
        }
    }
}

# --- Write default cship.toml ---
if (-not (Test-Path $CONFIG_FILE)) {
    New-Item -ItemType Directory -Force -Path $CONFIG_DIR | Out-Null
    $tomlContent = @'
# cship — Claude Code statusline
# Full config reference: https://cship.dev
[cship]
lines = [
  "$directory$git_branch$git_status$python$nodejs$rust",
  "$cship.model $cship.cost $cship.context_bar $cship.usage_limits"
]

[cship.model]
symbol = "🤖 "
style  = "bold cyan"

[cship.context_bar]
width              = 10
style              = "fg:#7dcfff"
warn_threshold     = 40.0
warn_style         = "fg:#e0af68"
critical_threshold = 70.0
critical_style     = "bold fg:#f7768e"

[cship.cost]
symbol             = "💰 "
style              = "fg:#a9b1d6"
warn_threshold     = 2.0
warn_style         = "fg:#e0af68"
critical_threshold = 5.0
critical_style     = "bold fg:#f7768e"

[cship.usage_limits]
five_hour_format   = "⌛ 5h {pct}% ({reset})"
seven_day_format   = "📅 7d {pct}% ({reset})"
separator          = " "
warn_threshold     = 60.0
warn_style         = "fg:#e0af68"
critical_threshold = 80.0
critical_style     = "bold fg:#f7768e"
'@
    [System.IO.File]::WriteAllText($CONFIG_FILE, $tomlContent, (New-Object System.Text.UTF8Encoding $false))
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
    # Create minimal settings.json — $claudeDir already exists (checked above)
    $utf8NoBom = New-Object System.Text.UTF8Encoding $false
    [System.IO.File]::WriteAllText($SETTINGS, '{"statusLine": {"type": "command", "command": "cship"}}', $utf8NoBom)
    Write-Host "Created settings.json with statusLine entry: $SETTINGS"
} else {
    $json = Get-Content $SETTINGS -Raw | ConvertFrom-Json
    $statusLineValue = [PSCustomObject]@{ type = "command"; command = "cship" }
    if (-not $json.PSObject.Properties["statusLine"]) {
        $json | Add-Member -NotePropertyName "statusLine" -NotePropertyValue $statusLineValue
    } else {
        $json.statusLine = $statusLineValue
    }
    $utf8NoBom = New-Object System.Text.UTF8Encoding $false
    [System.IO.File]::WriteAllText($SETTINGS, ($json | ConvertTo-Json -Depth 100), $utf8NoBom)
    Write-Host "Updated settings.json with statusLine entry: $SETTINGS"
}

# --- First-run preview ---
Write-Host ""
Write-Host "Running 'cship explain' as a first-run preview..."
& $BIN explain

Write-Host ""
Write-Host "cship $tag installed successfully."
Write-Host "Restart Claude Code for the statusline to take effect."
