//! Unified diffs, parsed into something a review surface can render (§43).
//!
//! The webview is given hunks and numbered lines, not a patch to re-parse in
//! JavaScript. Two reasons: line numbers are what makes a diff readable and
//! deriving them is exactly the fiddly part, and a structured diff can be shown
//! side by side or inline from the same data without asking Git twice.
//!
//! Git is the only thing that computes a diff here (D5). We parse its output;
//! we never diff two files ourselves and hope the result agrees with what the
//! user's `git diff` would have said.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::status::ChangeKind;

/// Most lines of diff that will be sent to the webview for one file.
///
/// A generated lockfile can be tens of thousands of lines. Sending them all
/// would stall the renderer to display something nobody reads line by line, so
/// the diff is cut and **says** it was cut (§81).
pub const MAX_DIFF_LINES: usize = 4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: LineKind,
    /// Line number on the left, absent for an added line.
    pub old_line: Option<u32>,
    /// Line number on the right, absent for a removed line.
    pub new_line: Option<u32>,
    pub text: String,
    /// Git's `\ No newline at end of file` applied to this line.
    pub no_newline: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    pub old_start: u32,
    pub new_start: u32,
    /// The text after the closing `@@`, which Git fills with the enclosing
    /// function or section when it can work one out. Free context, worth
    /// showing.
    pub heading: Option<String>,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub path: String,
    pub from_path: Option<String>,
    pub kind: ChangeKind,
    pub binary: bool,
    /// We declined to read the file at all. Only an untracked file can be in
    /// this state — Git happily diffs a large tracked file and we truncate it.
    pub too_large: bool,
    pub insertions: u32,
    pub deletions: u32,
    pub hunks: Vec<Hunk>,
    /// The diff was cut at `MAX_DIFF_LINES`.
    pub truncated: bool,
}

/// Parse the `-oldStart,oldCount +newStart,newCount` part of a hunk header.
///
/// A single-line range omits the count entirely (`@@ -1 +1 @@`), which is the
/// case a regex written from memory usually gets wrong.
fn parse_range(field: &str) -> Option<u32> {
    let digits = field.trim_start_matches(['-', '+']);
    let start = digits.split(',').next()?;
    start.parse().ok()
}

/// Parse one file's unified diff.
///
/// Everything before the first `@@` is header — `diff --git`, `index`, mode
/// changes, `---`/`+++` — and is skipped rather than mistaken for content. That
/// matters more than it looks: a removed line reading `--- a/x` inside a patch
/// file is content, and only the position relative to the first `@@` tells them
/// apart.
pub fn parse_unified(patch: &str) -> (Vec<Hunk>, bool, u32, u32, bool) {
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut binary = false;
    let mut insertions = 0;
    let mut deletions = 0;
    let mut truncated = false;
    let mut emitted = 0usize;

    let mut old_line = 0u32;
    let mut new_line = 0u32;

    for raw in patch.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);

        if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
            binary = true;
            continue;
        }

        if let Some(rest) = line.strip_prefix("@@ ") {
            let Some((ranges, heading)) = rest.split_once("@@") else {
                continue;
            };
            let mut fields = ranges.split_whitespace();
            let old = fields.next().and_then(parse_range).unwrap_or(1);
            let new = fields.next().and_then(parse_range).unwrap_or(1);
            old_line = old;
            new_line = new;
            let heading = heading.trim();
            hunks.push(Hunk {
                old_start: old,
                new_start: new,
                heading: (!heading.is_empty()).then(|| heading.to_string()),
                lines: Vec::new(),
            });
            continue;
        }

        let Some(hunk) = hunks.last_mut() else {
            continue; // still in the header
        };

        // Applies to the line already emitted, not to a new one.
        if line.starts_with('\\') {
            if let Some(last) = hunk.lines.last_mut() {
                last.no_newline = true;
            }
            continue;
        }

        if emitted >= MAX_DIFF_LINES {
            truncated = true;
            continue;
        }

        let (kind, text) = match line.as_bytes().first() {
            Some(b'+') => (LineKind::Added, &line[1..]),
            Some(b'-') => (LineKind::Removed, &line[1..]),
            Some(b' ') => (LineKind::Context, &line[1..]),
            // Git writes a context line that is empty as an empty string
            // rather than a single space. Treating it as a header would drop a
            // blank line out of the middle of the diff and shift every number
            // after it by one.
            None => (LineKind::Context, ""),
            _ => continue,
        };

        let (old_no, new_no) = match kind {
            LineKind::Added => {
                insertions += 1;
                let n = new_line;
                new_line += 1;
                (None, Some(n))
            }
            LineKind::Removed => {
                deletions += 1;
                let n = old_line;
                old_line += 1;
                (Some(n), None)
            }
            LineKind::Context => {
                let (o, n) = (old_line, new_line);
                old_line += 1;
                new_line += 1;
                (Some(o), Some(n))
            }
        };

        hunk.lines.push(DiffLine {
            kind,
            old_line: old_no,
            new_line: new_no,
            text: text.to_string(),
            no_newline: false,
        });
        emitted += 1;
    }

    (hunks, binary, insertions, deletions, truncated)
}

/// Present a file Git has never seen as one hunk of added lines.
///
/// Synthesised rather than obtained from `git diff --no-index /dev/null <path>`
/// so that an untracked file behaves identically on every platform and cannot
/// fail on Windows' lack of `/dev/null`. It is a strictly mechanical rendering
/// — every line, added, numbered from one — so there is nothing for Git to
/// disagree with.
pub fn added_file(path: &str, text: &str) -> FileDiff {
    let body = text.strip_suffix('\n').unwrap_or(text);
    let mut lines = Vec::new();
    let mut truncated = false;

    // `"".split('\n')` yields one empty item, which would render an empty file
    // as a file containing one blank line.
    let source = if body.is_empty() {
        Vec::new()
    } else {
        body.split('\n').collect()
    };

    for (index, line) in source.into_iter().enumerate() {
        if index >= MAX_DIFF_LINES {
            truncated = true;
            break;
        }
        lines.push(DiffLine {
            kind: LineKind::Added,
            old_line: None,
            new_line: Some(index as u32 + 1),
            text: line.strip_suffix('\r').unwrap_or(line).to_string(),
            no_newline: false,
        });
    }

    let insertions = lines.len() as u32;
    // An empty file is a real change — it appeared — but it has no lines, and a
    // hunk with no lines renders as an empty box. Say it with the counts alone.
    let hunks = if lines.is_empty() {
        Vec::new()
    } else {
        vec![Hunk {
            old_start: 0,
            new_start: 1,
            heading: None,
            lines,
        }]
    };

    FileDiff {
        path: path.to_string(),
        from_path: None,
        kind: ChangeKind::Untracked,
        binary: false,
        too_large: false,
        insertions,
        deletions: 0,
        hunks,
        truncated,
    }
}

/// Ask Git for one file's diff against `HEAD`, working tree and index together.
///
/// `HEAD` rather than the index because the question Review answers is "what is
/// different from the last commit" — whether the agent happened to stage its
/// work is not something the reader should have to think about.
///
/// `from_path` is the file's previous name when it was renamed, and passing it
/// is not optional decoration. Rename detection needs to see **both** sides:
/// with a pathspec naming only the new path, `-M` has nothing to pair it with
/// and Git reports `new file` with every line added. Verified against Git 2.55
/// — a five-line file renamed with one line edited came back as five additions
/// instead of one change, which would tell a reviewer that an agent rewrote a
/// file it had merely moved.
pub fn against_head(root: &Path, path: &str, from_path: Option<&str>) -> Option<String> {
    let mut args = vec![
        "diff",
        "--no-color",
        "--no-ext-diff",
        "-M", // detect renames, so a moved file is not a delete plus an add
        "-U3",
        "HEAD",
        "--",
        path,
    ];
    if let Some(from) = from_path {
        args.push(from);
    }
    super::run(root, &args).ok()
}
