#requires -version 5.1
[CmdletBinding()]
param(
    [switch]$PurgeConfig  # also remove %APPDATA%\Zentify data (logs/config)
)

function Ensure-Admin {
    $currentIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($currentIdentity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltinRole]::Administrator)) {
        Write-Host "Elevating privileges..." -ForegroundColor Yellow
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = 'powershell.exe'
        # Reconstruct bound parameters (e.g., -PurgeConfig)
        $paramArgs = @()
        foreach ($kvp in $MyInvocation.BoundParameters.GetEnumerator()) {
            $key = [string]$kvp.Key
            $val = $kvp.Value
            if ($val -is [switch]) {
                if ($val) { $paramArgs += "-$key" }
            } else {
                $paramArgs += "-$key `"$val`""
            }
        }
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

function Remove-FromPath($Dir, [ValidateSet('User','Machine')]$Scope = 'Machine') {
    $Dir = [IO.Path]::GetFullPath($Dir)
    $current = [Environment]::GetEnvironmentVariable('Path', $Scope)
    if ($null -eq $current) { $current = '' }
    $parts = $current.Split(';') | Where-Object { $_ -ne '' -and ($_ -ne $Dir) }
    $new = ($parts -join ';')
    [Environment]::SetEnvironmentVariable('Path', $new, $Scope)
    Write-Host "Removed from PATH ($Scope): $Dir" -ForegroundColor Green
    # Update current session PATH too
    $envParts = $env:Path.Split(';') | Where-Object { $_ -ne '' -and ($_ -ne $Dir) }
    $env:Path = ($envParts -join ';')
}

function Remove-Shortcut($ShortcutPath) {
    if (Test-Path $ShortcutPath) {
        Remove-Item $ShortcutPath -Force -ErrorAction SilentlyContinue
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

function Remove-DirRobust($Path) {
    if (-not (Test-Path $Path)) { return }
    # Try to clear attributes
    Get-ChildItem -Recurse -Force $Path -ErrorAction SilentlyContinue | ForEach-Object { try { $_.Attributes = 'Normal' } catch {} }
    # Try multiple attempts
    for ($i=0; $i -lt 3; $i++) {
        try {
            Remove-Item $Path -Recurse -Force -ErrorAction Stop
            return
        } catch {
            Start-Sleep -Milliseconds 200
        }
    }
    # Fallback: rename for manual cleanup
    try {
        $parent = Split-Path -Parent $Path
        $name = Split-Path -Leaf $Path
        $fallback = Join-Path $parent ("$name._pending_delete_" + [guid]::NewGuid().ToString())
        Rename-Item -Path $Path -NewName (Split-Path -Leaf $fallback) -Force -ErrorAction SilentlyContinue
        Write-Host "Directory in use. Renamed for manual cleanup: $fallback" -ForegroundColor Yellow
    } catch {}
}

try {
    Ensure-Admin

    # Try to read InstallLocation from Apps & Features; fall back to Program Files
    $regKeyNative = 'HKLM:Software\Microsoft\Windows\CurrentVersion\Uninstall\ZentifyCleaner'
    $regKeyWow6432 = 'HKLM:Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\ZentifyCleaner'
    $InstallDir = $null
    foreach ($rk in @($regKeyNative, $regKeyWow6432)) {
        try {
            $val = (Get-ItemProperty -Path $rk -ErrorAction SilentlyContinue).InstallLocation
            if ($val -and (Test-Path $val)) { $InstallDir = $val; break }
        } catch {}
    }
    if (-not $InstallDir) {
        $ProgramFiles64 = if ($env:ProgramW6432) { $env:ProgramW6432 } else { $env:ProgramFiles }
        $InstallDir = Join-Path $ProgramFiles64 'Zentify Cleaner'
    }
    $Cli = Join-Path $InstallDir 'zentify-cleaner.exe'
    $Web = Join-Path $InstallDir 'zentify-web.exe'

    Write-Host "Stopping any running instances..." -ForegroundColor Cyan
    foreach ($pName in 'zentify-cleaner','zentify-web') {
        Get-Process -Name $pName -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    }

    Write-Host "Removing Start Menu and Desktop shortcuts..." -ForegroundColor Cyan
    $StartMenuAll = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs'
    $StartMenuUser = Join-Path ([Environment]::GetFolderPath('StartMenu')) 'Programs'
    foreach ($sm in @($StartMenuAll, $StartMenuUser)) {
        if ($sm) {
            Remove-Shortcut (Join-Path $sm 'Zentify Cleaner.lnk')
            Remove-Shortcut (Join-Path $sm 'Zentify Cleaner Web UI.lnk')
            Remove-Shortcut (Join-Path $sm 'Uninstall Zentify Cleaner.lnk')
        }
    }
    $DesktopUser = [Environment]::GetFolderPath('Desktop')
    $DesktopPublic = Join-Path $env:Public 'Desktop'
    foreach ($desk in @($DesktopUser, $DesktopPublic)) {
        if ($desk) {
            Remove-Shortcut (Join-Path $desk 'Zentify Cleaner.lnk')
            Remove-Shortcut (Join-Path $desk 'Zentify Cleaner Web UI.lnk')
        }
    }

    # Remove launcher CMD if present
    $LauncherCmd = Join-Path $InstallDir 'Zentify Web UI.cmd'
    if (Test-Path $LauncherCmd) { Remove-Item $LauncherCmd -Force -ErrorAction SilentlyContinue }

    Write-Host "Removing install directory: $InstallDir" -ForegroundColor Cyan
    Remove-DirRobust $InstallDir

    # Remove from both Machine and User PATH scopes (in case older installer used User)
    foreach ($scope in 'Machine','User') { Remove-FromPath $InstallDir -Scope $scope }

    # Remove Apps & Features entries (both native and WOW6432Node if present)
    foreach ($rk in @($regKeyNative, $regKeyWow6432)) {
        try {
            if (Test-Path $rk) { Remove-Item $rk -Recurse -Force -ErrorAction SilentlyContinue }
        } catch {}
    }

    if ($PurgeConfig) {
        Write-Host "Purging user data (config/logs) under %APPDATA%\\Zentify ..." -ForegroundColor Yellow
        $appBase = Join-Path $env:APPDATA 'Zentify'
        if (Test-Path $appBase) { Remove-DirRobust $appBase }
    }

    # Broadcast env change so new processes pick up PATH immediately
    Broadcast-EnvironmentChange

    Write-Host "Uninstall complete." -ForegroundColor Green
    exit 0
} catch {
    Write-Error $_
    exit 1
}
