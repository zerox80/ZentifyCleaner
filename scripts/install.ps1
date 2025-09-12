#requires -version 5.1
[CmdletBinding()]
param(
    [switch]$NoBuild,
    [switch]$DesktopShortcut
)

function Ensure-Admin {
    $currentIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($currentIdentity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltinRole]::Administrator)) {
        Write-Host "Elevating privileges..." -ForegroundColor Yellow
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = 'powershell.exe'
        # Reconstruct bound parameters (e.g., -NoBuild)
        $paramArgs = @()
        $hasNoBuild = $false
        foreach ($kvp in $MyInvocation.BoundParameters.GetEnumerator()) {
            $key = [string]$kvp.Key
            $val = $kvp.Value
            if ($val -is [switch]) {
                if ($val) {
                    $paramArgs += "-$key"
                    if ($key -ieq 'NoBuild') { $hasNoBuild = $true }
                }
            } else {
                $paramArgs += "-$key `"$val`""
            }
        }
        if (-not $hasNoBuild) { $paramArgs += '-NoBuild' }
        $psi.Arguments = "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`" " + ($paramArgs -join ' ')
        $psi.WorkingDirectory = (Split-Path -Parent $PSCommandPath)
        $psi.Verb = 'runas'
        try {
            $p = [System.Diagnostics.Process]::Start($psi)
            $p.WaitForExit()
            exit $p.ExitCode
        } catch {
            Write-Error "Administrator privileges are required. Aborting."
            exit 1
        }
    }
}

function Ensure-Tool($name, $commandName, $installUrl) {
    $cmd = Get-Command $commandName -ErrorAction SilentlyContinue
    if (-not $cmd) {
        Write-Host "Missing dependency: $name" -ForegroundColor Red
        if ($installUrl) {
            Write-Host "Install it from: $installUrl" -ForegroundColor Yellow
        }
        throw "Dependency '$name' not found"
    }
}

function Add-ToPath($Dir, [ValidateSet('User','Machine')]$Scope = 'Machine') {
    $Dir = [IO.Path]::GetFullPath($Dir)
    $current = [Environment]::GetEnvironmentVariable('Path', $Scope)
    if (-not $current) { $current = '' }
    $parts = $current.Split(';') | Where-Object { $_ -ne '' }
    if ($parts -notcontains $Dir) {
        $new = (($parts + $Dir) -join ';')
        [Environment]::SetEnvironmentVariable('Path', $new, $Scope)
        # Update current session
        if ($env:Path.Split(';') -notcontains $Dir) { $env:Path = "$env:Path;$Dir" }
        Write-Host "Added to PATH ($Scope): $Dir" -ForegroundColor Green
    } else {
        Write-Host "PATH ($Scope) already contains: $Dir" -ForegroundColor DarkYellow
    }
}

function Broadcast-EnvironmentChange {
    try {
        Add-Type -Namespace Win32 -Name Native -MemberDefinition @"
using System;
using System.Runtime.InteropServices;
public static class Native {
  [DllImport("user32.dll", SetLastError=true, CharSet=CharSet.Auto)]
  public static extern IntPtr SendMessageTimeout(IntPtr hWnd, int Msg, IntPtr wParam, string lParam, int fuFlags, int uTimeout, out IntPtr lpdwResult);
}
"@
        $HWND_BROADCAST = [IntPtr]0xffff
        $WM_SETTINGCHANGE = 0x001A
        $result = [IntPtr]::Zero
        [Win32.Native]::SendMessageTimeout($HWND_BROADCAST, $WM_SETTINGCHANGE, [IntPtr]::Zero, 'Environment', 2, 5000, [ref]$result) | Out-Null
    } catch {}
}

function New-Shortcut($Target, $ShortcutPath, $IconPath=$null, $Arguments=$null, $WorkingDirectory=$null) {
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($ShortcutPath)
    $shortcut.TargetPath = $Target
    if ($Arguments) { $shortcut.Arguments = $Arguments }
    if ($IconPath) { $shortcut.IconLocation = $IconPath }
    if ($WorkingDirectory) { $shortcut.WorkingDirectory = $WorkingDirectory }
    $shortcut.Save()
}

try {
    $RepoRoot = Split-Path -Parent $PSCommandPath
    $RepoRoot = Split-Path -Parent $RepoRoot # scripts/ -> project root
    Set-Location $RepoRoot

    # 1) Build as current user (so cargo/go are available in PATH)
    if (-not $NoBuild) {
        Write-Host "Checking dependencies..." -ForegroundColor Cyan
        Ensure-Tool 'Rust (cargo)' 'cargo' 'https://rustup.rs'

        Write-Host "Building Rust CLI + Web UI (release) with increased parallelism..." -ForegroundColor Cyan
        # Use more parallelism for faster builds:
        #  - RUSTFLAGS "-C codegen-units=8" enables parallel codegen per crate (faster compile, slightly less optimized than 1)
        #  - cargo -j 10 controls parallel rustc jobs across crates (up to 10 concurrent jobs)
        $env:RUSTFLAGS = '-C codegen-units=8'
        # Build all bins with the 'web' feature so zentify-web.exe is produced as well
        cargo build --release --locked --features web --bins -j 10
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    }

    # 2) Elevate for installation tasks only
    Ensure-Admin

    $CliExe = Join-Path $RepoRoot 'target\release\zentify-cleaner.exe'
    if (-not (Test-Path $CliExe)) { throw "CLI binary not found at $CliExe" }
    $WebExe = Join-Path $RepoRoot 'target\release\zentify-web.exe'
    if (-not (Test-Path $WebExe)) { throw "Web UI binary not found at $WebExe (did cargo build include --features web?)" }

    # Resolve Program Files (64-bit) robustly even if script runs under 32-bit PowerShell
    $ProgramFiles64 = if ($env:ProgramW6432) { $env:ProgramW6432 } else { $env:ProgramFiles }
    $InstallDir = Join-Path $ProgramFiles64 'Zentify Cleaner'
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

    # Stop any running instances to avoid file lock issues
    Write-Host "Stopping any running instances..." -ForegroundColor Cyan
    foreach ($pName in 'zentify-web','zentify-cleaner') {
        Get-Process -Name $pName -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    }

    Write-Host "Copying files to $InstallDir" -ForegroundColor Cyan
    Copy-Item $CliExe $InstallDir -Force
    Copy-Item $WebExe $InstallDir -Force
    foreach ($file in @('README.md','LICENSE','Icon.png')) {
        if (Test-Path (Join-Path $RepoRoot $file)) { Copy-Item (Join-Path $RepoRoot $file) $InstallDir -Force }
    }

    # Also copy uninstaller scripts to the install directory so Apps & Features works without the repo
    $ScriptsDir = Join-Path $RepoRoot 'scripts'
    foreach ($f in @('uninstall.ps1','uninstall.cmd')) {
        $src = Join-Path $ScriptsDir $f
        if (Test-Path $src) { Copy-Item $src $InstallDir -Force }
    }

    Add-ToPath $InstallDir -Scope 'Machine'
    # Broadcast env change so new processes pick up PATH immediately
    Broadcast-EnvironmentChange

    # 3) Create a small launcher CMD that starts the Web UI and opens the browser
    $LauncherCmd = Join-Path $InstallDir 'Zentify Web UI.cmd'
    $cmdContent = @"
@echo off
setlocal
set DIR=%~dp0
REM Start web server elevated (UAC prompt)
powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath '%DIR%zentify-web.exe' -Verb RunAs"
REM Give it a moment to bind, then open browser
ping 127.0.0.1 -n 3 >nul
start "" "http://127.0.0.1:7878"
"@
    Set-Content -Path $LauncherCmd -Value $cmdContent -Encoding ASCII -Force

    # 4) Create Start Menu shortcut (All Users) to the launcher
    $StartMenuAll = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs'
    $ShortcutPath = Join-Path $StartMenuAll 'Zentify Cleaner Web UI.lnk'
    $IconPath = Join-Path $InstallDir 'zentify-web.exe'
    Write-Host "Creating Start Menu shortcut: $ShortcutPath" -ForegroundColor Cyan
    New-Shortcut -Target $LauncherCmd -ShortcutPath $ShortcutPath -IconPath $IconPath -WorkingDirectory $InstallDir

    # 4b) Create Start Menu shortcut for CLI as well
    $CliShortcut = Join-Path $StartMenuAll 'Zentify Cleaner.lnk'
    $CliIcon = Join-Path $InstallDir 'zentify-cleaner.exe'
    Write-Host "Creating Start Menu shortcut: $CliShortcut" -ForegroundColor Cyan
    New-Shortcut -Target (Join-Path $InstallDir 'zentify-cleaner.exe') -ShortcutPath $CliShortcut -IconPath $CliIcon -WorkingDirectory $InstallDir

    # 4c) Create Start Menu shortcut for Uninstall
    $UninstallShortcut = Join-Path $StartMenuAll 'Uninstall Zentify Cleaner.lnk'
    Write-Host "Creating Start Menu shortcut: $UninstallShortcut" -ForegroundColor Cyan
    New-Shortcut -Target (Join-Path $InstallDir 'uninstall.cmd') -ShortcutPath $UninstallShortcut -IconPath $CliIcon -WorkingDirectory $InstallDir

    # 5) Optionally create a Desktop shortcut for all users
    if ($DesktopShortcut) {
        $DesktopPublic = Join-Path $env:Public 'Desktop'
        if (-not (Test-Path $DesktopPublic)) { New-Item -ItemType Directory -Force -Path $DesktopPublic | Out-Null }
        $DesktopShortcutPath = Join-Path $DesktopPublic 'Zentify Cleaner Web UI.lnk'
        Write-Host "Creating Desktop shortcut: $DesktopShortcutPath" -ForegroundColor Cyan
        New-Shortcut -Target $LauncherCmd -ShortcutPath $DesktopShortcutPath -IconPath $IconPath -WorkingDirectory $InstallDir
        # CLI Desktop shortcut too
        $DesktopCliShortcut = Join-Path $DesktopPublic 'Zentify Cleaner.lnk'
        Write-Host "Creating Desktop shortcut: $DesktopCliShortcut" -ForegroundColor Cyan
        New-Shortcut -Target (Join-Path $InstallDir 'zentify-cleaner.exe') -ShortcutPath $DesktopCliShortcut -IconPath $CliIcon -WorkingDirectory $InstallDir
    }

    # 6) Register in Apps & Features (Add/Remove Programs)
    Write-Host "Registering in Apps & Features..." -ForegroundColor Cyan
    $displayName = 'Zentify Cleaner'
    # Parse version from Cargo.toml
    $cargoToml = Join-Path $RepoRoot 'Cargo.toml'
    $version = '1.0.0'
    try {
        if (Test-Path $cargoToml) {
            $content = Get-Content $cargoToml -Raw
            # Use single-quoted regex to avoid escaping double quotes inside the pattern
            $m = [regex]::Match($content, '(?m)^version\s*=\s*"(?<v>[^"]+)"')
            if ($m.Success) { $version = $m.Groups['v'].Value }
        }
    } catch {}
    $publisher = 'Zentify'
    $displayIcon = Join-Path $InstallDir 'zentify-cleaner.exe'
    $uninstallCmd = "cmd /c `"" + (Join-Path $InstallDir 'uninstall.cmd') + "`""
    $regKey = "HKLM:Software\Microsoft\Windows\CurrentVersion\Uninstall\ZentifyCleaner"
    New-Item -Path $regKey -Force | Out-Null
    New-ItemProperty -Path $regKey -Name 'DisplayName' -Value $displayName -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $regKey -Name 'DisplayVersion' -Value $version -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $regKey -Name 'Publisher' -Value $publisher -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $regKey -Name 'InstallLocation' -Value $InstallDir -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $regKey -Name 'DisplayIcon' -Value $displayIcon -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $regKey -Name 'UninstallString' -Value $uninstallCmd -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $regKey -Name 'QuietUninstallString' -Value $uninstallCmd -PropertyType String -Force | Out-Null

    Write-Host "Installation complete." -ForegroundColor Green
    Write-Host "Installed to: $InstallDir"
    Write-Host "You can run 'zentify-cleaner' from any new terminal."
    Write-Host "Launch the Web UI from the Start Menu: 'Zentify Cleaner Web UI'"

    exit 0
} catch {
    Write-Error $_
    exit 1
}
