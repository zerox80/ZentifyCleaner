use std::net::SocketAddr;
use std::sync::Arc;
use std::collections::HashMap;

use axum::{
    extract::{State, Path},
    http::{StatusCode, HeaderMap},
    response::Html,
    routing::{get, post},
    Json, Router,
};
use axum::extract::DefaultBodyLimit;
use serde::{Deserialize, Serialize};
use tokio::{net::TcpListener, time::timeout, sync::Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use rand::{rngs::OsRng, RngCore};
use zentify_cleaner::{Config, load_config, run_clean, RunOverrides, format_bytes, env_truthy, is_elevated, preview_targets, TargetsPreview};
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
// No Security import needed here; use is_elevated() from library
#[cfg(windows)]
// Call ShellExecuteW via fully-qualified path at call site

#[derive(Clone)]
struct AppState {
    csrf_token: String,
    inner: Arc<InnerState>,
}

struct InnerState {
    history: Mutex<Vec<RunRecord>>,                // latest-first
    config_override: Mutex<Option<Config>>,        // in-memory override, if any
    jobs: Mutex<HashMap<String, JobStatusData>>,   // async run jobs
}

#[derive(Debug, Deserialize, Clone)]
struct RunRequest {
    dry_run: bool,
    verbose: bool,
    quiet: bool,
    exact_stats: bool,
    allow_system_clean: bool,
    prefetch: bool,
    max_parallelism: Option<u32>,
}

#[derive(Debug, Serialize, Clone)]
struct RunResponse {
    ok: bool,
    exit_code: i32,
    stdout: String,
    stderr: String,
    files_deleted: u64,
    dirs_deleted: u64,
    links_removed: u64,
    bytes_freed: u64,
    elapsed: f64,
    dry_run: bool,
    exact_stats: bool,
    cleaned_dirs: Vec<String>,
}

#[derive(Debug, Serialize)]
struct CsrfResponse {
    token: String,
}

#[derive(Debug, Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
    commit: &'static str,
    describe: &'static str,
    build_unix_time: &'static str,
    target: &'static str,
}

#[derive(Debug, Serialize)]
struct VersionInfo {
    version: &'static str,
    commit: &'static str,
    describe: &'static str,
    build_unix_time: &'static str,
    target: &'static str,
}

#[derive(Debug, Serialize)]
struct PermissionsInfo {
    elevated: bool,
    default_allow_system_clean: bool,
}

#[derive(Debug, Serialize, Clone)]
struct RunRecord {
    id: Option<String>,
    started_at: u64,
    finished_at: u64,
    response: RunResponse,
}

#[derive(Debug, Serialize, Clone)]
struct JobStatusData {
    id: String,
    status: String, // running|completed|failed|timeout
    started_at: u64,
    finished_at: Option<u64>,
    result: Option<RunResponse>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct JobIdResponse { id: String }

#[derive(Debug, Serialize)]
struct ConfigInfo {
    loaded: Config,
    effective_categories: zentify_cleaner::Categories,
    override_applied: Option<Config>,
}

#[cfg(windows)]
fn wide_null(s: &std::ffi::OsStr) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_wide().collect();
    v.push(0);
    v
}

#[cfg(windows)]
fn relaunch_as_admin() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_w = wide_null(exe.as_os_str());
    let verb = wide_null(std::ffi::OsStr::new("runas"));
    let dir_wide = match std::env::current_dir() {
        Ok(d) => wide_null(d.as_os_str()),
        Err(_) => Vec::new(),
    };
    let dir_ptr = if dir_wide.is_empty() { std::ptr::null() } else { dir_wide.as_ptr() };
    let res = unsafe { windows_sys::Win32::UI::Shell::ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), exe_w.as_ptr(), std::ptr::null(), dir_ptr, 1) } as isize;
    if res <= 32 {
        return Err(format!("ShellExecuteW failed with code {}", res));
    }
    Ok(())
}

// ---------- Helpers ----------

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

fn generate_csrf_token() -> String {
    let mut buf = [0u8; 32];
    OsRng.fill_bytes(&mut buf);
    hex_encode(&buf)
}

fn ensure_loopback(addr: &SocketAddr) -> Result<(), String> {
    if addr.ip().is_loopback() {
        return Ok(());
    }
    if env_truthy("ZENTIFY_WEB_ALLOW_NON_LOCAL") {
        return Ok(());
    }
    Err(format!(
        "Refusing to bind to non-loopback address {}. Set ZENTIFY_WEB_ALLOW_NON_LOCAL=1 to override.",
        addr
    ))
}

#[tokio::main]
async fn main() {
    // Initialize logging once; if already set by env_logger elsewhere, ignore errors.
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .is_test(false)
        .try_init();

    // Default: do NOT auto-elevate. Opt-in via ZENTIFY_WEB_AUTO_ELEVATE=1.
    #[cfg(windows)]
    {
        let do_auto = env_truthy("ZENTIFY_WEB_AUTO_ELEVATE");
        if !is_elevated() && do_auto {
            if let Err(e) = relaunch_as_admin() {
                eprintln!("zentify-web: failed to relaunch as administrator: {}", e);
            }
            return;
        }
    }

    // Security: per-process CSRF token that must be sent with state-changing requests
    let csrf_token = generate_csrf_token();
    let state = AppState {
        csrf_token,
        inner: Arc::new(InnerState {
            history: Mutex::new(Vec::new()),
            config_override: Mutex::new(None),
            jobs: Mutex::new(HashMap::new()),
        })
    };

    let app = Router::new()
        .route("/", get(ui))
        .route("/api/health", get(health))
        .route("/api/version", get(version))
        .route("/api/permissions", get(permissions))
        .route("/api/csrf", get(csrf))
        .route("/api/config", get(get_config).put(put_config).delete(delete_config))
        .route("/api/preview", post(preview_targets_handler))
        .route("/api/history", get(history))
        .route("/api/run", post(run_cleaner))
        .route("/api/run-async", post(run_cleaner_async))
        .route("/api/job/:id", get(job_status).delete(delete_job))
        .layer(DefaultBodyLimit::max(32 * 1024))
        .with_state(state);

    let bind_addr = std::env::var("ZENTIFY_WEB_BIND")
        .unwrap_or_else(|_| "127.0.0.1:7878".to_string());
    let addr: SocketAddr = bind_addr.parse().expect("Invalid ZENTIFY_WEB_BIND address");
    if let Err(e) = ensure_loopback(&addr) {
        eprintln!("zentify-web: {}", e);
        return;
    }
    // Axum 0.7 / Hyper 1.0: use TcpListener + axum::serve
    let listener = TcpListener::bind(addr).await.expect("failed to bind listener");
    if let Ok(local) = listener.local_addr() {
        println!("zentify-web: listening on http://{}", local);
    }
    axum::serve(listener, app)
        .await
        .expect("server error");
}

async fn ui() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        commit: option_env!("GIT_COMMIT").unwrap_or("unknown"),
        describe: option_env!("GIT_DESCRIBE").unwrap_or("unknown"),
        build_unix_time: option_env!("BUILD_UNIX_TIME").unwrap_or("0"),
        target: option_env!("BUILD_TARGET").unwrap_or("unknown"),
    })
}

async fn version() -> Json<VersionInfo> {
    Json(VersionInfo {
        version: env!("CARGO_PKG_VERSION"),
        commit: option_env!("GIT_COMMIT").unwrap_or("unknown"),
        describe: option_env!("GIT_DESCRIBE").unwrap_or("unknown"),
        build_unix_time: option_env!("BUILD_UNIX_TIME").unwrap_or("0"),
        target: option_env!("BUILD_TARGET").unwrap_or("unknown"),
    })
}

async fn permissions() -> Json<PermissionsInfo> {
    let elevated = is_elevated();
    let default_allow = elevated || env_truthy("ZENTIFY_ALLOW_SYSTEM_CLEAN");
    Json(PermissionsInfo { elevated, default_allow_system_clean: default_allow })
}

async fn csrf(State(state): State<AppState>) -> Json<CsrfResponse> {
    Json(CsrfResponse { token: state.csrf_token.clone() })
}

// Map beliebiger Fehler auf eine (StatusCode, String)-Antwort für Axum-Handler
fn internal_error<E: std::fmt::Display>(err: E) -> (StatusCode, String) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("internal error: {}", err),
    )
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn csrf_check(headers: &HeaderMap, state: &AppState) -> Result<(), (StatusCode, String)> {
    let hdr = headers.get("x-csrf-token").and_then(|v| v.to_str().ok());
    if hdr != Some(state.csrf_token.as_str()) {
        return Err((StatusCode::FORBIDDEN, "missing or invalid CSRF token".into()));
    }
    Ok(())
}

async fn build_cfg_and_overrides(state: &AppState, req: &RunRequest) -> (Config, RunOverrides) {
    let base_cfg = load_config();
    let override_opt = { state.inner.config_override.lock().await.clone() };
    let mut cfg: Config = override_opt.unwrap_or(base_cfg);
    if req.dry_run { cfg.dry_run = true; }
    if req.verbose { cfg.verbose = true; cfg.quiet = false; }
    if req.quiet { cfg.quiet = true; cfg.verbose = false; }
    if req.exact_stats { cfg.exact_stats = true; }

    let overrides = RunOverrides {
        allow_system: req.allow_system_clean,
        prefetch: Some(req.prefetch),
        max_parallelism: req.max_parallelism.map(|n| n as usize),
    };
    (cfg, overrides)
}

async fn push_history(state: &AppState, id: Option<String>, started_at: u64, finished_at: u64, resp: RunResponse) {
    let mut h = state.inner.history.lock().await;
    h.insert(0, RunRecord { id, started_at, finished_at, response: resp });
    const MAX_HIST: usize = 50;
    if h.len() > MAX_HIST { h.truncate(MAX_HIST); }
}

async fn run_cleaner(State(state): State<AppState>, headers: HeaderMap, Json(req): Json<RunRequest>) -> Result<Json<RunResponse>, (StatusCode, String)> {
    // CSRF validation: require the exact per-process token
    csrf_check(&headers, &state)?;

    // Build config (respect in-memory override) and overrides
    let (cfg, overrides) = build_cfg_and_overrides(&state, &req).await;

    // Run heavy sync cleaning logic on blocking thread
    let handle = tokio::task::spawn_blocking(move || { run_clean(&cfg, &overrides) });

    let timeout_secs: u64 = std::env::var("ZENTIFY_WEB_RUN_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(600);

    let started_at = now_unix();
    let summary = match timeout(Duration::from_secs(timeout_secs), handle).await {
        Ok(join_res) => join_res.map_err(internal_error)?,
        Err(_) => {
            return Err((StatusCode::REQUEST_TIMEOUT, format!("cleaner timed out after {}s", timeout_secs)));
        }
    };

    let mut stdout = String::new();
    if summary.dry_run {
        stdout.push_str(&format!(
            "Dry-run summary: would remove {} files, {} dirs, {} links; free approx {} ({} bytes) in {:?}.\n",
            summary.files_deleted,
            summary.dirs_deleted,
            summary.links_removed,
            format_bytes(summary.bytes_freed),
            summary.bytes_freed,
            summary.elapsed
        ));
    } else {
        stdout.push_str(&format!(
            "Summary: removed {} files, {} dirs, {} links; freed {} ({} bytes) in {:?}.\n",
            summary.files_deleted,
            summary.dirs_deleted,
            summary.links_removed,
            format_bytes(summary.bytes_freed),
            summary.bytes_freed,
            summary.elapsed
        ));
        if !summary.exact_stats {
            stdout.push_str("Note: Byte counts for directories are approximate (fast mode). Use --exact-stats for precise totals.\n");
        }
    }

    let resp = RunResponse { ok: true, exit_code: 0, stdout, stderr: String::new(), files_deleted: summary.files_deleted, dirs_deleted: summary.dirs_deleted, links_removed: summary.links_removed, bytes_freed: summary.bytes_freed, elapsed: summary.elapsed.as_secs_f64(), dry_run: summary.dry_run, exact_stats: summary.exact_stats, cleaned_dirs: summary.cleaned_dirs };
    let finished_at = now_unix();
    push_history(&state, None, started_at, finished_at, resp.clone()).await;
    Ok(Json(resp))
}

 

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="de">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Zentify Cleaner – Web UI</title>
  <meta http-equiv="Content-Security-Policy" content="default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'">
  <style>
    :root{--bg:#0b1220;--card:#121b2f;--text:#e6edf3;--muted:#9fb0c3;--accent:#5eb5f9;--good:#20c997;--warn:#f9c74f;--bad:#fa5252}
    *{box-sizing:border-box}
    body{margin:0;background:linear-gradient(180deg,#0b1220,#0d1526);color:var(--text);font:16px/1.5 system-ui,Segoe UI,Roboto,Ubuntu,Arial}
    .wrap{max-width:980px;margin:0 auto;padding:32px}
    header h1{margin:.2rem 0 0;font-size:1.9rem}
    header p{margin:.2rem 0 1rem;color:var(--muted)}
    .grid{display:grid;grid-template-columns:1fr;gap:16px}
    @media (min-width:960px){.grid{grid-template-columns:1fr 1fr}}
    .card{background:var(--card);border:1px solid #1e2a44;border-radius:14px;padding:16px 16px 12px;box-shadow:0 10px 30px rgba(0,0,0,.25)}
    .row{display:flex;gap:12px;align-items:center;margin:8px 0}
    .row label{flex:1}
    .actions{display:flex;gap:12px;flex-wrap:wrap;margin-top:12px}
    button{background:var(--accent);color:#041225;border:none;border-radius:10px;padding:10px 14px;font-weight:600;cursor:pointer}
    button[disabled]{opacity:.55;cursor:not-allowed}
    .btn-sec{background:#25324a;color:var(--text)}
    .log{white-space:pre-wrap;background:#0a0f1a;border-radius:10px;border:1px solid #1e2a44;padding:12px;min-height:220px}
    .badge{display:inline-block;background:#223049;padding:2px 8px;border-radius:99px;color:var(--muted);font-size:.8rem}
    .status-ok{color:var(--good)}.status-bad{color:var(--bad)}
    .hint{color:var(--muted);font-size:.9rem}
  </style>
</head>
<body>
  <div class="wrap">
    <header>
      <span class="badge">Local only</span>
      <h1>Zentify Cleaner – Web UI</h1>
      <p>Führe den Cleaner bequem aus dem Browser aus. Achtung: Aggressives Löschen von Caches/Temps. Verwende <b>Dry‑Run</b> zum Testen.</p>
    </header>

    <div class="grid">
      <section class="card">
        <h2>Optionen</h2>
        <div class="row"><label><input type="checkbox" id="dry_run" checked> Dry‑Run (Simulation, nichts wird gelöscht)</label></div>
        <div class="row"><label><input type="checkbox" id="verbose"> Verbose</label></div>
        <div class="row"><label><input type="checkbox" id="quiet"> Quiet</label></div>
        <div class="row"><label><input type="checkbox" id="exact_stats"> Exakte Statistiken (langsamer)</label></div>
        <div class="row"><label><input type="checkbox" id="allow_system_clean"> Systemweite Bereiche erlauben (Risiko!)</label></div>
        <div class="row"><label><input type="checkbox" id="prefetch"> Prefetch bereinigen</label></div>
        <div class="row">
          <label for="maxp">Max. Parallelität</label>
          <input id="maxp" type="number" min="0" step="1" placeholder="auto" style="width:120px;background:#0a0f1a;border:1px solid #1e2a44;border-radius:8px;color:var(--text);padding:6px">
        </div>
        <div class="actions">
          <button id="run">Ausführen</button>
          <button class="btn-sec" id="health">Health</button>
        </div>
        <p class="hint">Hinweis: Im Standard (Fast‑Modus) sind Byte‑Summen näherungsweise. Mit <b>Exakte Statistiken</b> werden präzise Werte ermittelt.</p>
      </section>

      <section class="card">
        <h2>Zusammenfassung</h2>
        <div id="summary"></div>
      </section>

      <section class="card">
        <h2>Ausgabe</h2>
        <div id="status" class="hint"></div>
        <pre id="log" class="log"></pre>
      </section>
    </div>
  </div>

  <script>
  const $ = sel => document.querySelector(sel);
  const log = msg => { const el = '#log'; const elRef = document.querySelector(el); elRef.textContent = msg; };
  const setStatus = (ok, code) => {
    const s = $('#status');
    if (code === undefined) s.innerHTML = '';
    else s.innerHTML = ok ? `<b class="status-ok">Erfolg</b> (Exit ${code})` : `<b class="status-bad">Fehler</b> (Exit ${code})`;
  };
  const setSummary = (data) => {
    const s = $('#summary');
    const list = (data.cleaned_dirs||[]).map(p => `<li><code>${p}</code></li>`).join('');
    s.innerHTML = `
      <p><strong>Dateien gelöscht:</strong> ${data.files_deleted}</p>
      <p><strong>Verzeichnisse gelöscht:</strong> ${data.dirs_deleted}</p>
      <p><strong>Links entfernt:</strong> ${data.links_removed}</p>
      <p><strong>Bytes freigegeben:</strong> ${data.bytes_freed}</p>
      <p><strong>Dauer:</strong> ${data.elapsed.toFixed(2)}s</p>
      <p><strong>Dry-Run:</strong> ${data.dry_run ? 'Ja' : 'Nein'}</p>
      <p><strong>Exakte Statistiken:</strong> ${data.exact_stats ? 'Ja' : 'Nein'}</p>
      <details open>
        <summary><strong>Bereinigte Pfade:</strong> (${(data.cleaned_dirs||[]).length})</summary>
        <ul style="max-height:240px;overflow:auto;padding-left:20px">${list}</ul>
      </details>
    `;
  };

  let CSRF_TOKEN = null;
  async function ensureCsrf(){
    if (CSRF_TOKEN) return;
    try {
      const res = await fetch('/api/csrf');
      const data = await res.json();
      CSRF_TOKEN = data.token;
    } catch (e) {
      console.warn('CSRF token fetch failed', e);
    }
  }

  $('#run').addEventListener('click', async () => {
    setStatus();
    $('#run').disabled = true;
    log('Starte ...');
    await ensureCsrf();
    const body = {
      dry_run: $('#dry_run').checked,
      verbose: $('#verbose').checked,
      quiet: $('#quiet').checked,
      exact_stats: $('#exact_stats').checked,
      allow_system_clean: $('#allow_system_clean').checked,
      prefetch: $('#prefetch').checked,
      max_parallelism: $('#maxp').value ? Number($('#maxp').value) : null,
    };
    try {
      const res = await fetch('/api/run', { method:'POST', headers:{'Content-Type':'application/json','X-CSRF-Token': (CSRF_TOKEN||'')}, body: JSON.stringify(body) });
      const data = await res.json();
      setStatus(data.ok, data.exit_code);
      setSummary(data);
      log((data.stdout||'') + (data.stderr ? '\n--- STDERR ---\n' + data.stderr : ''));
    } catch (e) {
      setStatus(false, -1);
      log('Request fehlgeschlagen: ' + e);
    } finally {
      $('#run').disabled = false;
    }
  });

  $('#health').addEventListener('click', async () => {
    try {
      const res = await fetch('/api/health');
      const data = await res.json();
      log('Health: ' + JSON.stringify(data, null, 2));
    } catch (e) {
      log('Health fehlgeschlagen: ' + e);
    }
  });
  </script>
</body>
</html>"#;

// ---------------- Additional API Handlers ----------------

async fn get_config(State(state): State<AppState>) -> Json<ConfigInfo> {
    let loaded = load_config();
    let effective_categories = loaded.effective_categories();
    let override_applied = state.inner.config_override.lock().await.clone();
    Json(ConfigInfo { loaded, effective_categories, override_applied })
}

async fn put_config(State(state): State<AppState>, headers: HeaderMap, Json(cfg): Json<Config>) -> Result<Json<ConfigInfo>, (StatusCode, String)> {
    csrf_check(&headers, &state)?;
    {
        let mut ov = state.inner.config_override.lock().await;
        *ov = Some(cfg.clone());
    }
    Ok(get_config(State(state)).await)
}

async fn delete_config(State(state): State<AppState>, headers: HeaderMap) -> Result<StatusCode, (StatusCode, String)> {
    csrf_check(&headers, &state)?;
    {
        let mut ov = state.inner.config_override.lock().await;
        *ov = None;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn preview_targets_handler(State(state): State<AppState>, Json(req): Json<RunRequest>) -> Result<Json<TargetsPreview>, (StatusCode, String)> {
    // No CSRF required for read-only preview; could be tightened if desired
    let (cfg, overrides) = build_cfg_and_overrides(&state, &req).await;
    let preview = preview_targets(&cfg, &overrides);
    Ok(Json(preview))
}

async fn history(State(state): State<AppState>) -> Json<Vec<RunRecord>> {
    let h = state.inner.history.lock().await;
    Json(h.clone())
}

async fn run_cleaner_async(State(state): State<AppState>, headers: HeaderMap, Json(req): Json<RunRequest>) -> Result<Json<JobIdResponse>, (StatusCode, String)> {
    csrf_check(&headers, &state)?;
    let id = {
        let mut b = [0u8; 8];
        OsRng.fill_bytes(&mut b);
        hex_encode(&b)
    };
    let (cfg, overrides) = build_cfg_and_overrides(&state, &req).await;
    let started_at = now_unix();
    {
        let mut jobs = state.inner.jobs.lock().await;
        jobs.insert(id.clone(), JobStatusData { id: id.clone(), status: "running".into(), started_at, finished_at: None, result: None, error: None });
    }
    let state2 = state.clone();
    let id_for_task = id.clone();
    tokio::spawn(async move {
        let handle = tokio::task::spawn_blocking(move || { run_clean(&cfg, &overrides) });
        let timeout_secs: u64 = std::env::var("ZENTIFY_WEB_RUN_TIMEOUT_SECS").ok().and_then(|s| s.parse::<u64>().ok()).unwrap_or(600);
        let finished_at;
        let mut status = "completed".to_string();
        let mut error: Option<String> = None;
        let result: Option<RunResponse>;
        let started_at_local = started_at;
        match timeout(Duration::from_secs(timeout_secs), handle).await {
            Ok(join_res) => match join_res {
                Ok(summary) => {
                    let mut stdout = String::new();
                    if summary.dry_run {
                        stdout.push_str(&format!(
                            "Dry-run summary: would remove {} files, {} dirs, {} links; free approx {} ({} bytes) in {:?}.\n",
                            summary.files_deleted, summary.dirs_deleted, summary.links_removed, format_bytes(summary.bytes_freed), summary.bytes_freed, summary.elapsed
                        ));
                    } else {
                        stdout.push_str(&format!(
                            "Summary: removed {} files, {} dirs, {} links; freed {} ({} bytes) in {:?}.\n",
                            summary.files_deleted, summary.dirs_deleted, summary.links_removed, format_bytes(summary.bytes_freed), summary.bytes_freed, summary.elapsed
                        ));
                        if !summary.exact_stats { stdout.push_str("Note: Byte counts for directories are approximate (fast mode). Use --exact-stats for precise totals.\n"); }
                    }
                    let resp = RunResponse { ok: true, exit_code: 0, stdout, stderr: String::new(), files_deleted: summary.files_deleted, dirs_deleted: summary.dirs_deleted, links_removed: summary.links_removed, bytes_freed: summary.bytes_freed, elapsed: summary.elapsed.as_secs_f64(), dry_run: summary.dry_run, exact_stats: summary.exact_stats, cleaned_dirs: summary.cleaned_dirs };
                    finished_at = now_unix();
                    result = Some(resp.clone());
                    push_history(&state2, Some(id_for_task.clone()), started_at_local, finished_at, resp).await;
                }
                Err(e) => {
                    finished_at = now_unix();
                    status = "failed".into();
                    error = Some(format!("internal error: {}", e));
                    result = None;
                }
            },
            Err(_) => {
                finished_at = now_unix();
                status = "timeout".into();
                error = Some(format!("cleaner timed out after {}s", timeout_secs));
                result = None;
            }
        }
        let mut jobs = state2.inner.jobs.lock().await;
        if let Some(entry) = jobs.get_mut(&id_for_task) {
            entry.status = status;
            entry.finished_at = Some(finished_at);
            entry.result = result;
            entry.error = error;
        }
    });

    Ok(Json(JobIdResponse { id }))
}

async fn job_status(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<JobStatusData>, (StatusCode, String)> {
    let jobs = state.inner.jobs.lock().await;
    if let Some(s) = jobs.get(&id) {
        return Ok(Json(s.clone()));
    }
    Err((StatusCode::NOT_FOUND, "job not found".into()))
}

async fn delete_job(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> Result<StatusCode, (StatusCode, String)> {
    csrf_check(&headers, &state)?;
    let mut jobs = state.inner.jobs.lock().await;
    if jobs.remove(&id).is_some() {
        return Ok(StatusCode::NO_CONTENT);
    }
    Err((StatusCode::NOT_FOUND, "job not found".into()))
}
