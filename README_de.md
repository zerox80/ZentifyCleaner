# Zentify Cleaner

[![CI](https://github.com/zerox80/ZentifyCleaner/actions/workflows/ci.yml/badge.svg)](https://github.com/zerox80/ZentifyCleaner/actions/workflows/ci.yml)

Minimaler, extrem schneller Windows-Cleaner (CLI), der temporäre Dateien und Caches aggressiv entfernt. Entwickelt für Windows 10/11.

> Achtung: Dieses Tool löscht Cache-/Temp-Verzeichnisse bewusst aggressiv, ohne Rückfrage. Bitte lies die Sicherheitshinweise unten.

---

## Inhalt

- [Features](#features)
- [Unterstützte Ziele (Beispiele)](#unterstützte-ziele-beispiele)
- [Sicherheitshinweise](#sicherheitshinweise)
- [Design & Sicherheitsarchitektur](#design--sicherheitsarchitektur)
- [Leistung](#leistung)
- [Systemvoraussetzungen](#systemvoraussetzungen)
- [Installation](#installation)
- [Verwendung](#verwendung)
- [Konfiguration](#konfiguration)
- [Umgebungsvariablen](#umgebungsvariablen)
- [FAQ](#faq)
- [Fehlerbehebung](#fehlerbehebung)
- [Bekannte Einschränkungen](#bekannte-einschränkungen)
- [Deinstallation](#deinstallation)
- [Build aus dem Quellcode](#build-aus-dem-quellcode)
- [CI/CD](#cicd)
- [Roadmap](#roadmap)
- [Beitragen](#beitragen)
- [Support](#support)
- [Lizenz](#lizenz)

---

## Features

- Aggressive Bereinigung typischer Windows-Temp- und Cache-Pfade
- Unterstützung gängiger Browser-Caches (Chromium-basiert und Firefox)
- Aufräumen von Windows Update-Downloads, Delivery-Optimization-Caches, WER-Report-Queues/Archive, Minidumps u. a.
- Entfernen von Explorer-Thumbnail- und Icon-Caches (`thumbcache*.db`, `iconcache*.db`)
- Deduplizierte Zielpfade und robuster Löschvorgang (vollständiges Entfernen mit Fallback auf flaches Aufräumen)
- Konfigurierbar via `.zentify/config.json` (Dry-Run, Verbose/Quiet, Kategorien)
- Sicherere Defaults: Systemweite Pfade (z. B. `%WINDIR%`) werden nur nach Opt‑in (`ZENTIFY_ALLOW_SYSTEM_CLEAN=1`) ODER bei Start mit Administratorrechten bereinigt.
- Bei System‑Clean wird zusätzlich das `Prefetch`‑Cleaning aktiviert.
- Null Abhängigkeiten zur Laufzeit (reines Rust-CLI), sehr schnell startend
- Zwei Modi für Statistiken: schneller „Fast‑Modus“ (Byte‑Zähler näherungsweise) oder exakte Zählung per `--exact-stats`

## Unterstützte Ziele (Beispiele)

Auszug der aktuell angepeilten Pfade/Pattern (abhängig von vorhandenen Verzeichnissen und Profilen):

- Benutzer-/System-Temp: `%TEMP%`, `%LOCALAPPDATA%/Temp`, `%WINDIR%/Temp`, `%SystemRoot%/Temp`
- Windows-spezifisch: 
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
  - Live Kernel Reports (groß; nur bei System‑Clean): `%WINDIR%/LiveKernelReports`
  - Speicherabbild (sehr groß; nur bei System‑Clean): `%WINDIR%/MEMORY.DMP`
  - Defender Verlauf/History (optional): `%ProgramData%/Microsoft/Windows Defender/Scans/History`
  - ASP.NET Temporary Files (optional): `%WINDIR%/Microsoft.NET/Framework{,64}/v4.0.30319/Temporary ASP.NET Files`
- Browser-Caches:
  - Chromium-basiert (z. B. Chrome, Edge, Brave, Vivaldi, Opera GX): pro Profil u. a. `Cache`, `Code Cache`, `GPUCache`, `ShaderCache`, `DawnCache`, `GrShaderCache`, `Media Cache`, `Service Worker/CacheStorage`, `Application Cache`, `Network/Cache`
  - Chromium (produktweit, nicht profilbezogen): `User Data/ShaderCache/GPUCache` (Chrome/Edge/Brave/Vivaldi/Opera)
  - Opera (Stable): analog zu Opera GX unter `%LOCALAPPDATA%/Opera Software/Opera Stable/...`
  - Firefox: `cache2`, `startupCache` je Profil unter `%LOCALAPPDATA%/Mozilla/Firefox/Profiles`
- Explorer Thumbnail-/Icon-Caches (Datei-Löschung): `thumbcache*.db`, `iconcache*.db` in `%LOCALAPPDATA%/Microsoft/Windows/Explorer`

- Anwendungen/Services (optional):
  - Microsoft Teams (Classic): `%APPDATA%/Microsoft/Teams/{Cache,GPUCache,Service Worker/CacheStorage,IndexedDB,Local Storage}`
  - Microsoft Teams (New/UWP): `%LOCALAPPDATA%/Packages/MSTeams_8wekyb3d8bbwe/LocalCache`
  - Office Document Cache: `%LOCALAPPDATA%/Microsoft/Office/16.0/OfficeFileCache`
  - UWP/Store-Apps (pro Paket): `%LOCALAPPDATA%/Packages/*/{LocalCache,TempState}` (keine `LocalState`/`RoamingState`)
  - NVIDIA Shader Caches: `%LOCALAPPDATA%/NVIDIA/{GLCache,DXCache}`
  - Java Cache: `%LOCALAPPDATA%/Sun/Java/Deployment/cache`
  - Adobe Media Caches: `%LOCALAPPDATA%/Adobe/Common/{Media Cache,Media Cache Files}`
  - Windows Media Player Cache: `%LOCALAPPDATA%/Microsoft/Media Player/Cache`

Hinweis: Es wird nur bereinigt, was tatsächlich vorhanden ist. Nicht vorhandene Pfade werden übersprungen.

### Quellen (Auswahl)

- Windows Update/Delivery Optimization/WER: Microsoft Q&A, „SoftwareDistribution/Download löschen ist sicher“ – https://learn.microsoft.com/en-us/answers/questions/4204298/c-windowssoftwaredistributiondownload-deleting
- WER Settings (Default CrashDumps-Pfad) – https://learn.microsoft.com/en-us/windows/win32/wer/wer-settings
- WER ReportArchive/ReportQueue Cleanup – https://woshub.com/wer-windows-error-reporting-clear-reportqueue-folder-windows/
- Crash Dumps entfernen (wenn nicht benötigt) – https://learn.microsoft.com/en-us/answers/questions/217604/can-i-delete-dmp-files-from-a-windows-server-manua
- Chromium Service Worker CacheStorage (Edge/Chrome) – https://superuser.com/questions/1608022/how-to-clear-chrome-chromium-edge-service-worker-cache
- Vivaldi/Chromium Cache-Locations – https://forum.vivaldi.net/topic/91432/cache-locations
- Chrome Shader/GPU Cache – https://artifacts-kb.readthedocs.io/en/latest/sources/webbrowser/ChromeCache.html
- Mozilla Firefox cache2 Pfad – https://support.mozilla.org/en-US/questions/1275991
- Microsoft Teams Cache (offiziell) – https://learn.microsoft.com/en-us/troubleshoot/microsoftteams/teams-administration/clear-teams-cache
- New Teams (UWP) Cache – https://learn.microsoft.com/en-us/answers/questions/4418666/clear-new-teams-cache
- Office Document Cache – https://support.microsoft.com/en-us/office/delete-your-office-document-cache-b1d3765e-d71b-4bb8-99ca-acd22c42995d

## Sicherheitshinweise

- Aggressive Löschung: Cache-Ordner und temporäre Verzeichnisse werden, wenn möglich, komplett entfernt und teilweise neu angelegt. Das kann zu größeren einmaligen Rebuilds von Caches führen (z. B. Browser starten ggf. langsamer beim ersten Start nach der Bereinigung).
- Schließe Browser & Programme: Bitte vor dem Start alle Browser und Anwendungen schließen, um Dateisperren und unvollständige Löschvorgänge zu vermeiden.
- Keine Rückfragen: Das Tool hat keine Interaktivität und keine Bestätigungsdialoge. Ein Simulationsmodus ist über die Konfiguration (`dry_run`) verfügbar.
- Auf eigenes Risiko: Verwende das Tool nur, wenn du die Implikationen verstehst. Wichtige Nutzerdaten (Dokumente, Bilder etc.) werden nicht gezielt angetastet, dennoch gilt: Verwendung auf eigenes Risiko.
- Systemweite Pfade (z. B. `%WINDIR%/Temp`, `%ProgramData%/...`) werden nur gelöscht, wenn die Umgebungsvariable `ZENTIFY_ALLOW_SYSTEM_CLEAN=1` gesetzt ist ODER das Programm als Administrator läuft.
- Zusätzliche Schutzschicht (Allowlist): Das CLI bereinigt nur Pfade, die unter konservativ definierten Basispfaden liegen (`%LOCALAPPDATA%`, `%APPDATA%`, temporäre Verzeichnisse und – nur mit System‑Clean – `%WINDIR%`, `%SystemRoot%`, `%ProgramData%`). Unerwartete Pfade außerhalb dieser Bereiche werden verworfen.

## Design & Sicherheitsarchitektur

- Minimalistische, schnelle CLI: Keine Laufzeit-Abhängigkeiten; implementiert in Rust für geringe Startzeit.
- Konservative Schutzmechanismen:
  - Keine Operationen auf Wurzelverzeichnissen (z. B. `C:\`).
  - Erkennung und Nicht-Durchschreiten von Reparse-Points/Junctions/Symlinks (`is_reparse_point`), stattdessen Entfernung des Links selbst (best-effort).
  - Schutz vor sensiblen Top-Level-Systempfaden (`is_sensitive_dir`).
- Ausführungsmodi:
  - Benutzer-Clean: Standard, ohne Adminrechte, rein user-spezifische Caches/Temps.
  - System-Clean: Aktiviert bei Adminrechten oder `ZENTIFY_ALLOW_SYSTEM_CLEAN=1`, erweitert um Systempfade und erzwingt zusätzlich `Prefetch`-Cleanup.
- Parallelität & Performance:
  - Parallelisierte Verarbeitung von Zielverzeichnissen mit Obergrenze 8 Threads; anpassbar via `ZENTIFY_MAX_PARALLELISM`.
  - Vorab-Metriken pro Verzeichnis für präzise „freigegeben“-Summen.
- Transparenz:
  - Zusammenfassende Statistiken am Ende (Dateien/Verzeichnisse/Links/Bytes, Laufzeit).
  - `dry_run`-Unterstützung via Konfigurationsdatei zum sicheren Testen.

## Leistung

- Startzeit: Sehr kurz, da leichtgewichtiges Rust-CLI ohne zusätzliche Runtime-Abhängigkeiten.
- I/O-Muster: „Aggressive“ Löschung versucht zuerst `remove_dir_all`, fällt ansonsten auf flaches Aufräumen zurück.
- Parallelität: Standardmäßig bis zu `min(Kerne, 8)` Threads, über `ZENTIFY_MAX_PARALLELISM` regulierbar.
- Bekannte Einflussfaktoren: Gesperrte Dateien (z. B. laufende Browser/Programme) und sehr tiefe Verzeichnisbäume können die Laufzeit erhöhen.
 - Statistik‑Modi: 
   - Fast‑Modus (Standard): schnell, Byte‑Summen für komplett entfernte Verzeichnisse werden näherungsweise/teilweise ermittelt. In der Zusammenfassung erscheint ein Hinweis.
   - Exakt: `--exact-stats` bzw. `"exact_stats": true` in der Konfiguration ermittelt präzise Byte‑Summen (langsamer, da zusätzlicher Scan nötig ist).

## Systemvoraussetzungen

- Windows 10/11 (64‑bit)
- Für Installation via Script: PowerShell 5.1 oder neuer (Standard unter Windows 10/11)
- Für Build aus Source: Rust-Toolchain (MSRV in CI: 1.70)
- Administratorrechte werden für Installation/Deinstallation und vollständige Bereinigung empfohlen

## Installation

### A) Über Release-Paket (empfohlen)

1. Lade das neueste ZIP von den [Releases](https://github.com/zentify/zentify-cleaner/releases) herunter.
2. Entpacke es in einen Ordner deiner Wahl (z. B. `C:\Program Files\Zentify Cleaner`).
3. Optional: Füge den Ordner deinem System-`PATH` hinzu, um `zentify-cleaner` überall starten zu können.
4. Starte die GUI über `Zentify Web UI.cmd` (UAC‑Prompt erscheint automatisch) oder führe das CLI direkt aus (`zentify-cleaner.exe`).

Das ZIP enthält: `zentify-cleaner.exe`, `zentify-web.exe`, `Zentify Web UI.cmd`, `README.md` und ggf. `Icon.png`.

### B) Über die Installationsskripte (selbstbauend)

Das PowerShell-Skript baut das CLI und die Web‑GUI (Release) und installiert beide nach `C:\Program Files\Zentify Cleaner`. Außerdem wird der Ordner dem System‑`PATH` hinzugefügt, eine Startmenü‑Verknüpfung für das CLI (`Zentify Cleaner`) und für die GUI (`Zentify Cleaner Web UI`) erstellt und optional Desktop‑Verknüpfungen angelegt.

PowerShell (empfohlen):

```powershell
# Aus dem Projektstammverzeichnis
pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1

# Optional: ohne Build (falls bereits gebaut)
pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1 -NoBuild

# Optional: zusätzlich Desktop‑Verknüpfungen anlegen
pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1 -DesktopShortcut
```

CMD:

```bat
REM Aus dem Projektstammverzeichnis
scripts\install.cmd
REM Optional mit Parametern (werden durchgereicht):
scripts\install.cmd -DesktopShortcut
```

Das Skript hebt sich bei Bedarf automatisch auf Administratorrechte an.

### C) Portable Nutzung (ohne Installation)

Wenn du keine Installation vornehmen möchtest:

1. Entpacke das Release‑ZIP in einen beliebigen Ordner.
2. Doppelklicke `Zentify Web UI.cmd`, um die lokale Web‑Oberfläche mit Adminrechten zu starten und den Browser zu öffnen.
3. Alternativ kannst du das CLI direkt über `zentify-cleaner.exe` (ohne Admin) in einer Konsole starten.

## Verwendung

```powershell
# In einem neuen Terminal (User oder Admin):
zentify-cleaner [--dry-run] [--verbose] [--quiet] [--exact-stats]
```

- Flags/Argumente:
  - `--dry-run`: Löscht nichts, zeigt nur an, was gelöscht würde.
  - `--verbose`: Ausführlichere Ausgabe (setzt `quiet` außer Kraft).
  - `--quiet`: Unterdrückt die meiste Ausgabe.
  - `--exact-stats`: Exakte Byte‑Summen ermitteln (langsamer). Ohne dieses Flag wird zur besseren Performance auf eine schnelle, näherungsweise Byte‑Ermittlung gesetzt.
- Priorität der Konfiguration: CLI-Flag > Umgebungsvariablen > `.zentify/config.json`.
- Systemweite Bereinigung, wenn die Env-Variable `ZENTIFY_ALLOW_SYSTEM_CLEAN=1` gesetzt ist ODER wenn das Programm mit Administratorrechten läuft.
- Bei System‑Clean wird `Prefetch` zusätzlich bereinigt.
- Beim Start über den Windows Explorer (z. B. Rechtsklick → „Als Administrator ausführen“) bleibt das Konsolenfenster am Ende offen, bis du Enter drückst.
- Ausgabe: Zusammenfassung (Anzahl Dateien/Verzeichnisse/Links, freigegebene Bytes, Laufzeit) und „Aggressive cleaning complete.“

Weitere Beispiele:

```powershell
# Systemweiten Clean (inkl. Prefetch) als Nicht-Admin erzwingen:
$env:ZENTIFY_ALLOW_SYSTEM_CLEAN = '1'
zentify-cleaner

# Prefetch explizit aktivieren, ohne Adminrechte:
$env:ZENTIFY_PREFETCH = '1'
zentify-cleaner --verbose

# Parallelität auf 4 Threads begrenzen (nützlich bei I/O-Druck):
$env:ZENTIFY_MAX_PARALLELISM = '4'
zentify-cleaner --quiet
```

Dry-Run über Konfigurationsdatei:

```jsonc
// .zentify/config.json (im Arbeitsverzeichnis oder unter %APPDATA%/Zentify bzw. %PROGRAMDATA%/Zentify)
{
  "dry_run": true,
  "verbose": true,
  "quiet": false
}
```

Danach einfach `zentify-cleaner` starten.

## Web UI (Rust/Axum)

Eine schlanke, lokale Weboberfläche ist enthalten (optional, Feature-Flag `web`). Sie basiert auf Rust (Axum) und steuert das CLI im Hintergrund.

Starten:

```powershell
# Entwicklung
cargo run --bin zentify-web --features web --release

# oder nach Build
cargo build --bin zentify-web --features web --release --locked
./target/release/zentify-web.exe  # Windows
```

Standardmäßig lauscht die UI auf `http://127.0.0.1:7878/`. Bindings auf Nicht‑Loopback‑Adressen werden standardmäßig abgelehnt.

### Start der GUI (Windows, nach Installation)

- Über das Startmenü: `Zentify Cleaner Web UI` (öffnet Server und Browser).
- Alternativ im Installationsordner: `C:\Program Files\Zentify Cleaner\Zentify Web UI.cmd` doppelklicken.
- Optional wurde eine Desktop‑Verknüpfung angelegt, wenn `-DesktopShortcut` verwendet wurde.

Hinweis: Das Binärziel `zentify-web` wird nur gebaut, wenn das Feature `web` aktiviert ist (z. B. `--features web`). Das CLI `zentify-cleaner` ist davon unabhängig.

Optionale Umgebungsvariablen:

```powershell
$env:ZENTIFY_WEB_BIND = '127.0.0.1:8080'     # Bind-Adresse ändern (Loopback empfohlen)
$env:ZENTIFY_CLEANER_PATH = 'C:\\Program Files\\Zentify Cleaner\\zentify-cleaner.exe' # CLI-Pfad überschreiben
$env:ZENTIFY_WEB_ALLOW_NON_LOCAL = '1'       # Nicht-Loopback-Bind explizit erlauben (nicht empfohlen)
$env:ZENTIFY_WEB_ALLOW_PATH_FALLBACK = '1'   # Fallback auf PATH für zentify-cleaner erlauben (sonst nur Ko-Lokation/Override)
```

Endpunkte:

- `/` – Weboberfläche
- `/api/health` – Gesundheitsprüfung
- `/api/run` (POST, JSON) – startet den Cleaner mit Flags/Env (erfordert `X-CSRF-Token`-Header, siehe unten)

Beispiel-Request an `/api/run`:

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

Sicherheitsmaßnahmen der Web UI:

- CSRF‑Schutz: Vor jedem POST an `/api/run` muss ein gültiger CSRF‑Token im Header `X-CSRF-Token` gesendet werden. Den Token erhältst du von `/api/csrf`.
- Loopback‑Enforcement: Das Binding auf Nicht‑Loopback‑Adressen wird verweigert, außer `ZENTIFY_WEB_ALLOW_NON_LOCAL=1` ist gesetzt.
- Pfadauflösung des CLI: Standardmäßig wird NICHT auf den `PATH` zurückgegriffen. Das Web‑Frontend nutzt entweder einen explizit gesetzten Pfad (`ZENTIFY_CLEANER_PATH`) oder ein neben `zentify-web` befindliches Binary. Der Fallback auf `PATH` ist nur mit `ZENTIFY_WEB_ALLOW_PATH_FALLBACK=1` möglich.

Sicherheits-Hinweis: Die Web UI ist für die lokale Nutzung gedacht und bindet per Default nur an `127.0.0.1`. Exponiere die UI nicht ins Netzwerk/Internet.

## Konfiguration

Der Cleaner liest optional eine JSON-Konfiguration aus folgenden Orten (erste gefundene gewinnt):

1. Projekt/Arbeitsverzeichnis: `.zentify/config.json`
2. `%PROGRAMDATA%/Zentify/config.json`
3. `%APPDATA%/Zentify/config.json`

Minimalbeispiel:

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

Erläuterungen:

- `dry_run`: Löscht nichts, zeigt nur an, was gelöscht würde.
- `verbose`/`quiet`: Steuerung der Ausgabedetails.
- `categories`: Feingranulare Aktivierung/Deaktivierung einzelner Reinigungsbereiche.
- Systemweite Bereiche werden berücksichtigt, wenn `ZENTIFY_ALLOW_SYSTEM_CLEAN=1` gesetzt ist ODER das Programm als Administrator läuft.
- Unbekannte Schlüssel in der JSON werden ignoriert (zukunftssicher).
- Hinweis: Bei System‑Clean wird `prefetch` unabhängig von der Konfiguration auf `true` gesetzt.

### Erweitertes Beispiel (reserviert/experimentell)

Das Projekt verfolgt eine „zukunftssichere“ Konfigurationsdatei. Einige Felder sind bereits in Beispielen enthalten, werden aber vom aktuellen Binary noch nicht ausgewertet – sie werden ignoriert. Du kannst sie gefahrlos belassen; sie dienen der Roadmap und zukünftigen Funktionen.

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

Hinweis: Die fett markierten reservierten Felder oben (u. a. `interactive`, `min_age_days`, `logs`, `windows_old_cleanup`, `system_cleanup_commands`, `include_patterns`, `excluded_patterns`, `custom_directories`, `browser_profiles`) sind gegenwärtig Platzhalter und werden vom aktuellen Programmstand ignoriert.

## Umgebungsvariablen

- `ZENTIFY_ALLOW_SYSTEM_CLEAN`: Setze auf `1`/`true`/`yes`/`on`, um systemweite Bereiche ohne Administratorrechte zu bereinigen. Wenn du das Programm als Administrator startest, ist diese Variable nicht erforderlich – System‑Clean ist dann automatisch aktiv.
- `ZENTIFY_FORCE_NO_SYSTEM_CLEAN`: Setze auf `1`/`true`/`yes`/`on`, um systemweite Bereinigung AUSZUSCHALTEN – selbst wenn das Programm mit Administratorrechten läuft. Sicherheits‑Override.
- `ZENTIFY_PREFETCH`: Setze auf `1`/`true`/`yes`/`on`, um das Bereinigen des `Prefetch`‑Ordners explizit zu aktivieren (unabhängig von der Konfiguration).
- `ZENTIFY_MAX_PARALLELISM`: Ganzzahl (>0), begrenzt die parallelen Worker‑Threads. Standard ist `min(Anzahl_CPU_Kerne, 8)`.
- `ZENTIFY_WEB_BIND`: Bind‑Adresse für die Web UI (standardmäßig `127.0.0.1:7878`).
- `ZENTIFY_WEB_ALLOW_NON_LOCAL`: Wenn `1`, darf die Web UI an Nicht‑Loopback‑Adressen binden. Standard ist `0` (abgelehnt). Nicht empfohlen.
- `ZENTIFY_WEB_ALLOW_PATH_FALLBACK`: Wenn `1`, darf das Web‑Frontend als Fallback `zentify-cleaner` aus dem `PATH` starten. Standard ist `0`.
- `ZENTIFY_WEB_RUN_TIMEOUT_SECS`: Maximale Laufzeit für einen Cleaner‑Run über die Web‑API (Default: `600`). Bei Überschreitung wird der Prozess beendet und der Request mit 408 abgebrochen.

Beispiele:

```powershell
$env:ZENTIFY_ALLOW_SYSTEM_CLEAN = '1'  # System-Clean erlauben (ohne Admin)
$env:ZENTIFY_FORCE_NO_SYSTEM_CLEAN = '1'  # System-Clean verbieten (auch als Admin)
$env:ZENTIFY_PREFETCH = '1'  # Prefetch-Reinigung explizit aktivieren
$env:ZENTIFY_MAX_PARALLELISM = '4'  # 4 parallele Worker
```

Die Variablen gelten für die jeweilige Shell-Sitzung.

## FAQ

- Wie sicher ist das Tool?
  - Es löscht aggressiv Caches/Temps. Schutzmechanismen verhindern, dass hochsensible Top-Level-Systempfade als Ganzes bereinigt werden. Verwende `dry_run` für einen Probelauf und schließe Anwendungen vor dem Lauf.
- Benötige ich Administratorrechte?
  - Für reine Benutzerverzeichnisse: nein. Für systemweite Bereiche und `Prefetch`: ja, oder setze `ZENTIFY_ALLOW_SYSTEM_CLEAN=1` (mit Bedacht!).
- Was passiert bei gesperrten Dateien?
  - Gesperrte Pfade können nicht entfernt werden; das Tool versucht flaches Aufräumen. Starte Anwendungen neu/geschlossen und führe den Cleaner erneut aus.
- Unterstützt ihr Chrome/Edge/Brave/Vivaldi/Opera/Firefox?
  - Ja, die gängigen Chromium-basierten Browser sowie Firefox werden berücksichtigt, sofern Profile/Pfade existieren.
- Gibt es CLI-Flags wie `--dry-run`?
  - Ja. Unterstützt werden `--dry-run`, `--verbose`, `--quiet`. Flags haben Vorrang vor Env-Variablen und der Datei-Konfiguration.
- Werden unbekannte Konfigurationsschlüssel Fehler verursachen?
  - Nein, unbekannte Schlüssel werden aktuell ignoriert.

## Fehlerbehebung

1. Alle Browser/Programme schließen, die Caches offen halten könnten.
2. Bei Bedarf als Administrator ausführen (Rechtsklick → „Als Administrator ausführen“).
3. Mit `dry_run=true` und `verbose=true` starten, um exakt zu sehen, was passieren würde.
4. `ZENTIFY_MAX_PARALLELISM` reduzieren, wenn die I/O-Last zu hoch ist.
5. Prüfen, ob die erwarteten Pfade existieren (einige Ziele werden nur bereinigt, wenn Ordner vorhanden sind).

## Bekannte Einschränkungen

- Nur Windows (10/11, 64‑bit) wird unterstützt.
- Keine interaktive Bestätigung oder selektive Laufzeitfilterung (Designentscheidung; siehe Roadmap für optionale Interaktivität/Flags).
- Best‑effort bei Reparse-Points: Links werden als solche entfernt; Inhalte hinter Junctions werden nicht traversiert.
- UWP/Store‑Apps: Leeren von `LocalCache/TempState` kann initiale Re-Initialisierungen verursachen.
- Prefetch-Reinigung kann ersten Neustart von Anwendungen/Windows kurzfristig verlangsamen (normal und gewollt).

## Deinstallation

PowerShell:

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\uninstall.ps1
# Optional: auch Benutzerdaten unter %APPDATA%\Zentify entfernen
pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\uninstall.ps1 -PurgeConfig
```

CMD:

```bat
scripts\uninstall.cmd
```

Die Deinstallation stoppt laufende Prozesse (`zentify-web`, `zentify-cleaner`), entfernt die Startmenü‑ und optionalen Desktop‑Verknüpfungen sowie den GUI‑Launcher (`Zentify Web UI.cmd`) und löscht den Installationsordner. Außerdem wird der `PATH`‑Eintrag wieder bereinigt.

## Build aus dem Quellcode

```powershell
# Release-Build
cargo build --release --locked

# Ausführen (ohne Installation)
./target/release/zentify-cleaner.exe
```

Empfohlene Dev-Checks:

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-features --locked --verbose
```

## CI/CD

Das Repo enthält eine GitHub Actions Pipeline (`.github/workflows/ci.yml`) mit:

- Lint (fmt + clippy)
- Tests (Windows)
- Coverage (llvm-cov)
- MSRV-Build (1.70)
- Security Audit (cargo-audit)
- Release-Job für Windows (Artefakt inkl. CLI + Web UI + Launcher, SHA256)

## Roadmap

Die Roadmap ist mehrstufig und priorisiert Stabilität, Transparenz sowie sichere Defaults. Zeitpläne sind indikativ.

Phase 1 – CLI & UX

- `--dry-run`, `--verbose`, `--quiet` als CLI-Flags (neben Datei-Konfiguration).
- `--version`, `--help` und Exit-Codes verfeinern.
- Verbesserte Ausgabestruktur mit klaren Sections (Startkontext, Effektive Kategorien, Zusammenfassung).

Phase 2 – Konfiguration v1

- Validiertes Schema (JSON-Schema) für `.zentify/config.json` mit Warnungen bei unbekannten Schlüsseln (weiterhin tolerant).
- Ausschlussmuster (`excluded_patterns`) und Einschlussmuster (`include_patterns`) pro Kategorie.
- Benutzerdefinierte Verzeichnisse (`custom_directories`) mit Kategorie-Zuordnung.
- Per‑App‑Profile (feineres Enabling/Disabling für Browser/Teams/Office etc.).

Phase 3 – Logging & Observability

- Strukturierte Logs (z. B. JSON‑Lines) mit optionalem `--log-format json`.
- Export der Dry‑Run‑Ergebnisse als JSON/CSV (z. B. `--output report.json`).
- Detaillierte Metriken (dauer pro Ziel, Fehlerklassen, Anzahl gesperrter Objekte).

Phase 4 – Robustheit & Fehlerbehandlung

- Verbesserte Behandlung gesperrter Dateien (Retry-Strategien, klare Hinweise zu verursachenden Prozessen, soweit möglich).
- Granularere Fallbacks beim Löschen (Teil‑Reinigung je Ebene mit Fortschrittsindikator).
- Optionales „Skip on error“ pro Kategorie vs. „Fail fast“.

Phase 5 – Performance

- Adaptive Parallelität (I/O‑Backpressure, dynamische Threadzahl).
- Priorisierung großer Speicherfresser (zuerst große Dateien/Ordner für schnelle Freigabe sichtbarer Speicher).
- Optionaler I/O‑Scheduler für HDD vs. SSD optimierte Muster.

Phase 6 – Erweiterte Bereiche

- Weitere Caches (Visual Studio/.vs, NuGet, npm/yarn/pnpm, Python pip cache, Gradle/Maven), optional deaktiviert per Default.
- Feineres Windows‑Update‑Cleanup (unter Beachtung von WU/Known Issues, rein optional und transparent).
- Optionale Log‑Bereinigung (rotierende Logs, Größenlimits) – streng Opt‑in.

Phase 7 – Integrationen & Packaging

- Offizielles Winget/Chocolatey Paket.
- Signierte Releases, zusätzliche Artefakte (Portable/Installer‑Variante), `README.de.md/README.en.md`.
- Optionales Shell‑Integration Script (Kontextmenü „Zentify Cleaner ausführen“).

Phase 8 – Sicherheit & Qualität

- Erweiterte Tests auf Windows (Integrationstests mit temporären Sandbox‑Verzeichnissen).
- Regelmäßige `cargo-audit`/MSRV‑Updates (bereits in CI verankert, weiterhin pflegen).
- Threat‑Model‑Dokumentation und Security‑Guidelines.

Nice‑to‑have / Forschung

- Interaktiver Modus (`interactive: true`) mit Auswahl pro Kategorie (nur Opt‑in).
- Fortschrittsanzeige in TTY (bspw. `--progress`) vs. nicht‑TTY (reduzierte Ausgabe).
- Optionale Quarantäne (Papierkorb‑ähnlich) statt direktem Löschen, mit automatischer Aufräumfrist.

## Beitragen

Beiträge sind willkommen! Bitte halte dich an folgende Leitlinien:

- Code‑Stil prüfen: `cargo fmt --all -- --check`.
- Statische Analyse: `cargo clippy --all-targets -- -D warnings`.
- Tests ausführen: `cargo test --all-features --locked --verbose`.
- Pull Requests sollten knapp beschreiben, was geändert wurde, und ggf. Screens/Logs für Unterschiede in der Ausgabe enthalten.
- Beachte die Sicherheitsprinzipien dieses Projekts (keine riskanten Standard‑Aktionen, klare Opt‑ins, gute Fehlermeldungen).

Eröffne bei Fragen/Ideen bitte ein Issue mit einer klaren Problem‑ oder Featurebeschreibung.

## Support

- Probleme/Fehler bitte über GitHub Issues melden (inkl. OS‑Version, Log‑Ausschnitten und – wenn möglich – `dry_run`‑Ausgabe).
- Feature‑Wünsche gerne als „Feature Request“ markieren und den Use‑Case beschreiben.
- Sicherheitsrelevante Themen bitte verantwortungsvoll melden (Coordinated Disclosure).

## Lizenz

MIT – siehe [`LICENSE`](./LICENSE).

---

© 2025 Zentify Team
