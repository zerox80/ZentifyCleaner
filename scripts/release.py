#!/usr/bin/env python3
"""
Automated build + package + GitHub release helper for Zentify Cleaner (Windows).

What it does:
- Builds the Rust project in release mode with the optional Web feature
  using a timestamped target directory to avoid file locks.
- Collects the two executables (zentify-cleaner.exe, zentify-web.exe).
- Creates a handy Windows launcher "Zentify Web UI.cmd".
- Produces a ZIP bundle in ./dist.
- Optionally creates/updates a GitHub release and uploads the ZIP and EXEs
  as release assets.

Requirements:
- Python 3.8+
- Rust/Cargo available on PATH
- Git installed (to detect repo and HEAD)
- A GitHub token via env var GITHUB_TOKEN (scope: repo) to publish a release

Usage examples (PowerShell):
  # Build + ZIP only (no GitHub interaction)
  python .\scripts\release.py --zip-only

  # Full pipeline: build, zip, create GitHub release, upload assets
  $env:GITHUB_TOKEN = "<your_token>"
  python .\scripts\release.py --publish

Optional flags:
  --repo owner/name         # Override repo detection (default: parsed from git remote or $GITHUB_REPOSITORY)
  --tag vX.Y.Y              # Override tag/version (default: v<version> from Cargo.toml)
  --draft                   # Create release as draft (default: not draft)
  --prerelease              # Mark release as prerelease
  --skip-build              # Reuse last build (expects target dir to exist)
  --jobs 1                  # Cargo jobs (default 1 to reduce lock issues)
  --dry-run                 # Print actions, skip network calls and uploads

The script uses only Python's standard library for HTTP calls.
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import shutil
import subprocess
import sys
import urllib.parse
import urllib.request
from pathlib import Path
from zipfile import ZipFile, ZIP_DEFLATED

ROOT = Path(__file__).resolve().parents[1]


def run(cmd: list[str] | str, cwd: Path | None = None, env: dict | None = None, check: bool = True) -> subprocess.CompletedProcess:
    if isinstance(cmd, list):
        print("$", " ".join(cmd))
    else:
        print("$", cmd)
    cp = subprocess.run(cmd, cwd=str(cwd) if cwd else None, env=env, capture_output=True, text=True, shell=isinstance(cmd, str))
    if cp.stdout:
        print(cp.stdout)
    if cp.stderr:
        sys.stderr.write(cp.stderr)
    if check and cp.returncode != 0:
        raise RuntimeError(f"Command failed with exit code {cp.returncode}: {cmd}")
    return cp


def read_cargo_package() -> tuple[str, str]:
    cargo = (ROOT / "Cargo.toml").read_text(encoding="utf-8", errors="ignore")
    # naive parse under [package]
    pkg_re = re.compile(r"^\[package\][\s\S]*?^(?:\[|$)", re.M)
    m = pkg_re.search(cargo)
    block = m.group(0) if m else cargo
    name = re.search(r"^name\s*=\s*\"([^\"]+)\"", block, re.M)
    ver = re.search(r"^version\s*=\s*\"([^\"]+)\"", block, re.M)
    if not name or not ver:
        raise RuntimeError("Failed to parse package name/version from Cargo.toml")
    return name.group(1), ver.group(1)


def timestamp() -> str:
    return dt.datetime.now().strftime("%Y%m%d_%H%M%S")


def ensure_dir(p: Path) -> None:
    p.mkdir(parents=True, exist_ok=True)


def cargo_build(target_dir: Path, jobs: int = 1, skip_build: bool = False) -> None:
    if skip_build:
        print("[i] Skipping build as requested")
        return
    env = os.environ.copy()
    env.setdefault("CARGO_INCREMENTAL", "0")
    cmd = [
        "cargo", "build", "--release", "--features", "web", "--locked", "-j", str(jobs),
        "--target-dir", str(target_dir)
    ]
    run(cmd, cwd=ROOT, env=env, check=True)


def find_artifacts(target_dir: Path) -> dict[str, Path]:
    release_dir = target_dir / "release"
    zc = release_dir / "zentify-cleaner.exe"
    zw = release_dir / "zentify-web.exe"
    if not zc.exists() or not zw.exists():
        raise FileNotFoundError(f"Executables not found in {release_dir}")
    return {"zentify-cleaner.exe": zc, "zentify-web.exe": zw}


def write_web_launcher(out_dir: Path) -> Path:
    content = (
        "@echo off\r\n"
        "setlocal\r\n"
        "set \"DIR=%~dp0\"\r\n"
        "echo Starte Zentify Web UI ...\r\n"
        # Use PowerShell to elevate
        "powershell -NoProfile -ExecutionPolicy Bypass -Command "
        "\"Start-Process -Verb RunAs -FilePath '%DIR%zentify-web.exe'\"\r\n"
    )
    cmd_path = out_dir / "Zentify Web UI.cmd"
    cmd_path.write_text(content, encoding="utf-8")
    return cmd_path


def build_zip(name: str, version: str, artifacts: dict[str, Path], dist_dir: Path) -> Path:
    ensure_dir(dist_dir)
    zip_name = f"{name}-{version}-windows-x64.zip"
    zip_path = dist_dir / zip_name

    tmp_dir = dist_dir / f"bundle_{timestamp()}"
    ensure_dir(tmp_dir)

    # Copy executables
    for fn, src in artifacts.items():
        shutil.copy2(src, tmp_dir / fn)

    # Copy docs/icons if exist
    for fn in ["README.md", "LICENSE", "Icon.png"]:
        fp = ROOT / fn
        if fp.exists():
            shutil.copy2(fp, tmp_dir / fp.name)

    # Create launcher
    write_web_launcher(tmp_dir)

    # Zip
    with ZipFile(zip_path, "w", compression=ZIP_DEFLATED) as z:
        for p in tmp_dir.iterdir():
            z.write(p, arcname=p.name)

    shutil.rmtree(tmp_dir, ignore_errors=True)
    print(f"[+] Created {zip_path}")
    return zip_path


# ---------------- GitHub API (std lib only) ----------------

def http_json(method: str, url: str, token: str | None = None, data: dict | None = None) -> dict:
    headers = {
        "Accept": "application/vnd.github+json",
        "User-Agent": "zentify-release-script",
        "X-GitHub-Api-Version": "2022-11-28",
    }
    if token:
        headers["Authorization"] = f"Bearer {token}"
    body_bytes = None
    if data is not None:
        body_bytes = json.dumps(data).encode("utf-8")
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, method=method, data=body_bytes, headers=headers)
    with urllib.request.urlopen(req) as resp:
        resp_body = resp.read()
        if not resp_body:
            return {}
        return json.loads(resp_body.decode("utf-8"))


def http_upload(url: str, token: str, file_path: Path, content_type: str) -> dict:
    headers = {
        "Accept": "application/vnd.github+json",
        "User-Agent": "zentify-release-script",
        "X-GitHub-Api-Version": "2022-11-28",
        "Content-Type": content_type,
    }
    headers["Authorization"] = f"Bearer {token}"
    with file_path.open("rb") as f:
        data = f.read()
    req = urllib.request.Request(url, method="POST", data=data, headers=headers)
    with urllib.request.urlopen(req) as resp:
        resp_body = resp.read()
        if not resp_body:
            return {}
        return json.loads(resp_body.decode("utf-8"))


def detect_repo(arg_repo: str | None) -> str:
    # Priority: CLI arg > GITHUB_REPOSITORY > git remote origin
    if arg_repo:
        return arg_repo
    if os.environ.get("GITHUB_REPOSITORY"):
        return os.environ["GITHUB_REPOSITORY"]
    try:
        cp = run(["git", "config", "--get", "remote.origin.url"], cwd=ROOT, check=False)
        url = (cp.stdout or "").strip()
        if url:
            # Supported: https://github.com/owner/repo(.git)? or git@github.com:owner/repo(.git)?
            m = re.search(r"github\.com[:/](?P<owner>[^/]+)/(?P<repo>[^.\s]+)", url)
            if m:
                return f"{m.group('owner')}/{m.group('repo')}"
    except Exception:
        pass
    raise RuntimeError("Unable to detect GitHub repository. Use --repo owner/name or set GITHUB_REPOSITORY.")


def get_head_ref() -> str:
    cp = run(["git", "rev-parse", "--abbrev-ref", "HEAD"], cwd=ROOT, check=False)
    ref = (cp.stdout or "main").strip()
    return ref or "main"


def create_or_get_release(token: str, repo: str, tag: str, name: str, body: str, draft: bool, prerelease: bool, target_commitish: str) -> dict:
    # Try to find existing release
    rel_url = f"https://api.github.com/repos/{repo}/releases/tags/{urllib.parse.quote(tag)}"
    try:
        existing = http_json("GET", rel_url, token)
        if existing and existing.get("id"):
            print(f"[i] Release for tag {tag} already exists (id={existing['id']}).")
            return existing
    except urllib.error.HTTPError as e:
        if e.code != 404:
            raise
    # Create new release
    url = f"https://api.github.com/repos/{repo}/releases"
    payload = {
        "tag_name": tag,
        "name": name,
        "body": body,
        "draft": draft,
        "prerelease": prerelease,
        "target_commitish": target_commitish,
    }
    created = http_json("POST", url, token, payload)
    print(f"[+] Created release id={created.get('id')} tag={tag}")
    return created


def upload_asset_to_release(token: str, release: dict, file_path: Path, content_type: str) -> dict:
    upload_url_tmpl = release.get("upload_url", "")  # ...{?name,label}
    if not upload_url_tmpl:
        raise RuntimeError("Missing upload_url in release response")
    base = upload_url_tmpl.split("{", 1)[0]
    name = file_path.name
    url = f"{base}?name={urllib.parse.quote(name)}"
    print(f"[i] Uploading asset {name} ...")
    resp = http_upload(url, token, file_path, content_type)
    print(f"[+] Uploaded asset id={resp.get('id')} name={resp.get('name')}")
    return resp


def main() -> int:
    parser = argparse.ArgumentParser(description="Build + Zip + GitHub Release helper")
    parser.add_argument("--publish", action="store_true", help="Create/Update GitHub release and upload assets")
    parser.add_argument("--zip-only", action="store_true", help="Only build + zip (no GitHub calls)")
    parser.add_argument("--repo", default=None, help="GitHub repo owner/name (override auto-detection)")
    parser.add_argument("--tag", default=None, help="Tag to use (default: v<version> from Cargo.toml)")
    parser.add_argument("--draft", action="store_true", help="Create release as draft")
    parser.add_argument("--prerelease", action="store_true", help="Mark release as prerelease")
    parser.add_argument("--skip-build", action="store_true", help="Skip cargo build and reuse last artifacts")
    parser.add_argument("--jobs", type=int, default=1, help="Cargo jobs (default 1)")
    parser.add_argument("--dry-run", action="store_true", help="Print actions, skip network calls")

    args = parser.parse_args()

    name, version = read_cargo_package()
    tag = args.tag or f"v{version}"
    repo = detect_repo(args.repo)
    branch = get_head_ref()

    td = ROOT / f"target_build_{timestamp()}"
    dist_dir = ROOT / "dist"

    ensure_dir(dist_dir)

    try:
        cargo_build(td, jobs=args.jobs, skip_build=args.skip_build)
        artifacts = find_artifacts(td)
        zip_path = build_zip(name, version, artifacts, dist_dir)

        if args.zip_only and not args.publish:
            print("[i] Zip-only mode complete.")
            print(f"Artifacts:\n  {artifacts['zentify-cleaner.exe']}\n  {artifacts['zentify-web.exe']}\n  {zip_path}")
            return 0

        if not args.publish:
            print("[i] Build + Zip done. Skipping GitHub because --publish not set.")
            print(f"Artifacts:\n  {artifacts['zentify-cleaner.exe']}\n  {artifacts['zentify-web.exe']}\n  {zip_path}")
            return 0

        token = os.environ.get("GITHUB_TOKEN")
        if not token:
            raise RuntimeError("GITHUB_TOKEN not set. Export a GitHub token to publish release.")

        rel_name = f"{name} {tag}"
        rel_body = (
            f"Automated release for {name} {tag}.\n\n"
            f"Includes CLI and Web UI executables plus a launcher:\n\n"
            f"- zentify-cleaner.exe\n"
            f"- zentify-web.exe\n"
            f"- Zentify Web UI.cmd\n\n"
            f"See README.md for usage and notes."
        )

        if args.dry_run:
            print("[dry-run] Would create or fetch release and upload assets:")
            print(f"  Repo: {repo}")
            print(f"  Tag:  {tag}")
            print(f"  Name: {rel_name}")
            print(f"  Files: {zip_path.name}, {artifacts['zentify-cleaner.exe'].name}, {artifacts['zentify-web.exe'].name}")
            return 0

        release = create_or_get_release(
            token=token,
            repo=repo,
            tag=tag,
            name=rel_name,
            body=rel_body,
            draft=args.draft,
            prerelease=args.prerelease,
            target_commitish=branch,
        )

        # Upload ZIP and standalone executables
        upload_asset_to_release(token, release, zip_path, content_type="application/zip")
        upload_asset_to_release(token, release, artifacts["zentify-cleaner.exe"], content_type="application/octet-stream")
        upload_asset_to_release(token, release, artifacts["zentify-web.exe"], content_type="application/octet-stream")

        print("[+] Release published successfully.")
        return 0

    except Exception as e:
        print(f"[ERROR] {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
