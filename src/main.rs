//! taurio-shim — one binary, many names.
//!
//! Copy or hardlink this exe to `<name>.exe` next to a `<name>.shim` config
//! file. When invoked, the shim looks at its own filename, reads the matching
//! `.shim` file, and execs the target with the configured args plus whatever
//! the user passed on the command line.
//!
//! Special cases for `php` and `composer` — those use Taurio's project-aware
//! PHP runtime resolver so the right runtime is picked based on `.taurio.json`
//! in the cwd / its ancestors and on the global `taurio.json` config.

use serde::Deserialize;
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

const COMPOSER_PHAR: &str = r"C:\Taurio\bin\composer\composer.phar";

#[derive(Debug, Deserialize)]
struct TaurioConfig {
    #[serde(default)]
    default_php: String,
    #[serde(default)]
    php_runtimes: Vec<PhpRuntime>,
}

#[derive(Debug, Deserialize)]
struct PhpRuntime {
    version: String,
    php_path: String,
}

#[derive(Debug, Deserialize)]
struct ProjectConfig {
    #[serde(default)]
    php_version: Option<String>,
}

fn main() {
    let exe = env::current_exe().unwrap_or_else(|e| die(&format!("current_exe: {}", e)));
    let stem = exe
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| die("could not read shim filename"));
    let dir = exe
        .parent()
        .unwrap_or_else(|| die("shim has no parent dir"))
        .to_path_buf();

    let user_args: Vec<OsString> = env::args_os().skip(1).collect();

    let (target, prepend) = match stem.as_str() {
        "php" => resolve_php(),
        "composer" => resolve_composer(),
        _ => read_shim_file(&dir, &stem),
    };

    let mut full_args = prepend;
    full_args.extend(user_args);

    let status = Command::new(&target)
        .args(&full_args)
        .status()
        .unwrap_or_else(|e| die(&format!("failed to spawn {}: {}", target.display(), e)));

    exit(status.code().unwrap_or(1));
}

fn die(msg: &str) -> ! {
    eprintln!("taurio-shim: {}", msg);
    exit(127)
}

/// Read a sibling `<stem>.shim` file. Format (one key per line):
///   path = "C:\path\to\target.exe"
///   args = "--default --args here"
fn read_shim_file(dir: &Path, stem: &str) -> (PathBuf, Vec<OsString>) {
    let shim_path = dir.join(format!("{}.shim", stem));
    let raw = std::fs::read_to_string(&shim_path)
        .unwrap_or_else(|e| die(&format!("missing or unreadable {}: {}", shim_path.display(), e)));

    let mut path: Option<String> = None;
    let mut args_line: Option<String> = None;

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_lowercase();
            let value = unquote(value.trim());
            match key.as_str() {
                "path" => path = Some(value),
                "args" => args_line = Some(value),
                _ => {}
            }
        }
    }

    let target = path.unwrap_or_else(|| die(&format!("{} missing 'path' key", shim_path.display())));
    let args = args_line
        .map(|s| split_cmdline(&s))
        .unwrap_or_default();

    (PathBuf::from(target), args)
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Minimal command-line splitter for the args = "..." line. Honors double
/// quotes around tokens with spaces. Good enough for shim default args.
fn split_cmdline(s: &str) -> Vec<OsString> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut in_quotes = false;
    for c in s.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ' ' | '\t' if !in_quotes => {
                if !buf.is_empty() {
                    out.push(OsString::from(buf.clone()));
                    buf.clear();
                }
            }
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() {
        out.push(OsString::from(buf));
    }
    out
}

fn resolve_php() -> (PathBuf, Vec<OsString>) {
    let php_path = pick_php_path().unwrap_or_else(|| {
        die("could not resolve php runtime — check Taurio config and installed runtimes")
    });
    (PathBuf::from(php_path), Vec::new())
}

fn resolve_composer() -> (PathBuf, Vec<OsString>) {
    let php_path = pick_php_path().unwrap_or_else(|| {
        die("could not resolve php runtime for composer")
    });
    if !Path::new(COMPOSER_PHAR).exists() {
        die(&format!("composer.phar not installed at {}", COMPOSER_PHAR));
    }
    (PathBuf::from(php_path), vec![OsString::from(COMPOSER_PHAR)])
}

/// Walk cwd → root looking for `.taurio.json` / `.tauri.json` and read its
/// `php_version`. Then read `%APPDATA%\Taurio\taurio.json` and return the
/// matching runtime, falling back to `default_php`, then to the first runtime.
fn pick_php_path() -> Option<String> {
    let project_version = find_project_php_version();

    let appdata = env::var("APPDATA").ok()?;
    let cfg_path = PathBuf::from(appdata).join("Taurio").join("taurio.json");
    let raw = std::fs::read_to_string(&cfg_path).ok()?;
    let cfg: TaurioConfig = serde_json::from_str(&raw).ok()?;

    let want = project_version
        .as_ref()
        .map(|v| normalize_version(v))
        .or_else(|| {
            if cfg.default_php.trim().is_empty() {
                None
            } else {
                Some(normalize_version(&cfg.default_php))
            }
        });

    if let Some(v) = want {
        if let Some(rt) = cfg.php_runtimes.iter().find(|r| normalize_version(&r.version) == v) {
            return Some(rt.php_path.clone());
        }
    }
    cfg.php_runtimes.first().map(|r| r.php_path.clone())
}

fn find_project_php_version() -> Option<String> {
    let mut dir = env::current_dir().ok()?;
    loop {
        for name in [".taurio.json", ".tauri.json"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                if let Ok(raw) = std::fs::read_to_string(&candidate) {
                    if let Ok(parsed) = serde_json::from_str::<ProjectConfig>(&raw) {
                        if let Some(v) = parsed.php_version {
                            if !v.trim().is_empty() {
                                return Some(v);
                            }
                        }
                    }
                }
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn normalize_version(v: &str) -> String {
    let mut s = v.trim().to_lowercase();
    if let Some(rest) = s.strip_prefix("php") {
        s = rest.to_string();
    }
    if let Some(rest) = s.strip_prefix('-') {
        s = rest.to_string();
    }
    s
}
