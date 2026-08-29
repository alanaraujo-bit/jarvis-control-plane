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
    // The local model runtime (§92). Optional rather than recommended: a
    // machine without a capable GPU is a perfectly good machine for this
    // product, and marking a 17 GB download as something the user is missing
    // would be advice rather than a scan.
    Probe {
        id: "ollama",
        name: "Ollama",
        kind: ToolKind::Agent,
        importance: ToolImportance::Optional,
        bin: "ollama",
        args: &["--version"],
        install_hint: "winget install --id Ollama.Ollama",
        install_url: "https://ollama.com/download",
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

/// Pick the entry the operating system would actually execute.
///
/// `where` lists every match, and for npm-installed tools that includes a
/// extensionless shell script *before* the Windows launcher:
///
/// ```text
/// C:\Users\me\AppData\Roaming\npm\pnpm       <- cannot be spawned directly
/// C:\Users\me\AppData\Roaming\npm\pnpm.cmd   <- what actually runs
/// ```
///
/// Taking the first line reported pnpm as broken on a machine where it works
/// perfectly. Candidates are ranked by `PATHEXT` instead, which is the same
/// order the shell itself resolves.
fn preferred_executable(candidates: &[String]) -> Option<String> {
    if !cfg!(windows) {
        return candidates.first().cloned();
    }

    let pathext = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
        .to_uppercase();
    let order: Vec<String> = pathext.split(';').map(|e| e.trim().to_string()).collect();

    let rank = |path: &String| -> usize {
        let upper = path.to_uppercase();
        order
            .iter()
            .position(|ext| !ext.is_empty() && upper.ends_with(ext))
            // No recognised extension sorts last: it is the least likely to be
            // directly executable.
            .unwrap_or(order.len())
    };

    candidates.iter().min_by_key(|c| rank(c)).cloned()
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

    let candidates: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    preferred_executable(&candidates)
}

/// Build a `Command` that will actually start a tool resolved from PATH.
///
/// Two Windows facts conspire here. `CreateProcess` does not apply PATHEXT, so
/// `Command::new("pnpm")` cannot find `pnpm.cmd`; and a `.cmd`/`.bat` is a
/// script, not an executable, so only the command interpreter can run it.
/// Without both, a perfectly working pnpm was reported as "program not found"
/// in the environment scan.
///
/// Shared with `accounts`, which has to run `claude auth status` for exactly
/// the same reason and would otherwise repeat this — and get it subtly wrong,
/// because `claude` on this machine is a `.cmd` shim too.
pub(crate) fn tool_command(bin: &str, args: &[&str]) -> Option<Command> {
    let resolved = which(bin)?;
    let is_batch = resolved
        .rsplit('.')
        .next()
        .map(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
        .unwrap_or(false);

    let mut cmd = if cfg!(windows) && is_batch {
        let mut c = Command::new(std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()));
        c.arg("/c").arg(&resolved).args(args);
        c
    } else {
        let mut c = Command::new(&resolved);
        c.args(args);
        c
    };
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    Some(cmd)
}

/// Run a tool and return its standard output, or `None` if anything went wrong.
///
/// One extra environment variable may be applied, which is what lets an account
/// be interrogated at its own configuration directory without touching the
/// machine's. A non-zero exit is `None`: a status command that failed has not
/// told us anything, and treating its error text as an answer is how a
/// signed-in account gets reported as signed out.
pub(crate) fn run_tool(bin: &str, args: &[&str], env: Option<(String, String)>) -> Option<String> {
    let mut cmd = tool_command(bin, args)?;
    if let Some((key, value)) = env {
        cmd.env(key, value);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
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

    // Spawn the resolved path, not the bare name, and route batch wrappers
    // through the shell.
    //
    // Two Windows facts conspire here. `CreateProcess` does not apply PATHEXT,
    // so `Command::new("pnpm")` cannot find `pnpm.cmd`; and a `.cmd`/`.bat` is
    // a script, not an executable, so it can only be run by the command
    // interpreter. Without both, a perfectly working pnpm was reported as
    // "program not found" in the environment scan.
    let resolved = path.as_deref().unwrap_or(probe.bin);
    let is_batch = resolved
        .rsplit('.')
        .next()
        .map(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
        .unwrap_or(false);

    let mut cmd = if cfg!(windows) && is_batch {
        let mut c = Command::new(std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()));
        c.arg("/c").arg(resolved).args(probe.args);
        c
    } else {
        let mut c = Command::new(resolved);
        c.args(probe.args);
        c
    };
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
        assert_eq!(
            parse_version("codex-cli 0.147.0").as_deref(),
            Some("0.147.0")
        );
        assert_eq!(
            parse_version("2.1.240 (Claude Code)").as_deref(),
            Some("2.1.240")
        );
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

    /// Reproduces the real failure: pnpm was reported broken on a machine where
    /// it works, because `where` lists the extensionless npm shim first.
    #[test]
    #[cfg(windows)]
    fn prefers_the_launcher_over_an_extensionless_shim() {
        let candidates = vec![
            r"C:\Users\me\AppData\Roaming\npm\pnpm".to_string(),
            r"C:\Users\me\AppData\Roaming\npm\pnpm.cmd".to_string(),
        ];
        assert_eq!(
            preferred_executable(&candidates).as_deref(),
            Some(r"C:\Users\me\AppData\Roaming\npm\pnpm.cmd")
        );
    }

    #[test]
    #[cfg(windows)]
    fn respects_pathext_when_exe_and_cmd_both_exist() {
        let candidates = vec![
            r"C:\tools\thing.cmd".to_string(),
            r"C:\tools\thing.exe".to_string(),
        ];
        let pathext = std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .to_uppercase();
        let exe_first =
            pathext.find(".EXE").unwrap_or(usize::MAX) < pathext.find(".CMD").unwrap_or(usize::MAX);
        let expected = if exe_first {
            r"C:\tools\thing.exe"
        } else {
            r"C:\tools\thing.cmd"
        };
        assert_eq!(preferred_executable(&candidates).as_deref(), Some(expected));
    }

    /// The whole point of the scan is to report what is actually usable, so
    /// this drives the real probe against the real tools on this machine.
    #[test]
    fn scans_this_machine_and_reports_installed_tools_as_ready() {
        let report = scan();
        assert!(!report.tools.is_empty());

        // Git is required and is present here; if the scan cannot see it, the
        // scan is broken rather than the machine.
        let git = report
            .tools
            .iter()
            .find(|t| t.id == "git")
            .expect("git probe");
        assert_eq!(git.state, ToolState::Ready, "git detail: {:?}", git.detail);
        assert!(git.version.is_some());

        // Any tool we found on PATH must also be launchable. A tool that
        // resolves but cannot be run is the pnpm `.cmd` bug.
        for tool in &report.tools {
            if tool.path.is_some() {
                assert_ne!(
                    tool.state,
                    ToolState::Degraded,
                    "{} resolved to {:?} but could not be launched: {:?}",
                    tool.name,
                    tool.path,
                    tool.detail
                );
            }
        }
    }

    #[test]
    fn a_single_candidate_is_returned_unchanged() {
        let one = vec![r"C:\Program Files\Git\cmd\git.exe".to_string()];
        assert_eq!(preferred_executable(&one), Some(one[0].clone()));
        assert_eq!(preferred_executable(&[]), None);
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_687), (2026, 8, 22));
        // Leap-day boundary, where the era arithmetic is easiest to get wrong.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }
}
