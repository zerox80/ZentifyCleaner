use std::env;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn run_git(args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn main() {
    // Rerun markers when git references change
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");

    // Commit
    let commit = run_git(&["rev-parse", "HEAD"]) // full sha
        .or_else(|| env::var("GITHUB_SHA").ok())
        .unwrap_or_else(|| "unknown".to_string());
    let short_commit = commit.chars().take(12).collect::<String>();

    // Describe (tag/dirty) if available
    let describe = run_git(&["describe", "--tags", "--always", "--dirty"]).unwrap_or_else(|| "unknown".to_string());

    // Build time (unix seconds)
    let build_unix_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());

    // Target triple
    let target = env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());

    // Export as compile-time env
    println!("cargo:rustc-env=GIT_COMMIT={}", short_commit);
    println!("cargo:rustc-env=GIT_DESCRIBE={}", describe);
    println!("cargo:rustc-env=BUILD_UNIX_TIME={}", build_unix_time);
    println!("cargo:rustc-env=BUILD_TARGET={}", target);
}
