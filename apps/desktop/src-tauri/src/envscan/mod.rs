//! Environment scan (§14).
//!
//! Detects the developer tooling J.A.R.V.I.S. builds on and reports it in a way
//! the user can act on. This is deliberately *real* detection — resolving the
//! executable, running it and parsing its version — not a guess based on the
//! existence of a folder.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// What a tool means to the product, which drives how loudly we report it
/// missing. A missing `Required` tool blocks real work; a missing `Optional`
/// one just narrows what J.A.R.V.I.S. can do.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ToolImportance {
    Required,
    Recommended,
    Optional,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ToolKind {
    Vcs,
    Runtime,
    PackageManager,
    Agent,
    Platform,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ToolState {
    Ready,
    Missing,
    /// Present but did not answer a version probe — usually a broken install.
    Degraded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolReport {
    pub id: String,
    pub name: String,
    pub kind: ToolKind,
    pub importance: ToolImportance,
    pub state: ToolState,
    pub version: Option<String>,
    pub path: Option<String>,
    /// Human-readable explanation shown when the tool is not Ready.
    pub detail: Option<String>,
    /// Whether the provider appears to hold stored credentials. Presence only —
    /// no secret is ever read (§60/§61).
    pub authenticated: Option<bool>,
    pub install_hint: Option<String>,
    pub install_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentReport {
    pub tools: Vec<ToolReport>,
    pub scanned_at: String,
    /// True when everything marked Required is Ready.
    pub ready: bool,
}

struct Probe {
    id: &'static str,
    name: &'static str,
    kind: ToolKind,
    importance: ToolImportance,
    bin: &'static str,
    args: &'static [&'static str],
    install_hint: &'static str,
    install_url: &'static str,
}

const PROBES: &[Probe] = &[
    Probe {
        id: "git",
        name: "Git",
        kind: ToolKind::Vcs,
        importance: ToolImportance::Required,
        bin: "git",
        args: &["--version"],
        install_hint: "winget install --id Git.Git",
        install_url: "https://git-scm.com/downloads",
    },
    Probe {
        id: "node",
        name: "Node.js",
        kind: ToolKind::Runtime,
        importance: ToolImportance::Recommended,
        bin: "node",
        args: &["--version"],
        install_hint: "winget install --id OpenJS.NodeJS.LTS",
        install_url: "https://nodejs.org",
    },
    Probe {
        id: "pnpm",
        name: "pnpm",
        kind: ToolKind::PackageManager,
        importance: ToolImportance::Optional,
        bin: "pnpm",
        args: &["--version"],
        install_hint: "npm install -g pnpm",
        install_url: "https://pnpm.io/installation",
    },
    Probe {
        id: "claude",
        name: "Claude Code",
        kind: ToolKind::Agent,
        importance: ToolImportance::Recommended,
        bin: "claude",
        args: &["--version"],
        install_hint: "npm install -g @anthropic-ai/claude-code",
        install_url: "https://claude.com/claude-code",
    },
    Probe {
        id: "codex",
        name: "Codex",
        kind: ToolKind::Agent,
        importance: ToolImportance::Recommended,
        bin: "codex",
        args: &["--version"],
        install_hint: "npm install -g @openai/codex",
        install_url: "https://developers.openai.com/codex/cli",
    },
    Probe {
        id: "gh",
        name: "GitHub CLI",
        kind: ToolKind::Platform,
        importance: ToolImportance::Optional,
        bin: "gh",
        args: &["--version"],
        install_hint: "winget install --id GitHub.cli",
        install_url: "https://cli.github.com",
    },
];

/// Extract the first version-shaped token from a `--version` banner.
///
/// Tool banners are wildly inconsistent — `git version 2.55.0.windows.3`,
/// `v24.19.0`, `codex-cli 0.147.0`, `2.1.240 (Claude Code)` — so this matches
/// the shape of a version rather than special-casing each tool.
fn parse_version(output: &str) -> Option<String> {
    let line = output.lines().find(|l| !l.trim().is_empty())?;
    for raw in line.split_whitespace() {
        let token = raw
            .trim_start_matches('v')
            .trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '-');

        let first_segment_is_numeric = token
            .split('.')
            .next()
            .is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));

        if first_segment_is_numeric && token.contains('.') {
            return Some(token.to_string());
        }
    }
    None
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Credential *presence* check. We look only at whether a credential store
/// exists — never at its contents (§60/§61).
fn detect_auth(id: &str) -> Option<bool> {
    let home = home_dir()?;
    match id {
        "claude" => {
            let creds = home.join(".claude").join(".credentials.json");
            let has_env = std::env::var("ANTHROPIC_API_KEY").is_ok();
            Some(creds.exists() || has_env)
        }
        "codex" => Some(home.join(".codex").join("auth.json").exists()),
        _ => None,
    }
}

fn which(bin: &str) -> Option<String> {
    let locator = if cfg!(windows) { "where" } else { "which" };
    let mut cmd = Command::new(locator);
    cmd.arg(bin);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
}

fn run_probe(probe: &Probe) -> ToolReport {
    let path = which(probe.bin);

    let mut report = ToolReport {
        id: probe.id.to_string(),
        name: probe.name.to_string(),
        kind: probe.kind,
        importance: probe.importance,
        state: ToolState::Missing,
        version: None,
        path: path.clone(),
        detail: None,
        authenticated: None,
        install_hint: Some(probe.install_hint.to_string()),
        install_url: Some(probe.install_url.to_string()),
    };

    if path.is_none() {
        report.detail = Some(format!("{} was not found on PATH.", probe.name));
        return report;
    }

    let mut cmd = Command::new(probe.bin);
    cmd.args(probe.args);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    match cmd.output() {
        Ok(out) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            match parse_version(&text) {
                Some(v) => {
                    report.state = ToolState::Ready;
                    report.version = Some(v);
                }
                None => {
                    report.state = ToolState::Degraded;
                    report.detail =
                        Some("Installed, but did not report a recognisable version.".into());
                }
            }
        }
        Err(e) => {
            report.state = ToolState::Degraded;
            report.detail = Some(format!("Could not be launched: {e}"));
        }
    }

    report.authenticated = detect_auth(probe.id);
    report
}

/// Scan the environment. Probes run concurrently because each one costs a
/// process spawn, and a serial scan is perceptibly slow during onboarding.
pub fn scan() -> EnvironmentReport {
    let handles: Vec<_> = PROBES
        .iter()
        .map(|p| std::thread::spawn(move || run_probe(p)))
        .collect();

    let mut tools: Vec<ToolReport> = handles.into_iter().filter_map(|h| h.join().ok()).collect();

    tools.sort_by_key(|t| match t.importance {
        ToolImportance::Required => 0,
        ToolImportance::Recommended => 1,
        ToolImportance::Optional => 2,
    });

    let ready = tools
        .iter()
        .filter(|t| t.importance == ToolImportance::Required)
        .all(|t| t.state == ToolState::Ready);

    EnvironmentReport {
        tools,
        scanned_at: now_iso(),
        ready,
    }
}

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    // Minimal RFC3339 rendering, so the core does not depend on a date crate.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant's `civil_from_days`: days since the Unix epoch to a civil date.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[tauri::command]
pub fn scan_environment() -> EnvironmentReport {
    scan()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_world_version_banners() {
        // Verbatim banners from the tools this product integrates with.
        assert_eq!(
            parse_version("git version 2.55.0.windows.3").as_deref(),
            Some("2.55.0.windows.3")
        );
        assert_eq!(parse_version("v24.19.0").as_deref(), Some("24.19.0"));
        assert_eq!(parse_version("codex-cli 0.147.0").as_deref(), Some("0.147.0"));
        assert_eq!(parse_version("2.1.240 (Claude Code)").as_deref(), Some("2.1.240"));
        assert_eq!(parse_version("11.20.0").as_deref(), Some("11.20.0"));
        assert_eq!(
            parse_version("gh version 2.97.0 (2026-07-31)\nhttps://github.com/cli/cli").as_deref(),
            Some("2.97.0")
        );
    }

    #[test]
    fn rejects_output_without_a_version() {
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("command not found"), None);
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_687), (2026, 8, 22));
        // Leap-day boundary, where the era arithmetic is easiest to get wrong.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }
}
