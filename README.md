# Zentify Cleaner

Minimal, fast Windows temp cleaner (CLI + optional local Web UI). Built in Rust for speed and safety.

- Windows 10/11 support
- Safe-by-default cleaning of common temp/cache locations
- Dry-run mode, detailed logging, and optional exact byte statistics
- Optional local Web UI (`zentify-web`) with CSRF protection
- Clean Windows install/uninstall scripts with Start Menu shortcuts


## Table of Contents

- Features
- Safety model
- Installation
- Quick start
- CLI usage
- Web UI
- Configuration
- Environment variables
- Build from source
- Troubleshooting
- Project structure
- License


## Features

- Cleans common user and system caches (Temp, INetCache, WebCache, WER, DirectX/NVIDIA shader caches, Teams, Office cache, UWP LocalCache/TempState, Java/Adobe/WMP caches, and more)
- Browser caches (Chromium family: Chrome/Edge/Brave/Vivaldi/Opera; Firefox)
- Optional system-level targets (Windows Temp, Prefetch, Windows Update Download, Delivery Optimization cache, Defender history, crash dumps, ASP.NET temp)
- Dry-run preview and exact-stats mode
- Concurrency for fast cleaning

See `src/lib.rs` for the full list of categories and paths.


## Safety model

- Operates only under conservative allowed prefixes (e.g., `%TEMP%`, `%LOCALAPPDATA%`, `%APPDATA%`; plus `%WINDIR%`, `%SystemRoot%`, `%ProgramData%` when system cleaning is allowed)
- Skips filesystem roots (e.g., `C:\`)
- Avoids traversing reparse points (junctions/symlinks); removes the link itself instead
- Protects sensitive top-level directories (e.g., `Windows`, `Program Files`, `ProgramData`, `C:\Users` root)
- System-level cleaning is only enabled when elevated or explicitly allowed via env vars (see below)


## Installation

You can either use the Windows install scripts or build from source.

- PowerShell (recommended):
  ```powershell
  .\scripts\install.ps1
  ```
- CMD wrapper:
  ```bat
  .\scripts\install.cmd
  ```

What the installer does:
- Builds release binaries with the `web` feature
- Installs to `C:\Program Files\Zentify Cleaner`
- Adds the install directory to PATH (Machine)
- Creates Start Menu shortcuts: CLI, Web UI launcher, and Uninstall
- Registers an uninstaller in Apps & Features

Uninstall:
```bat
.\scripts\uninstall.cmd
```


## Quick start

- Dry-run (no deletions):
  ```bat
  zentify-cleaner --dry-run
  ```

- Actual cleaning with default categories:
  ```bat
  zentify-cleaner
  ```

- Run Web UI (local only by default):
  ```bat
  zentify-web
  ```
  Then open: http://127.0.0.1:7878


## CLI usage

`zentify-cleaner` supports the following flags:

```
--dry-run         Do not delete anything, only print what would be deleted
--verbose         Increase verbosity (overrides quiet)
--quiet           Silence most output
--exact-stats     Compute exact freed byte counts (slower)
```

Behavioral notes:
- System-level cleaning is enabled automatically when running elevated, or via env var `ZENTIFY_ALLOW_SYSTEM_CLEAN=1`. You can force-disable it with `ZENTIFY_FORCE_NO_SYSTEM_CLEAN=1`.
- Prefetch cleanup is disabled by default; enable via `ZENTIFY_PREFETCH=1` or by checking it in the Web UI.
- Concurrency defaults to up to 8 threads; override via `ZENTIFY_MAX_PARALLELISM=N`.
- In fast mode (default), directory byte totals are approximate. Use `--exact-stats` for precise totals.

Output example:
- On dry-run, you will see what would be removed and an approximate total bytes freed.
- On real runs, a summary is printed. If not in exact mode, a note clarifies byte counts are approximate.


## Web UI

`zentify-web` exposes a minimal local interface (Axum/Hyper) to run dry-runs and cleanups.

- Default bind address: `127.0.0.1:7878` (set `ZENTIFY_WEB_BIND=IP:PORT` to change)
- Loopback restriction is enforced unless `ZENTIFY_WEB_ALLOW_NON_LOCAL=1` is set (use with care)
- Optional auto-elevation on Windows via `ZENTIFY_WEB_AUTO_ELEVATE=1` (relaunches with UAC)
- CSRF protection: state-changing requests require a token obtained via `/api/csrf`

Key endpoints (used by the built-in UI):
- `GET /` – UI
- `GET /api/health` – health info
- `GET /api/version` – version metadata
- `GET /api/permissions` – elevation and defaults
- `GET /api/csrf` – CSRF token
- `GET/PUT/DELETE /api/config` – load/override/clear config
- `POST /api/preview` – list candidate targets
- `GET /api/history` – recent runs
- `POST /api/run` – run synchronously
- `POST /api/run-async` + `GET/DELETE /api/job/:id` – async job management


## Configuration

Zentify Cleaner can load an optional JSON config. Search order:
1. `./.zentify/config.json` (current directory)
2. `%ProgramData%/Zentify/config.json`
3. `%APPDATA%/Zentify/config.json`

Shape (example):
```json
{
  "dry_run": false,
  "verbose": false,
  "quiet": false,
  "exact_stats": false,
  "categories": {
    "windows_temp": true,
    "user_temp": true,
    "browser_cache": true,
    "windows_update": true,
    "delivery_optimization": true,
    "crash_dumps": true,
    "error_reports": true,
    "thumbnails": true,
    "directx_cache": true,
    "temp_internet_files": true,
    "prefetch": false,
    "defender_cache": true,
    "office_cache": true,
    "aspnet_temp": true,
    "teams_cache": true,
    "modern_apps_cache": true,
    "java_cache": true,
    "adobe_cache": true,
    "wmp_cache": true,
    "widgets_cache": true
  }
}
```

The Web UI also supports in-memory config overrides via its `/api/config` endpoint and UI controls.


## Environment variables

Cleaner (CLI & library):
- `ZENTIFY_ALLOW_SYSTEM_CLEAN=1` – allow system-level targets even if not elevated
- `ZENTIFY_FORCE_NO_SYSTEM_CLEAN=1` – force-disable system-level cleanup
- `ZENTIFY_PREFETCH=1` – enable Windows Prefetch cleanup
- `ZENTIFY_MAX_PARALLELISM=N` – limit worker threads

Web UI:
- `ZENTIFY_WEB_BIND=127.0.0.1:7878` – bind address
- `ZENTIFY_WEB_ALLOW_NON_LOCAL=1` – allow non-loopback binds (be cautious)
- `ZENTIFY_WEB_AUTO_ELEVATE=1` – attempt to relaunch elevated on start (Windows)
- `ZENTIFY_WEB_RUN_TIMEOUT_SECS=600` – server-side run timeout


## Build from source

Requirements:
- Rust 1.70+ (Windows 10/11)

Build all binaries (including Web UI):
```powershell
cargo build --release --locked --features web --bins
```
Binaries will be under `target\release\`:
- `zentify-cleaner.exe`
- `zentify-web.exe`


## Troubleshooting

- Cleaner cannot remove some files:
  - This can be normal when files are locked. The cleaner may schedule deletion on reboot for certain files (e.g., Explorer caches).
- No output or too quiet:
  - Ensure `--quiet` is not set. Use `--verbose` for more details.
- Web UI not reachable:
  - Verify the server printed the bound address and that you’re using the same host/port. Check firewall rules.
  - By default, non-loopback binds are refused. Set `ZENTIFY_WEB_ALLOW_NON_LOCAL=1` to override (with care).
- System-level cleaning didn’t happen:
  - Run elevated, or set `ZENTIFY_ALLOW_SYSTEM_CLEAN=1`. `ZENTIFY_FORCE_NO_SYSTEM_CLEAN=1` can disable it.


## Project structure

- `src/main.rs` – CLI entry (binary: `zentify-cleaner`)
- `src/lib.rs` – core cleaning logic and public API
- `src/bin/zentify-web.rs` – Web UI server (binary: `zentify-web`)
- `scripts/` – Windows install/uninstall helpers
- `build.rs` – build metadata hooks


## License

MIT. See `LICENSE`.
