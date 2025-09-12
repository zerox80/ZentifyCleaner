
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::io::{self, Write};
use serde::Deserialize;
use clap::Parser;
use log::{debug, info};
use zentify_cleaner::{load_config as core_load_config, run_clean as core_run_clean, RunOverrides as CoreRunOverrides};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
#[cfg(windows)]
use windows_sys::Win32::System::Console::{GetConsoleWindow, GetConsoleProcessList};
#[cfg(windows)]
use windows_sys::Win32::Security::{
    CheckTokenMembership, CreateWellKnownSid, SECURITY_MAX_SID_SIZE, WinBuiltinAdministratorsSid,
};

#[cfg(windows)]
fn main() {
    // Parse CLI flags
    let cli = Cli::parse();

    // Load configuration (optional) and apply CLI overrides (CLI > Env > Config)
    let mut cfg = core_load_config();
    if cli.dry_run { cfg.dry_run = true; }
    if cli.verbose { cfg.verbose = true; cfg.quiet = false; }
    if cli.quiet { cfg.quiet = true; cfg.verbose = false; }
    if cli.exact_stats { cfg.exact_stats = true; }

    init_logging(cfg.quiet, cfg.verbose);

    // Determine if system-level cleaning is allowed
    let mut allow_system = env_truthy("ZENTIFY_ALLOW_SYSTEM_CLEAN");
    if !allow_system && is_elevated() { allow_system = true; }
    if env_truthy("ZENTIFY_FORCE_NO_SYSTEM_CLEAN") { allow_system = false; }

    // run timing is reported by the library summary

    // Build overrides from env toggles
    let prefetch_override = if env_truthy("ZENTIFY_PREFETCH") { Some(true) } else { None };
    let max_par = std::env::var("ZENTIFY_MAX_PARALLELISM").ok().and_then(|s| s.parse::<usize>().ok());
    let overrides = CoreRunOverrides { allow_system, prefetch: prefetch_override, max_parallelism: max_par };

    // Execute cleaning via library
    let summary = core_run_clean(&cfg, &overrides);

    if !cfg.quiet {
        if cfg.dry_run {
            println!(
                "Dry-run summary: would remove {} files, {} dirs, {} links; free approx {} ({} bytes) in {:?}.",
                summary.files_deleted,
                summary.dirs_deleted,
                summary.links_removed,
                format_bytes(summary.bytes_freed),
                summary.bytes_freed,
                summary.elapsed
            );
        } else {
            println!(
                "Summary: removed {} files, {} dirs, {} links; freed {} ({} bytes) in {:?}.",
                summary.files_deleted,
                summary.dirs_deleted,
                summary.links_removed,
                format_bytes(summary.bytes_freed),
                summary.bytes_freed,
                summary.elapsed
            );
            if !summary.exact_stats {
                println!("Note: Byte counts for directories are approximate (fast mode). Use --exact-stats for precise totals.");
            }
        }
        println!("Aggressive cleaning complete.");
    }

    // If launched from Explorer (own console), keep window open until user presses Enter
    if should_pause_on_exit() { pause_console(); }
}

// ---------- CLI ----------

#[derive(Debug, Parser)]
#[command(name = "zentify-cleaner", version, author, about = "Minimal, fast Windows temp cleaner (Windows 10/11)")]
struct Cli {
    /// Do not delete anything, only print what would be deleted
    #[arg(long)]
    dry_run: bool,

    /// Increase verbosity (overrides quiet)
    #[arg(long)]
    verbose: bool,

    /// Silence most output
    #[arg(long)]
    quiet: bool,

    /// Compute exact freed byte counts (slower)
    #[arg(long)]
    exact_stats: bool,
}

fn init_logging(quiet: bool, verbose: bool) {
    let default_level = if quiet {
        "error"
    } else if verbose {
        "debug"
    } else {
        "info"
    };
    let env = env_logger::Env::default().default_filter_or(default_level);
    let _ = env_logger::Builder::from_env(env).is_test(false).try_init();
    debug!("Logger initialized with level: {}", default_level);
    info!("Starting Zentify Cleaner");
}

fn dedup_paths(v: &mut Vec<PathBuf>) {
    v.retain(|p| !p.as_os_str().is_empty());
    v.sort();
    v.dedup();
}

/// Build a conservative list of allowed root prefixes under which we will operate.
/// This is an extra safety net on top of explicit path selection and sensitive-dir checks.
fn allowed_prefixes(allow_system: bool) -> Vec<PathBuf> {
    let mut bases: Vec<PathBuf> = Vec::new();
    // User-scoped bases
    bases.push(std::env::temp_dir());
    if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
        bases.push(PathBuf::from(localappdata));
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        bases.push(PathBuf::from(appdata));
    }
    // System-scoped bases (only if explicitly allowed)
    if allow_system {
        if let Ok(windir) = std::env::var("WINDIR") { bases.push(PathBuf::from(windir)); }
        if let Ok(systemroot) = std::env::var("SystemRoot") { bases.push(PathBuf::from(systemroot)); }
        if let Ok(programdata) = std::env::var("ProgramData") { bases.push(PathBuf::from(programdata)); }
    }
    // Normalize: keep only absolute, existing directories, dedup
    bases.retain(|p| p.is_dir());
    bases.sort();
    bases.dedup();
    bases
}

fn canonicalize_ok(p: &Path) -> Option<PathBuf> { p.canonicalize().ok() }

fn is_under(path: &Path, base: &Path) -> bool {
    match (canonicalize_ok(path), canonicalize_ok(base)) {
        (Some(cp), Some(cb)) => cp.starts_with(cb),
        _ => false,
    }
}

fn retain_allowed_paths(v: &mut Vec<PathBuf>, allow_system: bool) {
    let bases = allowed_prefixes(allow_system);
    if bases.is_empty() { return; }
    v.retain(|p| bases.iter().any(|b| is_under(p, b)));
}

fn candidate_dirs(cats: &Categories, allow_system: bool) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();

    // User temp
    if cats.user_temp {
        v.push(std::env::temp_dir());
        if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
            v.push(Path::new(&localappdata).join("Temp"));
            if cats.directx_cache { v.push(Path::new(&localappdata).join("D3DSCache")); }
            if cats.temp_internet_files {
                v.push(Path::new(&localappdata).join("Microsoft/Windows/INetCache"));
                // Legacy WebCache (ESE) used by IE/Legacy Edge/Explorer
                v.push(Path::new(&localappdata).join("Microsoft/Windows/WebCache"));
            }
            if cats.crash_dumps {
                v.push(Path::new(&localappdata).join("CrashDumps"));
            }
            // User-level WER
            if cats.error_reports {
                v.push(Path::new(&localappdata).join("Microsoft/Windows/WER/ReportQueue"));
                v.push(Path::new(&localappdata).join("Microsoft/Windows/WER/ReportArchive"));
                v.push(Path::new(&localappdata).join("Microsoft/Windows/WER/Temp"));
            }
            // Windows Widgets (WebExperience) cache
            if cats.widgets_cache {
                let widgets_pkg = Path::new(&localappdata).join("Packages/MicrosoftWindows.Client.WebExperience_cw5n1h2txyewy");
                v.push(widgets_pkg.join("LocalCache"));
                v.push(widgets_pkg.join("TempState"));
            }
            // Teams (Classic) caches
            if cats.teams_cache {
                v.push(Path::new(&localappdata).join("Packages/MSTeams_8wekyb3d8bbwe/LocalCache")); // New Teams (UWP)
            }
            // Office Document Cache
            if cats.office_cache {
                v.push(Path::new(&localappdata).join("Microsoft/Office/16.0/OfficeFileCache"));
            }
            // NVIDIA shader caches (tie to directx_cache category)
            if cats.directx_cache {
                v.push(Path::new(&localappdata).join("NVIDIA/GLCache"));
                v.push(Path::new(&localappdata).join("NVIDIA/DXCache"));
            }
            // Windows Media Player cache (best-effort)
            if cats.wmp_cache {
                v.push(Path::new(&localappdata).join("Microsoft/Media Player/Cache"));
            }
            // Java cache
            if cats.java_cache {
                v.push(Path::new(&localappdata).join("Sun/Java/Deployment/cache"));
            }
            // Adobe media caches (common locations)
            if cats.adobe_cache {
                v.push(Path::new(&localappdata).join("Adobe/Common/Media Cache"));
                v.push(Path::new(&localappdata).join("Adobe/Common/Media Cache Files"));
            }
        }
        if let Ok(appdata) = std::env::var("APPDATA") {
            if cats.teams_cache {
                let teams_root = Path::new(&appdata).join("Microsoft/Teams");
                v.push(teams_root.join("Cache"));
                v.push(teams_root.join("GPUCache"));
                v.push(teams_root.join("Service Worker/CacheStorage"));
                v.push(teams_root.join("IndexedDB"));
                v.push(teams_root.join("Local Storage"));
            }
        }
    }

    // System-level only if allowed explicitly
    if allow_system {
        if let Ok(windir) = std::env::var("WINDIR") {
            if cats.windows_temp { v.push(Path::new(&windir).join("Temp")); }
            if cats.prefetch { v.push(Path::new(&windir).join("Prefetch")); }
            if cats.windows_update { v.push(Path::new(&windir).join("SoftwareDistribution/Download")); }
            if cats.crash_dumps {
                v.push(Path::new(&windir).join("Minidump"));
                v.push(Path::new(&windir).join("LiveKernelReports"));
            }
            if cats.aspnet_temp {
                v.push(Path::new(&windir).join("Microsoft.NET/Framework/v4.0.30319/Temporary ASP.NET Files"));
                v.push(Path::new(&windir).join("Microsoft.NET/Framework64/v4.0.30319/Temporary ASP.NET Files"));
            }
        }
        if let Ok(systemroot) = std::env::var("SystemRoot") {
            if cats.windows_temp { v.push(Path::new(&systemroot).join("Temp")); }
            if cats.windows_update { v.push(Path::new(&systemroot).join("SoftwareDistribution/Download")); }
        }
        if let Ok(programdata) = std::env::var("ProgramData") {
            if cats.delivery_optimization { v.push(Path::new(&programdata).join("Microsoft/Windows/DeliveryOptimization/Cache")); }
            if cats.error_reports {
                v.push(Path::new(&programdata).join("Microsoft/Windows/WER/ReportQueue"));
                v.push(Path::new(&programdata).join("Microsoft/Windows/WER/ReportArchive"));
                v.push(Path::new(&programdata).join("Microsoft/Windows/WER/Temp"));
            }
            if cats.defender_cache {
                v.push(Path::new(&programdata).join("Microsoft/Windows Defender/Scans/History"));
            }
        }
    }

    // Modern Apps (UWP) LocalCache/TempState
    if cats.modern_apps_cache {
        if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
            let packages = Path::new(&localappdata).join("Packages");
            if packages.is_dir() {
                if let Ok(rd) = fs::read_dir(&packages) {
                    for e in rd.flatten() {
                        let p = e.path();
                        if p.is_dir() {
                            v.push(p.join("LocalCache"));
                            v.push(p.join("TempState"));
                        }
                    }
                }
            }
        }
    }

    // Keep only existing directories
    v.retain(|p| p.is_dir());
    v
}

fn candidate_files(cats: &Categories, allow_system: bool) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    if cats.thumbnails {
        if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
            let explorer = Path::new(&localappdata).join("Microsoft/Windows/Explorer");
            if explorer.is_dir() {
                if let Ok(rd) = fs::read_dir(&explorer) {
                    for e in rd.flatten() {
                        let p = e.path();
                        if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                            let lower = name.to_ascii_lowercase();
                            if (lower.starts_with("thumbcache") || lower.starts_with("iconcache")) && lower.ends_with(".db") {
                                v.push(p);
                            }
                        }
                    }
                }
            }
        }
    }
    // Large system dump file
    if allow_system && cats.crash_dumps {
        if let Ok(windir) = std::env::var("WINDIR") {
            let memdmp = Path::new(&windir).join("MEMORY.DMP");
            if memdmp.is_file() {
                v.push(memdmp);
            }
        }
    }
    v
}

fn fast_clean_dir(dir: &Path, cfg: &Config, stats: &Stats) {
    let dry_run = cfg.dry_run;
    let verbose = cfg.verbose && !cfg.quiet;
    if !dir.is_dir() { return; }
    // Do not operate on filesystem roots (e.g., C:\)
    if dir.parent().is_none() { return; }
    // Extra safety: never operate on highly sensitive top-level system directories
    if is_sensitive_dir(dir) { return; }
    // Avoid traversing reparse points (junctions/symlinks) to reduce risk
    if is_reparse_point(dir) {
        // Best-effort: remove the link itself
        if dry_run {
            if verbose { println!("[dry-run] Would remove reparse link: {}", dir.display()); }
            stats.add_links(1);
            return;
        }
        if fs::remove_dir(dir).or_else(|_| fs::remove_file(dir)).is_ok() {
            stats.add_links(1);
        }
        return;
    }
    // Try to remove entirely, otherwise fall back to shallow file cleanup
    if dry_run {
        let (bytes, files, dirs) = compute_dir_stats(dir);
        if verbose { println!("[dry-run] Would remove dir (all): {} ({} files, {} dirs, {} bytes)", dir.display(), files, dirs, bytes); }
        stats.add_bytes(bytes);
        stats.add_files(files);
        stats.add_dirs(dirs + 1); // include the root dir
        return;
    }
    // Fast path: in fast mode, attempt removal without pre-counting
    if !cfg.exact_stats {
        // Versuche das Verzeichnis selbst schreibbar zu machen, damit remove_dir_all nicht an Readonly-Attributen scheitert
        set_writable(dir);
        if fs::remove_dir_all(dir).is_ok() {
            // We do not know exact bytes/files removed in fast mode
            stats.add_dirs(1); // count the root dir removed
            return;
        }
    } else {
        // Exact stats mode: pre-count to report accurate freed bytes
        let (bytes_all, files_all, mut dirs_all) = compute_dir_stats(dir);
        dirs_all += 1; // include the root dir
        // Auch hier vorab schreibbar setzen
        set_writable(dir);
        if fs::remove_dir_all(dir).is_ok() {
            stats.add_bytes(bytes_all);
            stats.add_files(files_all);
            stats.add_dirs(dirs_all);
            return;
        }
    }
    // Shallow cleanup on failure
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if is_reparse_point(&p) {
                // Remove the link itself (dir or file-like reparse)
                if dry_run {
                    if verbose { println!("[dry-run] Would remove reparse link: {}", p.display()); }
                    stats.add_links(1);
                    continue;
                }
                if fs::remove_dir(&p).or_else(|_| fs::remove_file(&p)).is_ok() {
                    stats.add_links(1);
                }
                continue;
            }
            if p.is_dir() {
                set_writable(&p);
                let (bytes, files, mut dirs) = compute_dir_stats(&p);
                if dry_run {
                    if verbose { println!("[dry-run] Would remove dir: {} ({} files, {} dirs, {} bytes)", p.display(), files, dirs, bytes); }
                    stats.add_bytes(bytes);
                    stats.add_files(files);
                    stats.add_dirs(dirs + 1);
                } else {
                    if fs::remove_dir_all(&p).is_ok() {
                        stats.add_bytes(bytes);
                        stats.add_files(files);
                        dirs += 1; // include the dir itself
                        stats.add_dirs(dirs);
                    }
                }
            } else {
                set_writable(&p);
                let size = fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                if dry_run {
                    if verbose { println!("[dry-run] Would remove file: {} ({} bytes)", p.display(), size); }
                    stats.add_bytes(size);
                    stats.add_files(1);
                } else {
                    if fs::remove_file(&p).is_ok() {
                        stats.add_bytes(size);
                        stats.add_files(1);
                        if verbose { println!("Removed file: {} ({} bytes)", p.display(), size); }
                    }
                }
            }
        }
    }
}

#[cfg(windows)]
fn is_reparse_point(p: &Path) -> bool {
    if let Ok(md) = fs::symlink_metadata(p) {
        // FILE_ATTRIBUTE_REPARSE_POINT = 0x0400
        (md.file_attributes() & 0x0400) != 0
    } else {
        false
    }
}

#[cfg(not(windows))]
fn is_reparse_point(_p: &Path) -> bool { false }

fn set_writable(path: &Path) {
    if let Ok(metadata) = fs::metadata(path) {
        let mut perms = metadata.permissions();
        if perms.readonly() {
            // Ignore errors: best-effort
            perms.set_readonly(false);
            let _ = fs::set_permissions(path, perms);
        }
    }
}

fn is_sensitive_dir(p: &Path) -> bool {
    // Only meaningful on Windows, but safe elsewhere
    let full = match p.canonicalize() { Ok(x) => x, Err(_) => return false };
    let full_str = full.to_string_lossy().to_ascii_lowercase();

    let mut sens: Vec<String> = Vec::new();
    if let Ok(windir) = std::env::var("WINDIR") {
        sens.push(Path::new(&windir).to_string_lossy().to_ascii_lowercase());
    }
    if let Ok(systemroot) = std::env::var("SystemRoot") {
        sens.push(Path::new(&systemroot).to_string_lossy().to_ascii_lowercase());
    }
    if let Ok(pf) = std::env::var("ProgramFiles") {
        sens.push(Path::new(&pf).to_string_lossy().to_ascii_lowercase());
    }
    if let Ok(pfx86) = std::env::var("ProgramFiles(x86)") {
        sens.push(Path::new(&pfx86).to_string_lossy().to_ascii_lowercase());
    }
    if let Ok(pd) = std::env::var("ProgramData") {
        sens.push(Path::new(&pd).to_string_lossy().to_ascii_lowercase());
    }
    if let Ok(sysdrive) = std::env::var("SystemDrive") {
        // Users root (e.g., C:\\Users)
        let users_root_str = format!("{}\\Users", sysdrive);
        let users_root = Path::new(&users_root_str);
        sens.push(users_root.to_string_lossy().to_ascii_lowercase());
    }

    sens.iter().any(|s| full_str == *s)
}

fn push_chromium_caches(targets: &mut Vec<PathBuf>, product_root: PathBuf) {
    // product_root like %LOCALAPPDATA%/Google/Chrome
    let user_data = product_root.join("User Data");
    if !user_data.is_dir() { return; }
    if let Ok(rd) = fs::read_dir(&user_data) {
        for e in rd.flatten() {
            let profile = e.path();
            if !profile.is_dir() { continue; }
            // Common cache subfolders in Chromium-based browsers
            targets.push(profile.join("Cache"));
            targets.push(profile.join("Code Cache"));
            targets.push(profile.join("GPUCache"));
            targets.push(profile.join("ShaderCache"));
            targets.push(profile.join("DawnCache"));
            targets.push(profile.join("GrShaderCache"));
            targets.push(profile.join("Media Cache"));
            targets.push(profile.join("Service Worker/CacheStorage"));
            targets.push(profile.join("Application Cache"));
            targets.push(profile.join("Network/Cache"));
        }
    }
    // Product-wide shader cache (not per-profile)
    targets.push(user_data.join("ShaderCache/GPUCache"));
}

// ---------- Config ----------

#[derive(Debug, Clone, Deserialize)]
struct Categories {
    #[serde(default = "true_bool")] windows_temp: bool,
    #[serde(default = "true_bool")] user_temp: bool,
    #[serde(default = "true_bool")] browser_cache: bool,
    #[serde(default = "true_bool")] windows_update: bool,
    #[serde(default = "true_bool")] delivery_optimization: bool,
    #[serde(default = "true_bool")] crash_dumps: bool,
    #[serde(default = "true_bool")] error_reports: bool,
    #[serde(default = "true_bool")] thumbnails: bool,
    #[serde(default = "true_bool")] directx_cache: bool,
    #[serde(default = "true_bool")] temp_internet_files: bool,
    #[serde(default = "false_bool")] prefetch: bool,
    // Newer categories
    #[serde(default = "true_bool")] defender_cache: bool,
    #[serde(default = "true_bool")] office_cache: bool,
    #[serde(default = "true_bool")] aspnet_temp: bool,
    #[serde(default = "true_bool")] teams_cache: bool,
    #[serde(default = "true_bool")] modern_apps_cache: bool,
    #[serde(default = "true_bool")] java_cache: bool,
    #[serde(default = "true_bool")] adobe_cache: bool,
    #[serde(default = "true_bool")] wmp_cache: bool,
    #[serde(default = "true_bool")] widgets_cache: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct Config {
    #[serde(default)] dry_run: bool,
    #[serde(default)] verbose: bool,
    #[serde(default)] quiet: bool,
    #[serde(default)] exact_stats: bool,
    #[serde(default)] categories: Option<Categories>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dry_run: false,
            verbose: false,
            quiet: false,
            exact_stats: false,
            categories: Some(Categories {
                windows_temp: true,
                user_temp: true,
                browser_cache: true,
                windows_update: true,
                delivery_optimization: true,
                crash_dumps: true,
                error_reports: true,
                thumbnails: true,
                directx_cache: true,
                temp_internet_files: true,
                prefetch: false,
                defender_cache: true,
                office_cache: true,
                aspnet_temp: true,
                teams_cache: true,
                modern_apps_cache: true,
                java_cache: true,
                adobe_cache: true,
                wmp_cache: true,
                widgets_cache: true,
            }),
        }
    }
}

impl Config {
    fn effective_categories(&self) -> Categories {
        self.categories.clone().unwrap_or(Categories {
            windows_temp: true,
            user_temp: true,
            browser_cache: true,
            windows_update: true,
            delivery_optimization: true,
            crash_dumps: true,
            error_reports: true,
            thumbnails: true,
            directx_cache: true,
            temp_internet_files: true,
            prefetch: false,
            defender_cache: true,
            office_cache: true,
            aspnet_temp: true,
            teams_cache: true,
            modern_apps_cache: true,
            java_cache: true,
            adobe_cache: true,
            wmp_cache: true,
            widgets_cache: true,
        })
    }
}

fn load_config() -> Config {
    // Search order: CWD/.zentify/config.json, %PROGRAMDATA%/Zentify/config.json, %APPDATA%/Zentify/config.json
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() { paths.push(cwd.join(".zentify/config.json")); }
    if let Ok(pd) = std::env::var("ProgramData") { paths.push(Path::new(&pd).join("Zentify/config.json")); }
    if let Ok(ad) = std::env::var("APPDATA") { paths.push(Path::new(&ad).join("Zentify/config.json")); }

    for p in paths {
        if p.is_file() {
            if let Ok(s) = fs::read_to_string(&p) {
                if let Ok(mut c) = serde_json::from_str::<Config>(&s) {
                    // Merge with defaults
                    let def = Config::default();
                    if c.categories.is_none() { c.categories = def.categories; }
                    return c;
                }
            }
        }
    }
    Config::default()
}

fn env_truthy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => {
            let v = v.trim();
            matches!(v, "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
        }
        Err(_) => false,
    }
}

fn true_bool() -> bool { true }
fn false_bool() -> bool { false }

// ---------- Stats & Helpers ----------

#[derive(Default)]
struct Stats {
    files_deleted: AtomicU64,
    dirs_deleted: AtomicU64,
    links_removed: AtomicU64,
    bytes_freed: AtomicU64,
}

impl Stats {
    fn add_files(&self, n: u64) { self.files_deleted.fetch_add(n, Ordering::Relaxed); }
    fn add_dirs(&self, n: u64) { self.dirs_deleted.fetch_add(n, Ordering::Relaxed); }
    fn add_links(&self, n: u64) { self.links_removed.fetch_add(n, Ordering::Relaxed); }
    fn add_bytes(&self, n: u64) { self.bytes_freed.fetch_add(n, Ordering::Relaxed); }
    fn snapshot(&self) -> (u64, u64, u64, u64) {
        (
            self.files_deleted.load(Ordering::Relaxed),
            self.dirs_deleted.load(Ordering::Relaxed),
            self.links_removed.load(Ordering::Relaxed),
            self.bytes_freed.load(Ordering::Relaxed),
        )
    }
}

fn compute_dir_stats(root: &Path) -> (u64, u64, u64) {
    // bytes, files, dirs (excluding root)
    let mut bytes: u64 = 0;
    let mut files: u64 = 0;
    let mut dirs: u64 = 0;
    if !root.is_dir() { return (0, 0, 0); }
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        if let Ok(rd) = fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                if is_reparse_point(&p) { continue; }
                if p.is_dir() {
                    dirs += 1;
                    stack.push(p);
                } else {
                    files += 1;
                    bytes = bytes.saturating_add(fs::metadata(&p).map(|m| m.len()).unwrap_or(0));
                }
            }
        }
    }
    (bytes, files, dirs)
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{} {}", bytes, UNITS[unit]) } else { format!("{:.2} {}", size, UNITS[unit]) }
}

// ---------- Elevation (Windows) ----------

#[cfg(windows)]
fn is_elevated() -> bool {
    unsafe {
        // Build the SID for the built-in Administrators group and check membership
        let mut sid = [0u8; SECURITY_MAX_SID_SIZE as usize];
        let mut sid_size: u32 = SECURITY_MAX_SID_SIZE as u32;
        let sid_ptr = sid.as_mut_ptr() as *mut core::ffi::c_void;
        if CreateWellKnownSid(WinBuiltinAdministratorsSid, std::ptr::null_mut(), sid_ptr, &mut sid_size) == 0 {
            return false;
        }
        let mut is_member: i32 = 0;
        if CheckTokenMembership(std::ptr::null_mut(), sid_ptr as _, &mut is_member) == 0 {
            return false;
        }
        is_member != 0
    }
}

#[cfg(not(windows))]
fn is_elevated() -> bool { false }

// ---------- Console pause logic (Windows) ----------

#[cfg(windows)]
fn should_pause_on_exit() -> bool {
    unsafe {
        // If no console window, nothing to pause (unlikely for console app)
        let hwnd = GetConsoleWindow();
        if hwnd.is_null() { return false; }
        // If only this process is attached to the console, it likely was launched from Explorer
        let mut list: [u32; 2] = [0; 2];
        let count = GetConsoleProcessList(list.as_mut_ptr(), list.len() as u32);
        count <= 1
    }
}

#[cfg(windows)]
fn pause_console() {
    let _ = write!(io::stdout(), "\nZum Beenden bitte Enter drücken . . . ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);
}

#[cfg(not(windows))]
fn should_pause_on_exit() -> bool { false }

#[cfg(not(windows))]
fn pause_console() {}

#[cfg(not(windows))]
fn main() {
    eprintln!("Zentify Cleaner supports Windows 10/11 only. Exiting.");
    std::process::exit(2);
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_truthy_variants() {
        std::env::set_var("Z_TEST_TRUTHY", "1");
        assert!(env_truthy("Z_TEST_TRUTHY"));
        std::env::set_var("Z_TEST_TRUTHY", "true");
        assert!(env_truthy("Z_TEST_TRUTHY"));
        std::env::set_var("Z_TEST_TRUTHY", "on");
        assert!(env_truthy("Z_TEST_TRUTHY"));
        std::env::remove_var("Z_TEST_TRUTHY");
        assert!(!env_truthy("Z_TEST_TRUTHY"));
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.00 MB");
    }

}
