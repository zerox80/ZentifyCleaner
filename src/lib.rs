use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, atomic::{AtomicUsize, AtomicU64, Ordering}};
use std::thread;
use std::time::{Duration, Instant};
use std::process::Command;

use serde::{Deserialize, Serialize};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};
#[cfg(windows)]
use windows_sys::Win32::Security::{CheckTokenMembership, CreateWellKnownSid, SECURITY_MAX_SID_SIZE, WinBuiltinAdministratorsSid};

// Public API types
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Categories {
    #[serde(default = "true_bool")] pub windows_temp: bool,
    #[serde(default = "true_bool")] pub user_temp: bool,
    #[serde(default = "true_bool")] pub browser_cache: bool,
    #[serde(default = "true_bool")] pub windows_update: bool,
    #[serde(default = "true_bool")] pub delivery_optimization: bool,
    #[serde(default = "true_bool")] pub crash_dumps: bool,
    #[serde(default = "true_bool")] pub error_reports: bool,
    #[serde(default = "true_bool")] pub thumbnails: bool,
    #[serde(default = "true_bool")] pub directx_cache: bool,
    #[serde(default = "true_bool")] pub temp_internet_files: bool,
    #[serde(default = "false_bool")] pub prefetch: bool,
    // Newer categories
    #[serde(default = "true_bool")] pub defender_cache: bool,
    #[serde(default = "true_bool")] pub office_cache: bool,
    #[serde(default = "true_bool")] pub aspnet_temp: bool,
    #[serde(default = "true_bool")] pub teams_cache: bool,
    #[serde(default = "true_bool")] pub modern_apps_cache: bool,
    #[serde(default = "true_bool")] pub java_cache: bool,
    #[serde(default = "true_bool")] pub adobe_cache: bool,
    #[serde(default = "true_bool")] pub wmp_cache: bool,
    #[serde(default = "true_bool")] pub widgets_cache: bool,
}

pub fn preview_targets(cfg: &Config, overrides: &RunOverrides) -> TargetsPreview {
    // Determine effective categories similar to run_clean
    let mut cats = cfg.effective_categories();
    if overrides.allow_system {
        cats.prefetch = true;
    }
    if let Some(p) = overrides.prefetch {
        if p { cats.prefetch = true; }
    }

    let mut dirs = candidate_dirs(&cats, overrides.allow_system);
    let mut files = candidate_files(&cats, overrides.allow_system);

    // Apply same filters
    dirs.retain(|p| p.is_dir());
    retain_allowed_paths(&mut dirs, overrides.allow_system);
    dedup_paths(&mut dirs);

    files.retain(|p| p.is_file());
    retain_allowed_paths(&mut files, overrides.allow_system);
    files.sort();
    files.dedup();

    let target_dirs = dirs.into_iter().map(|p| p.to_string_lossy().to_string()).collect();
    let target_files = files.into_iter().map(|p| p.to_string_lossy().to_string()).collect();

    TargetsPreview { target_dirs, target_files }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)] pub dry_run: bool,
    #[serde(default)] pub verbose: bool,
    #[serde(default)] pub quiet: bool,
    #[serde(default)] pub exact_stats: bool,
    #[serde(default)] pub categories: Option<Categories>,
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
    pub fn effective_categories(&self) -> Categories {
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

pub fn load_config() -> Config {
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

#[derive(Default)]
struct Stats {
    files_deleted: AtomicU64,
    dirs_deleted: AtomicU64,
    links_removed: AtomicU64,
    bytes_freed: AtomicU64,
    cleaned_dirs: Mutex<Vec<String>>,
}

impl Stats {
    fn add_files(&self, n: u64) { self.files_deleted.fetch_add(n, Ordering::Relaxed); }
    fn add_dirs(&self, n: u64) { self.dirs_deleted.fetch_add(n, Ordering::Relaxed); }
    fn add_links(&self, n: u64) { self.links_removed.fetch_add(n, Ordering::Relaxed); }
    fn add_bytes(&self, n: u64) { self.bytes_freed.fetch_add(n, Ordering::Relaxed); }
    fn add_cleaned_dir(&self, p: &Path) {
        if let Ok(mut v) = self.cleaned_dirs.lock() {
            v.push(p.to_string_lossy().to_string());
        }
    }
    fn snapshot(&self) -> (u64, u64, u64, u64) {
        (
            self.files_deleted.load(Ordering::Relaxed),
            self.dirs_deleted.load(Ordering::Relaxed),
            self.links_removed.load(Ordering::Relaxed),
            self.bytes_freed.load(Ordering::Relaxed),
        )
    }
    fn get_cleaned_dirs(&self) -> Vec<String> {
        let mut out: Vec<String> = self.cleaned_dirs.lock().map(|v| v.clone()).unwrap_or_default();
        out.sort();
        out.dedup();
        out
    }
}

pub struct RunOverrides {
    pub allow_system: bool,
    pub prefetch: Option<bool>,
    pub max_parallelism: Option<usize>,
}

pub struct Summary {
    pub files_deleted: u64,
    pub dirs_deleted: u64,
    pub links_removed: u64,
    pub bytes_freed: u64,
    pub elapsed: Duration,
    pub dry_run: bool,
    pub exact_stats: bool,
    pub cleaned_dirs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetsPreview {
    pub target_dirs: Vec<String>,
    pub target_files: Vec<String>,
}

pub fn run_clean(cfg: &Config, overrides: &RunOverrides) -> Summary {
    let start = Instant::now();

    // Determine effective categories for this run
    let mut cats = cfg.effective_categories();
    if overrides.allow_system {
        // As Administrator, enable prefetch cleanup for more aggressive cleaning
        cats.prefetch = true;
    }
    if let Some(p) = overrides.prefetch {
        if p { cats.prefetch = true; }
    }

    // Build aggressive list of temp/cache targets
    let mut targets = candidate_dirs(&cats, overrides.allow_system);
    let mut file_targets = candidate_files(&cats, overrides.allow_system);

    // Chromium-based browsers caches (Chrome, Edge, Brave, Vivaldi, Opera GX)
    if cats.browser_cache {
        if let Ok(base) = std::env::var("LOCALAPPDATA") {
            push_chromium_caches(&mut targets, Path::new(&base).join("Google/Chrome"));
            push_chromium_caches(&mut targets, Path::new(&base).join("Microsoft/Edge"));
            push_chromium_caches(&mut targets, Path::new(&base).join("BraveSoftware/Brave-Browser"));
            push_chromium_caches(&mut targets, Path::new(&base).join("Vivaldi/Vivaldi"));
            // Opera variants
            push_chromium_caches(&mut targets, Path::new(&base).join("Opera Software/Opera GX Stable"));
            push_chromium_caches(&mut targets, Path::new(&base).join("Opera Software/Opera Stable"));
        }
    }

    // Firefox caches
    if cats.browser_cache {
        if let Ok(base) = std::env::var("LOCALAPPDATA") {
            let profiles = Path::new(&base).join("Mozilla/Firefox/Profiles");
            if profiles.is_dir() {
                if let Ok(entries) = fs::read_dir(&profiles) {
                    for e in entries.flatten() {
                        let p = e.path();
                        if p.is_dir() {
                            targets.push(p.join("cache2"));
                            targets.push(p.join("startupCache"));
                        }
                    }
                }
            }
        }
    }

    // Delete directories aggressively (with limited concurrency)
    // Filter to existing directories first to avoid overhead on nonexistent paths
    targets.retain(|p| p.is_dir());
    // Extra safety: keep only paths under allowed prefixes
    retain_allowed_paths(&mut targets, overrides.allow_system);
    dedup_paths(&mut targets);
    let avail = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let mut concurrency = avail.min(8);
    if let Some(n) = overrides.max_parallelism { concurrency = n.clamp(1, avail); }

    let targets_arc = Arc::new(targets);
    let index = Arc::new(AtomicUsize::new(0));
    let stats = Arc::new(Stats::default());
    let mut handles = Vec::new();
    for _ in 0..concurrency {
        let t = Arc::clone(&targets_arc);
        let idx = Arc::clone(&index);
        let cfg_local = cfg.clone();
        let stats_local = Arc::clone(&stats);
        handles.push(thread::spawn(move || {
            loop {
                let i = idx.fetch_add(1, Ordering::Relaxed);
                if i >= t.len() { break; }
                let d = &t[i];
                fast_clean_dir(d, &cfg_local, &*stats_local);
            }
        }));
    }
    for h in handles { let _ = h.join(); }

    // If we're going to delete Explorer's thumbnail/icon caches, stop Explorer first to unlock files
    #[cfg(windows)]
    let mut explorer_stopped = false;
    #[cfg(windows)]
    {
        if cats.thumbnails && file_targets.iter().any(|p| p.to_string_lossy().to_ascii_lowercase().contains("microsoft\\windows\\explorer")) {
            if stop_explorer(cfg) {
                explorer_stopped = true;
            }
        }
    }

    // Delete specific files (e.g., thumbnail caches)
    file_targets.retain(|p| p.is_file());
    // Extra safety: keep only files under allowed prefixes
    retain_allowed_paths(&mut file_targets, overrides.allow_system);
    file_targets.sort();
    file_targets.dedup();
    for f in file_targets.drain(..) {
        if cfg.dry_run {
            let size = fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
            if cfg.verbose && !cfg.quiet { println!("[dry-run] Would remove file: {} ({} bytes)", f.display(), size); }
            stats.add_bytes(size);
            stats.add_files(1);
            if let Some(parent) = f.parent() { stats.add_cleaned_dir(parent); }
            continue;
        }
        set_writable(&f);
        let size = fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
        if fs::remove_file(&f).is_ok() {
            if cfg.verbose && !cfg.quiet { println!("Removed file: {} ({} bytes)", f.display(), size); }
            stats.add_bytes(size);
            stats.add_files(1);
            if let Some(parent) = f.parent() { stats.add_cleaned_dir(parent); }
        } else {
            // As a fallback on Windows, schedule deletion on next reboot (locked files like Explorer caches)
            #[cfg(windows)]
            {
                if schedule_delete_on_reboot(&f) {
                    if cfg.verbose && !cfg.quiet { println!("Scheduled for deletion on reboot: {} ({} bytes)", f.display(), size); }
                    // Count bytes and files as they will be freed on reboot
                    stats.add_bytes(size);
                    stats.add_files(1);
                    if let Some(parent) = f.parent() { stats.add_cleaned_dir(parent); }
                }
            }
        }
    }

    // Restart Explorer if we stopped it
    #[cfg(windows)]
    {
        if explorer_stopped {
            start_explorer(cfg);
        }
    }

    let (files, dirs, links, bytes) = stats.snapshot();
    let mut cleaned_dirs = stats.get_cleaned_dirs();
    // Also include top-level targets that were entirely removed (best-effort):
    // If a target dir no longer exists after cleaning, add it.
    for d in targets_arc.iter() {
        if !d.exists() {
            cleaned_dirs.push(d.to_string_lossy().to_string());
        }
    }
    cleaned_dirs.sort();
    cleaned_dirs.dedup();
    let elapsed = start.elapsed();

    Summary {
        files_deleted: files,
        dirs_deleted: dirs,
        links_removed: links,
        bytes_freed: bytes,
        elapsed,
        dry_run: cfg.dry_run,
        exact_stats: cfg.exact_stats,
        cleaned_dirs,
    }
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
            if cats.crash_dumps { v.push(Path::new(&windir).join("Minidump")); }
            if cats.crash_dumps { v.push(Path::new(&windir).join("LiveKernelReports")); }
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
                            // Windows 10/11 store thumbnail and icon caches primarily as DB files under this folder.
                            // Include all common variants, not just *.db, to match Windows Settings cleanup more closely.
                            if lower.starts_with("thumbcache") || lower.starts_with("iconcache") {
                                v.push(p.clone());
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
        stats.add_cleaned_dir(dir);
        return;
    }
    // Fast path: in fast mode, attempt removal without pre-counting
    if !cfg.exact_stats {
        // Versuche das Verzeichnis selbst schreibbar zu machen, damit remove_dir_all nicht an Readonly-Attributen scheitert
        set_writable(dir);
        if fs::remove_dir_all(dir).is_ok() {
            // We do not know exact bytes/files removed in fast mode
            stats.add_dirs(1); // count the root dir removed
            stats.add_cleaned_dir(dir);
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
            stats.add_cleaned_dir(dir);
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
                    stats.add_cleaned_dir(&p);
                } else {
                    if fs::remove_dir_all(&p).is_ok() {
                        stats.add_bytes(bytes);
                        stats.add_files(files);
                        dirs += 1; // include the dir itself
                        stats.add_dirs(dirs);
                        stats.add_cleaned_dir(&p);
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

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{} {}", bytes, UNITS[unit]) } else { format!("{:.2} {}", size, UNITS[unit]) }
}

// helpers for serde defaults
pub fn env_truthy(name: &str) -> bool {
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

// ---------------- Windows-specific helpers ----------------
#[cfg(windows)]
pub fn is_elevated() -> bool {
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
pub fn is_elevated() -> bool { false }
#[cfg(windows)]
fn stop_explorer(cfg: &Config) -> bool {
    // Attempt a graceful stop of Explorer to release locks on caches
    // Use taskkill /IM explorer.exe /F
    let res = Command::new("taskkill").args(["/IM", "explorer.exe", "/F"]).status();
    match res {
        Ok(st) if st.success() => {
            if cfg.verbose && !cfg.quiet { println!("Stopped Explorer.exe to unlock caches"); }
            // Give it a moment to terminate
            std::thread::sleep(Duration::from_millis(300));
            true
        }
        _ => false,
    }
}

#[cfg(windows)]
fn start_explorer(cfg: &Config) {
    let _ = Command::new("explorer.exe").status();
    if cfg.verbose && !cfg.quiet { println!("Restarted Explorer.exe"); }
}

#[cfg(windows)]
fn schedule_delete_on_reboot(p: &Path) -> bool {
    // Use MoveFileExW(path, NULL, MOVEFILE_DELAY_UNTIL_REBOOT)
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe { MoveFileExW(wide.as_ptr(), std::ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT) != 0 }
}
