//! Tests for Review.
//!
//! The Git-facing ones run against a **real repository** built by the `git`
//! binary this product actually shells out to (§80/D5). The two porcelain
//! orderings pinned here — `status -z` puts a rename's new path first, `diff
//! --numstat -z` puts the old path first — are the kind of detail that a
//! parser written from memory gets backwards, and getting them backwards
//! attributes a rename's line counts to a file that no longer exists.

use super::*;
use crate::git::diff::{parse_unified, LineKind, MAX_DIFF_LINES};
use crate::git::status::parse_status;

/// A real repository with one commit, ready to be dirtied.
fn repo() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    git::run(&root, &["init", "--initial-branch=main"]).unwrap();
    git::run(&root, &["config", "user.email", "test@example.com"]).unwrap();
    git::run(&root, &["config", "user.name", "Test"]).unwrap();
    // Local to this repository, so the test is not at the mercy of the
    // machine's global `core.autocrlf`.
    git::run(&root, &["config", "core.autocrlf", "false"]).unwrap();
    (dir, root)
}

fn commit(root: &Path, message: &str) {
    git::run(root, &["add", "-A"]).unwrap();
    git::run(root, &["commit", "-m", message]).unwrap();
}

#[test]
fn a_repository_with_no_commits_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    git::run(&root, &["init", "--initial-branch=main"]).unwrap();
    std::fs::write(root.join("a.txt"), "hello\n").unwrap();

    assert!(git::locate(&root).is_some());
    assert!(!git::status::has_commits(&root));
}

#[test]
fn finds_the_repository_root_from_a_subdirectory() {
    let (_guard, root) = repo();
    std::fs::create_dir_all(root.join("apps/web")).unwrap();
    std::fs::write(root.join("apps/web/index.ts"), "export {}\n").unwrap();
    commit(&root, "initial");

    let location = git::locate(&root.join("apps/web")).unwrap();
    assert_eq!(location.prefix, "apps/web/");
    assert_eq!(
        location.to_project("apps/web/index.ts").as_deref(),
        Some("index.ts")
    );
    // A change elsewhere in the repository is not this project's business.
    assert_eq!(location.to_project("other/thing.ts"), None);
    assert_eq!(location.to_repo("index.ts"), "apps/web/index.ts");
}

#[test]
fn reads_status_from_a_real_working_tree() {
    let (_guard, root) = repo();
    std::fs::write(root.join("kept.txt"), "one\n").unwrap();
    std::fs::write(root.join("gone.txt"), "two\n").unwrap();
    std::fs::write(root.join("moved.txt"), "three\n").unwrap();
    commit(&root, "initial");

    std::fs::write(root.join("kept.txt"), "one\nchanged\n").unwrap();
    std::fs::remove_file(root.join("gone.txt")).unwrap();
    git::run(&root, &["mv", "moved.txt", "elsewhere.txt"]).unwrap();
    std::fs::write(root.join("fresh.txt"), "new\n").unwrap();

    let changes = git::status::changed_files(&root);
    let find = |path: &str| {
        changes
            .iter()
            .find(|c| c.path == path)
            .unwrap_or_else(|| panic!("{path} missing from {changes:?}"))
    };

    assert_eq!(find("kept.txt").kind, ChangeKind::Modified);
    assert_eq!(find("gone.txt").kind, ChangeKind::Deleted);
    assert_eq!(find("fresh.txt").kind, ChangeKind::Untracked);

    // The rename, and the ordering that makes it readable.
    let renamed = find("elsewhere.txt");
    assert_eq!(renamed.kind, ChangeKind::Renamed);
    assert_eq!(renamed.from_path.as_deref(), Some("moved.txt"));
}

#[test]
fn a_wholly_untracked_directory_is_listed_file_by_file() {
    // Git's default `--untracked-files=normal` collapses a new directory into a
    // single record ending in `/`. That record has no filename to show, no line
    // count, and nothing to diff — it rendered as a blank row in the real app.
    let (_guard, root) = repo();
    std::fs::write(root.join("kept.txt"), "one\n").unwrap();
    commit(&root, "initial");

    std::fs::create_dir_all(root.join("assets/deep")).unwrap();
    std::fs::write(root.join("assets/a.txt"), "a\n").unwrap();
    std::fs::write(root.join("assets/deep/b.txt"), "b\n").unwrap();

    let paths: Vec<String> = git::status::changed_files(&root)
        .into_iter()
        .map(|c| c.path)
        .collect();

    assert!(paths.contains(&"assets/a.txt".to_string()), "got {paths:?}");
    assert!(paths.contains(&"assets/deep/b.txt".to_string()), "got {paths:?}");
    assert!(
        !paths.iter().any(|p| p.ends_with('/')),
        "no record should name a directory: {paths:?}"
    );
}

#[test]
fn a_rename_in_porcelain_z_lists_the_new_path_first() {
    // Pinned against real Git output. The human format prints
    // `orig -> new`; `-z` reverses the pair, and a parser that follows the
    // documentation for the wrong format silently swaps every rename.
    let changes = parse_status("R  new/path.rs\0old/path.rs\0 M other.rs\0");
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].path, "new/path.rs");
    assert_eq!(changes[0].from_path.as_deref(), Some("old/path.rs"));
    assert_eq!(changes[1].path, "other.rs");
    assert_eq!(changes[1].from_path, None);
}

#[test]
fn a_rename_in_numstat_z_lists_the_old_path_first() {
    // The opposite order to `status -z`, for the same rename. Verified against
    // Git 2.55: the record's own path field is empty and the two paths follow.
    let counts = parse_numstat("3\t1\tplain.rs\00\t0\t\0old.rs\0new.rs\0-\t-\timage.png\0");

    assert_eq!(counts.get("plain.rs"), Some(&(3, 1, false)));
    // Keyed on the new path, because that is what `status` reports and what
    // the join is done on.
    assert_eq!(counts.get("new.rs"), Some(&(0, 0, false)));
    assert_eq!(counts.get("old.rs"), None);
    // A binary file reports `-` for both counts and must not read as zero
    // changes.
    assert_eq!(counts.get("image.png"), Some(&(0, 0, true)));
}

#[test]
fn parses_a_real_diff_into_numbered_lines() {
    let (_guard, root) = repo();
    std::fs::write(root.join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
    commit(&root, "initial");
    std::fs::write(root.join("a.txt"), "one\nTWO\nthree\nfour\nfive\n").unwrap();

    let patch = git::diff::against_head(&root, "a.txt", None).unwrap();
    let (hunks, binary, insertions, deletions, truncated) = parse_unified(&patch);

    assert!(!binary);
    assert!(!truncated);
    assert_eq!((insertions, deletions), (2, 1));
    assert_eq!(hunks.len(), 1);

    let removed: Vec<&DiffLineForTest> = Vec::new();
    let _ = removed; // keep the shape obvious below

    let lines = &hunks[0].lines;
    let changed = lines
        .iter()
        .find(|l| l.kind == LineKind::Removed)
        .expect("a removed line");
    assert_eq!(changed.text, "two");
    // A removed line has a left-hand number and no right-hand one; getting
    // this wrong is what makes a diff unreadable.
    assert_eq!(changed.old_line, Some(2));
    assert_eq!(changed.new_line, None);

    let added = lines
        .iter()
        .find(|l| l.kind == LineKind::Added && l.text == "TWO")
        .expect("the replacement line");
    assert_eq!(added.old_line, None);
    assert_eq!(added.new_line, Some(2));

    let context = lines
        .iter()
        .find(|l| l.text == "three")
        .expect("a context line");
    assert_eq!((context.old_line, context.new_line), (Some(3), Some(3)));
}

// Only used to make the assertion above read clearly.
type DiffLineForTest = crate::git::diff::DiffLine;

#[test]
fn a_blank_context_line_keeps_the_numbering_straight() {
    // Git writes an unchanged empty line as an empty string, not as a single
    // space. Skipping it would shift every line number after it by one.
    let patch = "\
diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1,4 +1,4 @@
 one

-three
+THREE
";
    let (hunks, _, _, _, _) = parse_unified(patch);
    let lines = &hunks[0].lines;

    assert_eq!(lines[1].kind, LineKind::Context);
    assert_eq!(lines[1].text, "");
    assert_eq!(lines[2].old_line, Some(3));
    assert_eq!(lines[3].new_line, Some(3));
}

#[test]
fn a_single_line_hunk_header_has_no_count() {
    // `@@ -1 +1 @@` is legal and omits the comma entirely.
    let patch = "--- a/a\n+++ b/a\n@@ -7 +9 @@\n-old\n+new\n";
    let (hunks, _, insertions, deletions, _) = parse_unified(patch);
    assert_eq!(hunks[0].old_start, 7);
    assert_eq!(hunks[0].new_start, 9);
    assert_eq!(hunks[0].lines[0].old_line, Some(7));
    assert_eq!(hunks[0].lines[1].new_line, Some(9));
    assert_eq!((insertions, deletions), (1, 1));
}

#[test]
fn diff_header_lines_are_never_mistaken_for_content() {
    // The removed line here *is* `--- a/x`: it is the content of a patch file
    // being edited. Only its position after the first `@@` tells it apart from
    // the real header above.
    let patch = "\
diff --git a/p.patch b/p.patch
index 111..222 100644
--- a/p.patch
+++ b/p.patch
@@ -1,2 +1,2 @@
---- a/x
++++ b/x
";
    let (hunks, _, insertions, deletions, _) = parse_unified(patch);
    assert_eq!(hunks.len(), 1);
    assert_eq!((insertions, deletions), (1, 1));
    assert_eq!(hunks[0].lines[0].text, "--- a/x");
    assert_eq!(hunks[0].lines[1].text, "+++ b/x");
}

#[test]
fn a_missing_trailing_newline_is_marked_on_the_line_it_belongs_to() {
    let patch = "--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n";
    let (hunks, _, _, _, _) = parse_unified(patch);
    assert!(hunks[0].lines[0].no_newline);
    assert!(!hunks[0].lines[1].no_newline);
}

#[test]
fn a_renamed_file_needs_both_of_its_names_to_diff_as_a_rename() {
    // The bug this pins: with a pathspec naming only the new path, `-M` has
    // nothing to pair against and Git reports the move as a brand-new file with
    // every line added. A reviewer would be told an agent rewrote a file it had
    // only moved. Both orders verified against real Git 2.55.
    let (_guard, root) = repo();
    std::fs::write(root.join("old.txt"), "alpha\nbeta\ngamma\ndelta\nepsilon\n").unwrap();
    commit(&root, "initial");

    git::run(&root, &["mv", "old.txt", "new.txt"]).unwrap();
    std::fs::write(root.join("new.txt"), "alpha\nBETA\ngamma\ndelta\nepsilon\n").unwrap();

    // Only the new name: the rename is lost.
    let patch = git::diff::against_head(&root, "new.txt", None).unwrap();
    let (_, _, insertions, deletions, _) = parse_unified(&patch);
    assert_eq!(
        (insertions, deletions),
        (5, 0),
        "expected the one-sided pathspec to look like a whole new file"
    );

    // Both names: one changed line, as it actually is.
    let patch = git::diff::against_head(&root, "new.txt", Some("old.txt")).unwrap();
    let (hunks, _, insertions, deletions, _) = parse_unified(&patch);
    assert_eq!((insertions, deletions), (1, 1));
    assert!(hunks[0]
        .lines
        .iter()
        .any(|l| l.kind == LineKind::Added && l.text == "BETA"));
}

#[test]
fn a_binary_diff_is_reported_as_binary_not_as_empty() {
    let (_guard, root) = repo();
    std::fs::write(root.join("blob.bin"), [0u8, 1, 2, 3]).unwrap();
    commit(&root, "initial");
    std::fs::write(root.join("blob.bin"), [0u8, 9, 9, 9, 9]).unwrap();

    let patch = git::diff::against_head(&root, "blob.bin", None).unwrap();
    let (hunks, binary, _, _, _) = parse_unified(&patch);
    assert!(binary, "expected a binary diff, got: {patch:?}");
    assert!(hunks.is_empty());
}

#[test]
fn an_untracked_file_becomes_a_diff_of_added_lines() {
    let diff = git::diff::added_file("new.rs", "fn main() {}\nprintln!();\n");
    assert_eq!(diff.kind, ChangeKind::Untracked);
    assert_eq!(diff.insertions, 2);
    assert_eq!(diff.deletions, 0);
    assert_eq!(diff.hunks.len(), 1);
    assert_eq!(diff.hunks[0].new_start, 1);
    assert_eq!(diff.hunks[0].lines[1].new_line, Some(2));
    assert!(diff.hunks[0].lines.iter().all(|l| l.old_line.is_none()));
}

#[test]
fn an_empty_new_file_produces_no_hunk_rather_than_an_empty_one() {
    let diff = git::diff::added_file("empty.txt", "");
    assert_eq!(diff.insertions, 0);
    assert!(diff.hunks.is_empty());
}

#[test]
fn a_very_large_diff_is_cut_and_says_so() {
    let body: String = (0..MAX_DIFF_LINES + 500)
        .map(|i| format!("line {i}\n"))
        .collect();
    let diff = git::diff::added_file("big.txt", &body);

    assert!(diff.truncated);
    assert_eq!(diff.hunks[0].lines.len(), MAX_DIFF_LINES);
}

#[test]
fn a_recorded_change_is_matched_through_the_session_working_directory() {
    // `file_changes.path` is relative to the session's cwd and spelled with
    // backslashes on Windows, which is how Claude Code writes `trackingPath`.
    // Joining it against Git's forward-slash paths without folding the cwd in
    // matches nothing, and looks exactly like "no agent touched this file".
    let root = Path::new(r"C:\proj");
    let cwd = Path::new(r"C:\proj");
    assert_eq!(
        project_relative(root, cwd, r"src\game\types.ts").as_deref(),
        Some("src/game/types.ts")
    );

    // A session started above the project: the recorded path carries the
    // project folder in it.
    let cwd = Path::new(r"C:\");
    assert_eq!(
        project_relative(root, cwd, r"proj\src\main.rs").as_deref(),
        Some("src/main.rs")
    );

    // And one that touched something outside the project entirely.
    assert_eq!(project_relative(root, cwd, r"other\file.rs"), None);
}
