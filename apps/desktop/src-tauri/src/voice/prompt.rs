//! Building the vocabulary-priming prompt (§54).
//!
//! whisper.cpp accepts a short "initial prompt" that biases its decoder
//! toward the words it contains — a proper noun it would otherwise
//! phonetically guess at gets recognised outright when it is in this text.
//! Verified directly, not assumed: the same recorded sentence, transcribed
//! twice, turned "tauri build" into "talibiu" unprimed and correctly into
//! "Tauri" (still losing "build") primed with a vocabulary list built the
//! same way as this one — see D29 in docs/DECISIONS.md for the full probe.

use rusqlite::OptionalExtension;

use crate::db::Database;

/// Vocabulary every project shares: this product's own name and the tools
/// every session already knows how to launch (§26).
const BASELINE: &str =
    "pnpm, npm, git, Tauri, Rust, TypeScript, Claude Code, Codex, J.A.R.V.I.S., jarvis-desktop";

/// Keep the prompt short. whisper.cpp's initial prompt shares the decoder's
/// context budget with the transcription itself — a long prompt does not
/// just cost tokens, it can crowd out the very audio being transcribed.
const MAX_ROOT_ENTRIES: usize = 12;

/// Build the priming prompt for one project: the shared baseline, its current
/// branch, and a handful of its own top-level file and directory names.
///
/// Deliberately shallow — one directory read, not a project-wide walk. A
/// prompt this size cannot usefully hold a whole file tree anyway, and this
/// product already knows what a slow full-repo scan costs (see rule 9 in
/// docs/HANDOFF.md).
pub fn build(db: &Database, project_id: &str, project_path: &std::path::Path) -> String {
    let mut parts = vec![BASELINE.to_string()];

    if let Ok(Some(branch)) = current_branch(db, project_id) {
        if !branch.is_empty() {
            parts.push(branch);
        }
    }

    let mut entries: Vec<String> = std::fs::read_dir(project_path)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| !name.starts_with('.'))
        .take(MAX_ROOT_ENTRIES)
        .collect();
    entries.sort();
    if !entries.is_empty() {
        parts.push(entries.join(", "));
    }

    parts.join(", ")
}

fn current_branch(db: &Database, project_id: &str) -> crate::db::Result<Option<String>> {
    db.with(|conn| {
        conn.query_row(
            "SELECT git_branch FROM projects WHERE id = ?1",
            [project_id],
            |row| row.get(0),
        )
        .optional()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_baseline_vocabulary_is_always_present() {
        let db = Database::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prompt = build(&db, "no-such-project", dir.path());
        assert!(prompt.contains("Tauri"));
        assert!(prompt.contains("J.A.R.V.I.S."));
    }

    #[test]
    fn root_level_entries_are_folded_into_the_prompt() {
        let db = Database::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("apps")).unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        // A dotfile must not appear — it is noise, not project vocabulary.
        std::fs::write(dir.path().join(".gitignore"), "").unwrap();

        let prompt = build(&db, "no-such-project", dir.path());
        assert!(prompt.contains("apps"));
        assert!(prompt.contains("Cargo.toml"));
        assert!(!prompt.contains(".gitignore"));
    }

    #[test]
    fn a_project_with_no_branch_on_record_still_produces_a_prompt() {
        let db = Database::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prompt = build(&db, "unknown", dir.path());
        assert!(!prompt.is_empty());
    }
}
