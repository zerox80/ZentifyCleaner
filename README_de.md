# Zentify Cleaner

Minimaler, schneller Windows‑Cleaner für temporäre Dateien – mit CLI und optionalem lokalen Web‑UI. Entwickelt in Rust für hohe Performance und Sicherheit.

- Unterstützt Windows 10/11
- Vorsichtige Standard‑Sicherheitsmechanismen (nur sichere Präfixe, keine Root‑Laufwerke, Schutz sensibler Verzeichnisse)
- Dry‑Run (Trockendurchlauf), ausführliches Logging, wahlweise exakte Byte‑Statistiken
- Optionales Web‑UI (`zentify-web`) mit CSRF‑Schutz
- Saubere Windows‑Installations-/Deinstallationsskripte inkl. Startmenü‑Shortcuts


## Inhalt

- Funktionen
- Sicherheitsmodell
- Installation
- Schnellstart
- CLI‑Nutzung
- Web‑UI
- Konfiguration
- Umgebungsvariablen
- Aus dem Quellcode bauen
- Fehlerbehebung
- Projektstruktur
- Lizenz


## Funktionen

- Bereinigung verbreiteter Benutzer‑ und System‑Caches: `Temp`, `INetCache`, `WebCache`, `WER` (Fehlerberichte), DirectX/NVIDIA Shader‑Caches, Teams‑Caches, Office‑Cache, UWP `LocalCache`/`TempState`, Java/Adobe/WMP‑Caches u. v. m.
- Browser‑Caches: Chromium‑Familie (Chrome/Edge/Brave/Vivaldi/Opera), Firefox
- Optional systemweite Ziele (Windows `Temp`, `Prefetch`, Windows Update `Download`, Delivery Optimization‑Cache, Defender‑Historie, Crash‑Dumps, ASP.NET Temp)
- Dry‑Run‑Vorschau und exakter Statistik‑Modus
- Parallelisierung für schnelle Bereinigung

Die vollständigen Kategorien und Pfade findest du in `src/lib.rs`.


## Sicherheitsmodell

- Arbeiten nur unter konservativen, erlaubten Präfixen (z. B. `%TEMP%`, `%LOCALAPPDATA%`, `%APPDATA%`; plus `%WINDIR%`, `%SystemRoot%`, `%ProgramData%` wenn systemweite Bereinigung erlaubt ist)
- Keine Operationen auf Laufwerkswurzeln (z. B. `C:\`)
- Traversieren von Reparse Points (Junctions/Symlinks) wird vermieden; stattdessen wird der Link selbst entfernt
- Schutz sensibler Top‑Level‑Verzeichnisse (z. B. `Windows`, `Program Files`, `ProgramData`, `C:\Users`‑Root)
- Systemweite Bereinigung ist nur mit erhöhten Rechten oder expliziter Freigabe via Umgebungsvariablen aktiv (siehe unten)


## Installation

Du kannst die Windows‑Skripte verwenden oder aus dem Quellcode bauen.

- PowerShell (empfohlen):
  ```powershell
  .\scripts\install.ps1
  ```
- CMD‑Wrapper:
  ```bat
  .\scripts\install.cmd
  ```

Was der Installer macht:
- Baut Release‑Binärdateien mit dem Feature `web`
- Installiert nach `C:\Program Files\Zentify Cleaner`
- Fügt den Installationspfad zur PATH‑Systemvariablen (Machine) hinzu
- Erstellt Startmenü‑Shortcuts: CLI, Web‑UI‑Launcher und Uninstall
- Registriert einen Uninstaller in „Apps & Features“

Deinstallation:
```bat
.\scripts\uninstall.cmd
```


## Schnellstart

- Dry‑Run (nichts wird gelöscht):
  ```bat
  zentify-cleaner --dry-run
  ```

- Tatsächliche Bereinigung mit Standardkategorien:
  ```bat
  zentify-cleaner
  ```

- Web‑UI (standardmäßig nur lokal):
  ```bat
  zentify-web
  ```
  Danach im Browser öffnen: http://127.0.0.1:7878


## CLI‑Nutzung

`zentify-cleaner` unterstützt folgende Flags:

```
--dry-run         Nichts löschen, nur anzeigen, was gelöscht würde
--verbose         Ausführliche Ausgabe (überschreibt quiet)
--quiet           Die meiste Ausgabe unterdrücken
--exact-stats     Exakte Byte‑Summen ermitteln (langsamer)
```

Hinweise zum Verhalten:
- Systemweite Bereinigung wird automatisch aktiviert, wenn der Prozess erhöht läuft, oder per `ZENTIFY_ALLOW_SYSTEM_CLEAN=1`. Mit `ZENTIFY_FORCE_NO_SYSTEM_CLEAN=1` lässt sie sich erzwingen deaktivieren.
- Prefetch‑Bereinigung ist standardmäßig aus; aktiviere sie via `ZENTIFY_PREFETCH=1` oder im Web‑UI.
- Parallelität ist standardmäßig bis zu 8 Threads; überschreibe per `ZENTIFY_MAX_PARALLELISM=N`.
- Im schnellen Modus (Standard) sind Verzeichnis‑Byte‑Summen näherungsweise. Mit `--exact-stats` erhältst du präzise Werte.

Ausgabe:
- Im Dry‑Run siehst du, was entfernt würde, und eine (ggf. angenäherte) Gesamtsumme.
- In echten Läufen erscheint eine Zusammenfassung; ohne exakte Statistik wird ein Hinweis zu „approximate bytes“ ausgegeben.


## Web‑UI

`zentify-web` stellt eine schlanke lokale Oberfläche (Axum/Hyper) bereit.

- Standard‑Bind‑Adresse: `127.0.0.1:7878` (änderbar via `ZENTIFY_WEB_BIND=IP:PORT`)
- Nicht‑Loopback‑Bind wird verweigert, außer `ZENTIFY_WEB_ALLOW_NON_LOCAL=1` ist gesetzt (mit Vorsicht verwenden)
- Optionale Auto‑Elevation unter Windows via `ZENTIFY_WEB_AUTO_ELEVATE=1` (Neustart mit UAC‑Prompt)
- CSRF‑Schutz: Zustandsändernde Requests benötigen ein Token von `/api/csrf`

Wichtige Endpunkte (vom integrierten UI genutzt):
- `GET /` – UI
- `GET /api/health` – Health‑Informationen
- `GET /api/version` – Versionsmetadaten
- `GET /api/permissions` – Rechte & Standardverhalten
- `GET /api/csrf` – CSRF‑Token
- `GET/PUT/DELETE /api/config` – Konfiguration laden/überschreiben/zurücksetzen
- `POST /api/preview` – Zielvorschau (Directories/Files)
- `GET /api/history` – letzte Läufe
- `POST /api/run` – synchroner Lauf
- `POST /api/run-async` + `GET/DELETE /api/job/:id` – asynchrone Jobs


## Konfiguration

Zentify Cleaner kann optional eine JSON‑Konfiguration laden. Suchreihenfolge:
1. `./.zentify/config.json` (aktuelles Verzeichnis)
2. `%ProgramData%/Zentify/config.json`
3. `%APPDATA%/Zentify/config.json`

Beispiel:
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

Das Web‑UI unterstützt zudem In‑Memory‑Overrides via `/api/config` und die UI‑Schalter.


## Umgebungsvariablen

Cleaner (CLI & Library):
- `ZENTIFY_ALLOW_SYSTEM_CLEAN=1` – systemweite Ziele erlauben, auch ohne Erhöhung
- `ZENTIFY_FORCE_NO_SYSTEM_CLEAN=1` – systemweite Bereinigung erzwingen deaktivieren
- `ZENTIFY_PREFETCH=1` – Windows Prefetch bereinigen
- `ZENTIFY_MAX_PARALLELISM=N` – Worker‑Threads begrenzen

Web‑UI:
- `ZENTIFY_WEB_BIND=127.0.0.1:7878` – Bind‑Adresse
- `ZENTIFY_WEB_ALLOW_NON_LOCAL=1` – Nicht‑Loopback zulassen (vorsichtig einsetzen)
- `ZENTIFY_WEB_AUTO_ELEVATE=1` – Versuch, beim Start mit Adminrechten neu zu starten (Windows)
- `ZENTIFY_WEB_RUN_TIMEOUT_SECS=600` – Server‑Timeout für Läufe


## Aus dem Quellcode bauen

Voraussetzungen:
- Rust 1.70+ (Windows 10/11)

Alle Binaries (inkl. Web‑UI) bauen:
```powershell
cargo build --release --locked --features web --bins
```
Die Binaries liegen anschließend unter `target\\release\\`:
- `zentify-cleaner.exe`
- `zentify-web.exe`


## Fehlerbehebung

- Einige Dateien lassen sich nicht entfernen:
  - Das ist bei gesperrten Dateien normal. Für bestimmte Dateien (z. B. Explorer‑Caches) kann eine Löschung beim Neustart geplant werden.
- Keine/zu wenige Ausgaben:
  - Prüfe, dass `--quiet` nicht gesetzt ist. Für mehr Details `--verbose` nutzen.
- Web‑UI nicht erreichbar:
  - Prüfe die Ausgabe der Bind‑Adresse und verwende identischen Host/Port. Firewall‑Regeln prüfen.
  - Standardmäßig sind Nicht‑Loopback‑Binds untersagt. `ZENTIFY_WEB_ALLOW_NON_LOCAL=1` erlaubt sie (bewusst einsetzen).
- Systemweite Bereinigung fand nicht statt:
  - Erhöht ausführen oder `ZENTIFY_ALLOW_SYSTEM_CLEAN=1` setzen. Mit `ZENTIFY_FORCE_NO_SYSTEM_CLEAN=1` lässt sie sich deaktivieren.


## Projektstruktur

- `src/main.rs` – CLI‑Einstieg (Binary: `zentify-cleaner`)
- `src/lib.rs` – Kernlogik & öffentliche API
- `src/bin/zentify-web.rs` – Web‑UI‑Server (Binary: `zentify-web`)
- `scripts/` – Windows‑Install/Uninstall‑Helfer
- `build.rs` – Build‑Metadaten‑Hooks


## Lizenz

MIT. Siehe `LICENSE`.
