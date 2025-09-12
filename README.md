# Zentify Cleaner

[![CI](https://github.com/zerox80/ZentifyCleaner/actions/workflows/ci.yml/badge.svg)](https://github.com/zerox80/ZentifyCleaner/actions/workflows/ci.yml)

Minimal, extremely fast Windows cleaner (CLI) that aggressively removes temporary files and caches. Built for Windows 10/11.

> Warning: This tool intentionally deletes cache/temp directories aggressively without asking for confirmation. Please read the safety notes below.

---

## Table of Contents

- [Features](#features)
- [Supported Targets (Examples)](#supported-targets-examples)
- [Safety Notes](#safety-notes)
- [Design & Security Architecture](#design--security-architecture)
- [Performance](#performance)
- [System Requirements](#system-requirements)
- [Installation](#installation)
- [Usage](#usage)
- [Configuration](#configuration)
- [Environment Variables](#environment-variables)
- [FAQ](#faq)
- [Troubleshooting](#troubleshooting)
- [Known Limitations](#known-limitations)
- [Uninstall](#uninstall)
- [Build from Source](#build-from-source)
- [CI/CD](#cicd)
- [Roadmap](#roadmap)
- [Contributing](#contributing)
- [Support](#support)
- [License](#license)

---

## Features

- Aggressive cleanup of common Windows temp and cache paths
- Supports common browser caches (Chromium-based and Firefox)
- Cleans Windows Update downloads, Delivery Optimization caches, WER report queues/archives, minidumps, and more
- Removes Explorer thumbnail and icon caches (`thumbcache*.db`, `iconcache*.db`)
- Deduplicated target paths and robust deletion (full removal with fallback to shallow cleanup)
- Configurable via `.zentify/config.json` (dry-run, verbose/quiet, categories)
- Safer defaults: System-wide paths (e.g., `%WINDIR%`) are only cleaned after opt-in (`ZENTIFY_ALLOW_SYSTEM_CLEAN=1`) OR when running with Administrator privileges
- During a system clean, `Prefetch` cleaning is additionally enabled
- Zero runtime dependencies (pure Rust CLI), very fast startup
- Two statistics modes: Fast mode (approximate byte counting) or exact counting via `--exact-stats`

## Supported Targets (Examples)

A non-exhaustive list of currently targeted paths/patterns (depending on existing directories and profiles):

- User/System temp: `%TEMP%`, `%LOCALAPPDATA%/Temp`, `%WINDIR%/Temp`, `%SystemRoot%/Temp`
- Windows-specific:
  - DirectX Shader Cache: `%LOCALAPPDATA%/D3DSCache`
  - IE/Edge (Legacy) Cache: `%LOCALAPPDATA%/Microsoft/Windows/INetCache`
  - WebCache (ESE, Legacy IE/Edge/Explorer): `%LOCALAPPDATA%/Microsoft/Windows/WebCache`
  - Prefetch: `%WINDIR%/Prefetch`
  - Windows Update Downloads: `%WINDIR%/SoftwareDistribution/Download`, `%SystemRoot%/SoftwareDistribution/Download`
  - Minidumps: `%WINDIR%/Minidump`
  - Delivery Optimization: `%ProgramData%/Microsoft/Windows/DeliveryOptimization/Cache`
  - Windows Error Reporting: `%ProgramData%/Microsoft/Windows/WER/{ReportQueue,ReportArchive,Temp}`
  - Windows Error Reporting (User): `%LOCALAPPDATA%/Microsoft/Windows/WER/{ReportQueue,ReportArchive,Temp}`
  - User CrashDumps: `%LOCALAPPDATA%/CrashDumps`
  - Live Kernel Reports (large; system clean only): `%WINDIR%/LiveKernelReports`
  - Memory dump (very large; system clean only): `%WINDIR%/MEMORY.DMP`
  - Defender history (optional): `%ProgramData%/Microsoft/Windows Defender/Scans/History`
  - ASP.NET Temporary Files (optional): `%WINDIR%/Microsoft.NET/Framework{,64}/v4.0.30319/Temporary ASP.NET Files`
- Browser caches:
  - Chromium-based (e.g., Chrome, Edge, Brave, Vivaldi, Opera GX): per profile including `Cache`, `Code Cache`, `GPUCache`, `ShaderCache`, `DawnCache`, `GrShaderCache`, `Media Cache`, `Service Worker/CacheStorage`, `Application Cache`, `Network/Cache`
  - Chromium (product-wide, not per profile): `User Data/ShaderCache/GPUCache` (Chrome/Edge/Brave/Vivaldi/Opera)
  - Opera (Stable): analogous to Opera GX under `%LOCALAPPDATA%/Opera Software/Opera Stable/...`
  - Firefox: `cache2`, `startupCache` per profile under `%LOCALAPPDATA%/Mozilla/Firefox/Profiles`
- Explorer thumbnail/icon caches (file deletion): `thumbcache*.db`, `iconcache*.db` in `%LOCALAPPDATA%/Microsoft/Windows/Explorer`

- Applications/Services (optional):
  - Microsoft Teams (Classic): `%APPDATA%/Microsoft/Teams/{Cache,GPUCache,Service Worker/CacheStorage,IndexedDB,Local Storage}`
  - Microsoft Teams (New/UWP): `%LOCALAPPDATA%/Packages/MSTeams_8wekyb3d8bbwe/LocalCache`
  - Office Document Cache: `%LOCALAPPDATA%/Microsoft/Office/16.0/OfficeFileCache`
  - UWP/Store apps (per package): `%LOCALAPPDATA%/Packages/*/{LocalCache,TempState}` (no `LocalState`/`RoamingState`)
  - NVIDIA shader caches: `%LOCALAPPDATA%/NVIDIA/{GLCache,DXCache}`
  - Java cache: `%LOCALAPPDATA%/Sun/Java/Deployment/cache`
  - Adobe media caches: `%LOCALAPPDATA%/Adobe/Common/{Media Cache,Media Cache Files}`
  - Windows Media Player cache: `%LOCALAPPDATA%/Microsoft/Media Player/Cache`

Note: Only existing paths are cleaned. Non-existent paths are skipped.

### Sources (Selection)

- Windows Update/Delivery Optimization/WER: Microsoft Q&A, “Deleting SoftwareDistribution/Download is safe” – https://learn.microsoft.com/en-us/answers/questions/4204298/c-windowssoftwaredistributiondownload-deleting
- WER settings (default CrashDumps path) – https://learn.microsoft.com/en-us/windows/win32/wer/wer-settings
- WER ReportArchive/ReportQueue cleanup – https://woshub.com/wer-windows-error-reporting-clear-reportqueue-folder-windows/
- Removing crash dumps (if not needed) – https://learn.microsoft.com/en-us/answers/questions/217604/can-i-delete-dmp-files-from-a-windows-server-manua
- Chromium Service Worker CacheStorage (Edge/Chrome) – https://superuser.com/questions/1608022/how-to-clear-chrome-chromium-edge-service-worker-cache
- Vivaldi/Chromium cache locations – https://forum.vivaldi.net/topic/91432/cache-locations
- Chrome Shader/GPU cache – https://artifacts-kb.readthedocs.io/en/latest/sources/webbrowser/ChromeCache.html
- Mozilla Firefox cache2 path – https://support.mozilla.org/en-US/questions/1275991
- Microsoft Teams cache (official) – https://learn.microsoft.com/en-us/troubleshoot/microsoftteams/teams-administration/clear-teams-cache
- New Teams (UWP) cache – https://learn.microsoft.com/en-us/answers/questions/4418666/clear-new-teams-cache
- Office Document Cache – https://support.microsoft.com/en-us/office/delete-your-office-document-cache-b1d3765e-d71b-4bb8-99ca-acd22c42995d

## Safety Notes

- Aggressive deletion: Cache folders and temp directories are removed entirely where possible and may be recreated. This can lead to larger one-time cache rebuilds (e.g., browsers may start slower the first time after cleaning).
- Close browsers & apps: Please close all browsers and applications before starting to avoid file locks and partial deletions.
- No prompts: The tool is non-interactive and does not ask for confirmation. A simulation mode is available via configuration (`dry_run`).
- Use at your own risk: Use only if you understand the implications. Important user data (documents, pictures, etc.) are not intentionally targeted, but still: use at your own risk.
- System-wide paths (e.g., `%WINDIR%/Temp`, `%ProgramData%/...`) are cleaned only if the environment variable `ZENTIFY_ALLOW_SYSTEM_CLEAN=1` is set OR if the program runs with Administrator privileges.
- Additional safeguard (allowlist): The CLI only cleans paths under conservatively defined base paths (`%LOCALAPPDATA%`, `%APPDATA%`, temporary directories and – only with system clean – `%WINDIR%`, `%SystemRoot%`, `%ProgramData%`). Unexpected paths outside these areas are rejected.

## Design & Security Architecture

- Minimalistic, fast CLI: No runtime dependencies; implemented in Rust for low startup time.
- Conservative safeguards:
  - No operations on root directories (e.g., `C:\`).
  - Detects and does not traverse reparse points/junctions/symlinks (`is_reparse_point`), instead removes the link itself (best effort).
  - Protection against sensitive top-level system paths (`is_sensitive_dir`).
- Execution modes:
  - User clean: Default, no admin rights, user-specific caches/temps only.
  - System clean: Activated when running as admin or `ZENTIFY_ALLOW_SYSTEM_CLEAN=1`; extends to system paths and force-enables `Prefetch` cleanup.
- Parallelism & performance:
  - Parallel processing of target directories with a maximum of 8 threads; configurable via `ZENTIFY_MAX_PARALLELISM`.
  - Pre-computed metrics per directory for accurate “freed” totals.
- Transparency:
  - Summary statistics at the end (files/dirs/links/bytes, runtime).
  - `dry_run` support via configuration file for safe testing.

## Performance

- Startup time: Very short, lightweight Rust CLI with no additional runtime.
- I/O pattern: “Aggressive” deletion tries `remove_dir_all` first, otherwise falls back to shallow cleanup.
- Parallelism: By default up to `min(cores, 8)` threads, adjustable via `ZENTIFY_MAX_PARALLELISM`.
- Known factors: Locked files (e.g., running browsers/apps) and very deep directory trees can increase runtime.
- Statistic modes:
  - Fast mode (default): fast; byte totals for entirely removed directories are determined approximately/partially. The summary notes this.
  - Exact: `--exact-stats` or `"exact_stats": true` in configuration computes precise byte totals (slower due to additional scan).

## System Requirements

- Windows 10/11 (64-bit)
- For installation via script: PowerShell 5.1 or newer (default on Windows 10/11)
- For building from source: Rust toolchain (MSRV in CI: 1.70)
- Administrator privileges recommended for installation/uninstall and full cleanup

## Installation

### A) Via release package (recommended)

1. Download the latest ZIP from [Releases](https://github.com/zentify/zentify-cleaner/releases).
2. Extract it to a folder of your choice (e.g., `C:\Program Files\Zentify Cleaner`).
3. Optional: Add the folder to your system `PATH` to run `zentify-cleaner` from anywhere.
4. Launch the GUI via `Zentify Web UI.cmd` (UAC prompt will appear automatically) or run the CLI directly (`zentify-cleaner.exe`).

The ZIP contains: `zentify-cleaner.exe`, `zentify-web.exe`, `Zentify Web UI.cmd`, `README.md`, and possibly `Icon.png`.

### B) Via installation scripts (self-building)

The PowerShell script builds the CLI and Web UI (release) and installs both into `C:\Program Files\Zentify Cleaner`. It also adds the folder to the system `PATH`, creates Start Menu shortcuts for the CLI (`Zentify Cleaner`) and the GUI (`Zentify Cleaner Web UI`), and can optionally create Desktop shortcuts.

PowerShell (recommended):

```powershell
# From the project root
pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1

# Optional: skip build (if already built)
pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1 -NoBuild

# Optional: also create Desktop shortcuts
pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1 -DesktopShortcut
```

CMD:

```bat
REM From the project root
scripts\install.cmd
REM Optional with parameters (passed through):
scripts\install.cmd -DesktopShortcut
```

The script will automatically elevate to Administrator if needed.

### C) Portable use (no installation)

If you do not want to install:

1. Extract the release ZIP to any folder.
2. Double-click `Zentify Web UI.cmd` to start the local web UI with admin rights and open the browser.
3. Alternatively, run the CLI directly via `zentify-cleaner.exe` (non-admin) in a console.

## Usage

```powershell
# In a new terminal (user or admin):
zentify-cleaner [--dry-run] [--verbose] [--quiet] [--exact-stats]
```

- Flags/arguments:
  - `--dry-run`: Does not delete anything, only shows what would be deleted.
  - `--verbose`: More detailed output (overrides `quiet`).
  - `--quiet`: Suppresses most output.
  - `--exact-stats`: Compute exact byte totals (slower). Without this flag, a faster approximate byte calculation is used for better performance.
- Configuration priority: CLI flag > environment variables > `.zentify/config.json`.
- System-wide cleanup if env var `ZENTIFY_ALLOW_SYSTEM_CLEAN=1` is set OR when running with Administrator rights.
- During system clean, `Prefetch` is additionally cleaned.
- When launched from Windows Explorer (e.g., right-click → “Run as administrator”), the console window stays open at the end until you press Enter.
- Output: Summary (numbers of files/dirs/links, freed bytes, runtime) and “Aggressive cleaning complete.”

More examples:

```powershell
# Force system-wide clean (incl. Prefetch) without admin:
$env:ZENTIFY_ALLOW_SYSTEM_CLEAN = '1'
zentify-cleaner

# Explicitly enable Prefetch without admin:
$env:ZENTIFY_PREFETCH = '1'
zentify-cleaner --verbose

# Limit parallelism to 4 threads (useful under I/O pressure):
$env:ZENTIFY_MAX_PARALLELISM = '4'
zentify-cleaner --quiet
```

Dry-run via configuration file:

```jsonc
// .zentify/config.json (in the working directory or under %APPDATA%/Zentify or %PROGRAMDATA%/Zentify)
{
  "dry_run": true,
  "verbose": true,
  "quiet": false
}
```

Then simply run `zentify-cleaner`.

## Web UI (Rust/Axum)

A lightweight local web interface is included (optional, feature flag `web`). It is based on Rust (Axum) and controls the CLI in the background.

Start:

```powershell
# Development
cargo run --bin zentify-web --features web --release

# or after build
cargo build --bin zentify-web --features web --release --locked
./target/release/zentify-web.exe  # Windows
```

By default, the UI listens on `http://127.0.0.1:7878/`. Bindings to non-loopback addresses are denied by default.

### Launching the GUI (Windows, after installation)

- Via Start Menu: `Zentify Cleaner Web UI` (starts server and browser)
- Alternatively in the installation folder: double-click `C:\Program Files\Zentify Cleaner\Zentify Web UI.cmd`
- Optionally a Desktop shortcut is created if `-DesktopShortcut` was used

Note: The `zentify-web` binary is only built when the `web` feature is enabled (e.g., `--features web`). The CLI `zentify-cleaner` is independent of this.

Optional environment variables:

```powershell
$env:ZENTIFY_WEB_BIND = '127.0.0.1:8080'     # Change bind address (loopback recommended)
$env:ZENTIFY_CLEANER_PATH = 'C:\\Program Files\\Zentify Cleaner\\zentify-cleaner.exe' # Override CLI path
$env:ZENTIFY_WEB_ALLOW_NON_LOCAL = '1'       # Explicitly allow non-loopback bind (not recommended)
$env:ZENTIFY_WEB_ALLOW_PATH_FALLBACK = '1'   # Allow fallback to PATH for zentify-cleaner (otherwise only co-location/override)
```

Endpoints:

- `/` – Web UI
- `/api/health` – Health check
- `/api/run` (POST, JSON) – starts the cleaner with flags/env (requires `X-CSRF-Token` header, see below)

Example request to `/api/run`:

```json
{
  "dry_run": true,
  "verbose": true,
  "quiet": false,
  "exact_stats": false,
  "allow_system_clean": false,
  "prefetch": false,
  "max_parallelism": 4
}
```

Web UI security measures:

- CSRF protection: Before any POST to `/api/run`, a valid CSRF token must be sent in the `X-CSRF-Token` header. Obtain the token from `/api/csrf`.
- Loopback enforcement: Binding to non-loopback addresses is refused unless `ZENTIFY_WEB_ALLOW_NON_LOCAL=1` is set.
- CLI path resolution: By default, PATH is NOT used. The web frontend uses either an explicitly set path (`ZENTIFY_CLEANER_PATH`) or a binary located next to `zentify-web`. Fallback to PATH is only possible with `ZENTIFY_WEB_ALLOW_PATH_FALLBACK=1`.

Security note: The Web UI is intended for local use and by default binds only to `127.0.0.1`. Do not expose the UI to the network/Internet.

## Configuration

The cleaner optionally reads a JSON configuration from the following locations (first found wins):

1. Project/working directory: `.zentify/config.json`
2. `%PROGRAMDATA%/Zentify/config.json`
3. `%APPDATA%/Zentify/config.json`

Minimal example:

```json
{
  "dry_run": false,
  "verbose": false,
  "quiet": false,
  "exact_stats": false,
  "categories": {
    "user_temp": true,
    "browser_cache": true,
    "thumbnails": true,
    "windows_temp": true,
    "windows_update": true,
    "delivery_optimization": true,
    "crash_dumps": true,
    "error_reports": true,
    "directx_cache": true,
    "temp_internet_files": true,
    "prefetch": false
  }
}
```

Explanations:

- `dry_run`: Do not delete anything; show what would be deleted.
- `verbose`/`quiet`: Control verbosity.
- `categories`: Fine-grained enable/disable of individual cleaning areas.
- System-wide areas are considered when `ZENTIFY_ALLOW_SYSTEM_CLEAN=1` is set OR the program runs with Administrator privileges.
- Unknown keys in the JSON are ignored (forward compatible).
- Note: During system clean, `prefetch` is set to `true` regardless of configuration.

### Advanced example (reserved/experimental)

The project aims for a “future-proof” configuration file. Some fields are already shown in examples but are not yet processed by the current binary – they are ignored. You can safely keep them; they represent the roadmap and future functionality.

```json
{
  "dry_run": false,
  "verbose": false,
  "quiet": false,
  "interactive": false,
  "yes": false,
  "aggressive": true,
  "min_age_days": 5,
  "max_files": 0,
  "remove_empty_dirs": true,
  "show_progress": true,
  "scan_max_depth": 10,
  "categories": {
    "windows_temp": true,
    "user_temp": true,
    "browser_cache": true,
    "windows_update": true,
    "delivery_optimization": true,
    "crash_dumps": true,
    "error_reports": true,
    "defender_cache": true,
    "thumbnails": true,
    "directx_cache": true,
    "temp_internet_files": true,
    "prefetch": false,
    "logs": true,
    "java_cache": true,
    "adobe_cache": true,
    "office_cache": true,
    "aspnet_temp": true,
    "wmp_cache": true,
    "teams_cache": true,
    "widgets_cache": true,
    "modern_apps_cache": true,
    "windows_old_cleanup": false,
    "system_cleanup_commands": false
  },
  "browser_profiles": {
    "chrome": true,
    "firefox": true,
    "edge": true,
    "brave": true,
    "opera": true,
    "vivaldi": true,
    "internet_explorer": true
  },
  "include_patterns": [],
  "excluded_patterns": [
    "desktop.ini",
    "thumbs.db",
    "ntuser.dat",
    "pagefile.sys"
  ],
  "custom_directories": []
}
```

Note: The bold “reserved” fields above (incl. `interactive`, `min_age_days`, `logs`, `windows_old_cleanup`, `system_cleanup_commands`, `include_patterns`, `excluded_patterns`, `custom_directories`, `browser_profiles`) are currently placeholders and ignored by the current program version.

## Environment Variables

- `ZENTIFY_ALLOW_SYSTEM_CLEAN`: Set to `1`/`true`/`yes`/`on` to clean system-wide areas without Administrator rights. If you run the program as Administrator, this variable is not required – system clean is automatically active.
- `ZENTIFY_FORCE_NO_SYSTEM_CLEAN`: Set to `1`/`true`/`yes`/`on` to DISABLE system-wide cleaning – even when the program runs with Administrator rights. Safety override.
- `ZENTIFY_PREFETCH`: Set to `1`/`true`/`yes`/`on` to explicitly enable cleaning of the `Prefetch` folder (independent of configuration).
- `ZENTIFY_MAX_PARALLELISM`: Integer (>0), limits parallel worker threads. Default is `min(number_of_cpu_cores, 8)`.
- `ZENTIFY_WEB_BIND`: Bind address for the Web UI (default `127.0.0.1:7878`).
- `ZENTIFY_WEB_ALLOW_NON_LOCAL`: If `1`, the Web UI may bind to non-loopback addresses. Default is `0` (denied). Not recommended.
- `ZENTIFY_WEB_ALLOW_PATH_FALLBACK`: If `1`, the web frontend may use `zentify-cleaner` from `PATH` as a fallback. Default is `0`.
- `ZENTIFY_WEB_RUN_TIMEOUT_SECS`: Maximum runtime for a cleaner run via the Web API (default: `600`). On timeout, the process is terminated and the request returns 408.

Examples:

```powershell
$env:ZENTIFY_ALLOW_SYSTEM_CLEAN = '1'  # Allow system clean (without admin)
$env:ZENTIFY_FORCE_NO_SYSTEM_CLEAN = '1'  # Forbid system clean (even as admin)
$env:ZENTIFY_PREFETCH = '1'  # Explicitly enable Prefetch cleanup
$env:ZENTIFY_MAX_PARALLELISM = '4'  # 4 parallel workers
```

The variables apply to the respective shell session.

## FAQ

- How safe is the tool?
  - It aggressively deletes caches/temps. Safeguards prevent high-risk, top-level system paths from being cleaned as a whole. Use `dry_run` for a trial run and close applications before running.
- Do I need Administrator rights?
  - For user directories only: no. For system-wide areas and `Prefetch`: yes, or set `ZENTIFY_ALLOW_SYSTEM_CLEAN=1` (with care!).
- What happens with locked files?
  - Locked paths cannot be removed; the tool attempts a shallow cleanup. Close/restart applications and run the cleaner again.
- Do you support Chrome/Edge/Brave/Vivaldi/Opera/Firefox?
  - Yes, the common Chromium-based browsers and Firefox are supported, provided profiles/paths exist.
- Are CLI flags like `--dry-run` supported?
  - Yes. Supported flags are `--dry-run`, `--verbose`, `--quiet`. Flags take precedence over env vars and file configuration.
- Will unknown configuration keys cause errors?
  - No, unknown keys are currently ignored.

## Troubleshooting

1. Close all browsers/apps that may hold caches open.
2. Run as Administrator if needed (right-click → “Run as administrator”).
3. Start with `dry_run=true` and `verbose=true` to see exactly what would happen.
4. Reduce `ZENTIFY_MAX_PARALLELISM` if I/O pressure is too high.
5. Check whether the expected paths exist (some targets are only cleaned if folders exist).

## Known Limitations

- Only Windows (10/11, 64-bit) is supported.
- No interactive confirmation or selective runtime filtering (design choice; see roadmap for optional interactivity/flags).
- Best-effort with reparse points: Links are removed as such; contents behind junctions are not traversed.
- UWP/Store apps: Clearing `LocalCache/TempState` may cause initial re-initializations.
- Prefetch cleanup can temporarily slow down the first restarts of apps/Windows (normal and intended).

## Uninstall

PowerShell:

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\uninstall.ps1
# Optional: also remove user data under %APPDATA%\Zentify
pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\uninstall.ps1 -PurgeConfig
```

CMD:

```bat
scripts\uninstall.cmd
```

Uninstall stops running processes (`zentify-web`, `zentify-cleaner`), removes Start Menu and optional Desktop shortcuts as well as the GUI launcher (`Zentify Web UI.cmd`), and deletes the installation folder. It also cleans up the `PATH` entry.

## Build from Source

```powershell
# Release build
cargo build --release --locked

# Run (without installation)
./target/release/zentify-cleaner.exe
```

Recommended dev checks:

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-features --locked --verbose
```

## CI/CD

This repo contains a GitHub Actions pipeline (`.github/workflows/ci.yml`) with:

- Lint (fmt + clippy)
- Tests (Windows)
- Coverage (llvm-cov)
- MSRV build (1.70)
- Security audit (cargo-audit)
- Release job for Windows (artifact incl. CLI + Web UI + launcher, SHA256)

## Roadmap

The roadmap is multi-stage and prioritizes stability, transparency, and safe defaults. Timelines are indicative.

Phase 1 – CLI & UX

- `--dry-run`, `--verbose`, `--quiet` as CLI flags (besides file configuration)
- `--version`, `--help` and refined exit codes
- Improved output structure with clear sections (startup context, effective categories, summary)

Phase 2 – Configuration v1

- Validated schema (JSON Schema) for `.zentify/config.json` with warnings for unknown keys (still tolerant)
- Exclusion patterns (`excluded_patterns`) and inclusion patterns (`include_patterns`) per category
- Custom directories (`custom_directories`) with category assignment
- Per-app profiles (finer enabling/disabling for browsers/Teams/Office etc.)

Phase 3 – Logging & Observability

- Structured logs (e.g., JSON lines) with optional `--log-format json`
- Export dry-run results as JSON/CSV (e.g., `--output report.json`)
- Detailed metrics (duration per target, error classes, number of locked objects)

Phase 4 – Robustness & Error Handling

- Improved handling of locked files (retry strategies, clear hints to responsible processes where possible)
- More granular deletion fallbacks (partial cleanup per level with progress indicator)
- Optional “skip on error” per category vs. “fail fast”

Phase 5 – Performance

- Adaptive parallelism (I/O backpressure, dynamic thread count)
- Prioritize large disk hogs (clean large files/folders first for visible free space gains)
- Optional I/O scheduler optimized patterns for HDD vs. SSD

Phase 6 – Extended Areas

- More caches (Visual Studio/.vs, NuGet, npm/yarn/pnpm, Python pip cache, Gradle/Maven), optionally disabled by default
- Finer Windows Update cleanup (respecting WU/Known Issues, purely optional and transparent)
- Optional log cleanup (rotating logs, size limits) – strict opt-in

Phase 7 – Integrations & Packaging

- Official Winget/Chocolatey package
- Signed releases, additional artifacts (portable/installer variants), `README.de.md/README.en.md`
- Optional shell integration script (context menu “Run Zentify Cleaner”)

Phase 8 – Security & Quality

- Extended tests on Windows (integration tests with temporary sandbox directories)
- Regular `cargo-audit`/MSRV updates (already in CI, continue maintenance)
- Threat model documentation and security guidelines

Nice-to-have / Research

- Interactive mode (`interactive: true`) with per-category selection (opt-in only)
- Progress indicator in TTY (e.g., `--progress`) vs. non-TTY (reduced output)
- Optional quarantine (recycle-bin-like) instead of immediate deletion, with automatic retention cleanup

## Contributing

Contributions are welcome! Please follow these guidelines:

- Check code style: `cargo fmt --all -- --check`.
- Static analysis: `cargo clippy --all-targets -- -D warnings`.
- Run tests: `cargo test --all-features --locked --verbose`.
- Pull requests should briefly describe what changed and, if possible, include screenshots/logs showing differences in output.
- Adhere to the project’s security principles (no risky default actions, clear opt-ins, good error messages).

Please open an issue with a clear problem or feature description for questions/ideas.

## Support

- Please report issues/bugs via GitHub Issues (include OS version, log excerpts, and – if possible – `dry_run` output).
- Feature requests are welcome; mark them as “Feature Request” and describe the use case.
- Please report security-related topics responsibly (coordinated disclosure).

## License

MIT – see [`LICENSE`](./LICENSE).

---

© 2025 Zentify Team
