//! Tests for the project filesystem.
//!
//! Everything runs against a real temporary directory. The confinement tests in
//! particular are worth nothing against a mock: what makes them hard is how the
//! actual filesystem behaves — canonicalisation, case folding, symlinks — and a
//! fake would agree with whatever we assumed.

use super::*;

/// A directory whose path is canonical to begin with.
///
/// On Windows the system temp directory is normally under `C:\Users\<name>\
/// AppData\Local\Temp`, which is fine — but on some machines it is reached
/// through a short (8.3) path, and then `root.join(x).canonicalize()` and
/// `root` disagree for reasons that have nothing to do with the code under
/// test. Canonicalising the root up front removes that from the picture.
fn tempdir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    (dir, root)
}

#[test]
fn resolves_a_path_inside_the_project() {
    let (_guard, root) = tempdir();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();

    let resolved = resolve(&root, "src/main.rs").unwrap();
    assert_eq!(std::fs::read_to_string(resolved).unwrap(), "fn main() {}");
}

#[test]
fn resolves_a_file_that_does_not_exist_yet() {
    // Saving a new file has to work, and it is the case where the filesystem
    // cannot help — only the component check stands between us and `..`.
    let (_guard, root) = tempdir();
    let resolved = resolve(&root, "notes/new.md").unwrap();
    assert!(resolved.starts_with(&root));
}

#[test]
fn refuses_to_climb_out_of_the_project() {
    let (_guard, root) = tempdir();
    std::fs::create_dir_all(root.join("src")).unwrap();

    for attempt in [
        "../secrets.txt",
        "src/../../secrets.txt",
        "..",
        "src/..\\..\\secrets.txt",
    ] {
        assert!(
            matches!(resolve(&root, attempt), Err(FileError::Outside)),
            "`{attempt}` should have been refused"
        );
    }
}

#[test]
fn refuses_an_absolute_path() {
    let (_guard, root) = tempdir();

    // A drive letter, a bare root, and a UNC share: three different
    // `Component` variants, all of which must be rejected.
    for attempt in [
        r"C:\Windows\System32\drivers\etc\hosts",
        "/etc/passwd",
        r"\\server\share\file",
        r"\Windows",
    ] {
        assert!(
            matches!(resolve(&root, attempt), Err(FileError::Outside)),
            "`{attempt}` should have been refused"
        );
    }
}

#[test]
fn a_sibling_directory_with_a_shared_prefix_is_not_inside() {
    // `C:\demo-evil` starts with the *string* `C:\demo`. A prefix check on
    // strings would let it through; the component-wise one does not.
    let (_guard, base) = tempdir();
    let root = base.join("demo");
    let sibling = base.join("demo-evil");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();

    assert!(!contains(&root, &sibling));
    assert!(contains(&root, &root.join("a/b")));
}

#[cfg(windows)]
#[test]
fn comparison_is_case_insensitive_on_windows() {
    // These are the same directory on this platform, and a case-sensitive
    // check would reject a perfectly legitimate path.
    let root = Path::new(r"C:\Users\Someone\Project");
    let child = Path::new(r"c:\users\someone\project\src\main.rs");
    assert!(contains(root, child));
}

#[test]
fn a_verbatim_path_compares_equal_to_a_plain_one() {
    // `canonicalize` returns `\\?\C:\...` on Windows. Comparing that against a
    // plain root fails for a reason that has nothing to do with containment.
    let plain = Path::new(r"C:\Users\Someone\Project");
    let verbatim = Path::new(r"\\?\C:\Users\Someone\Project\src");
    assert!(contains(plain, verbatim));
}

#[test]
fn lists_a_directory_with_folders_first() {
    let (_guard, root) = tempdir();
    std::fs::create_dir_all(root.join("zzz-dir")).unwrap();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join("Alpha.txt"), "a").unwrap();
    std::fs::write(root.join("beta.txt"), "bb").unwrap();

    let entries = list_dir(&root, "").unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();

    // Directories first, then case-insensitive alphabetical — and `.git` is
    // never offered, because browsing into it is a way to break a repository.
    assert_eq!(names, vec!["zzz-dir", "Alpha.txt", "beta.txt"]);
    assert_eq!(entries[1].size, Some(1));
    assert_eq!(entries[2].size, Some(2));
    assert_eq!(entries[0].size, None);
}

#[test]
fn marks_git_ignored_entries_in_a_real_repository() {
    let (_guard, root) = tempdir();
    crate::git::run(&root, &["init", "--initial-branch=main"]).unwrap();
    std::fs::write(root.join(".gitignore"), "node_modules/\n*.log\n").unwrap();
    std::fs::create_dir_all(root.join("node_modules")).unwrap();
    std::fs::write(root.join("app.log"), "noise").unwrap();
    std::fs::write(root.join("app.rs"), "code").unwrap();

    let entries = list_dir(&root, "").unwrap();
    let ignored = |name: &str| {
        entries
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name} missing"))
            .ignored
    };

    assert!(ignored("node_modules"));
    assert!(ignored("app.log"));
    assert!(!ignored("app.rs"));
    // Ignored files are still listed. Hiding a file the user can see in
    // Explorer is more confusing than dimming it.
    assert_eq!(entries.len(), 4);
}

#[test]
fn reads_a_text_file_and_normalises_line_endings() {
    let (_guard, root) = tempdir();
    std::fs::write(root.join("crlf.txt"), "one\r\ntwo\r\n").unwrap();

    let contents = read(&root, "crlf.txt").unwrap();
    assert_eq!(contents.text.as_deref(), Some("one\ntwo\n"));
    assert!(contents.crlf);
    assert!(contents.trailing_newline);
    assert!(contents.unreadable.is_none());
}

#[test]
fn a_round_trip_leaves_a_crlf_file_byte_identical() {
    // Opening a file and saving it unchanged must not produce a diff. This is
    // the difference between an editor people trust in a repository and one
    // that rewrites every line ending the first time it is used.
    let (_guard, root) = tempdir();
    let original = "alpha\r\nbeta\r\n";
    std::fs::write(root.join("f.txt"), original).unwrap();

    let contents = read(&root, "f.txt").unwrap();
    write(
        &root,
        "f.txt",
        contents.text.as_deref().unwrap(),
        contents.crlf,
        contents.trailing_newline,
        None,
    )
    .unwrap();

    assert_eq!(std::fs::read(root.join("f.txt")).unwrap(), original.as_bytes());
}

#[test]
fn a_round_trip_preserves_a_missing_trailing_newline() {
    let (_guard, root) = tempdir();
    let original = "no newline at the end";
    std::fs::write(root.join("f.txt"), original).unwrap();

    let contents = read(&root, "f.txt").unwrap();
    write(
        &root,
        "f.txt",
        contents.text.as_deref().unwrap(),
        contents.crlf,
        contents.trailing_newline,
        None,
    )
    .unwrap();

    assert_eq!(std::fs::read(root.join("f.txt")).unwrap(), original.as_bytes());
}

#[test]
fn refuses_to_render_a_binary_file_as_text() {
    let (_guard, root) = tempdir();
    std::fs::write(root.join("icon.png"), [0x89, b'P', b'N', b'G', 0x00, 0x1a]).unwrap();

    let contents = read(&root, "icon.png").unwrap();
    assert_eq!(contents.unreadable, Some(Unreadable::Binary));
    assert!(contents.text.is_none());
}

#[test]
fn says_when_a_file_is_too_large_rather_than_opening_it() {
    let (_guard, root) = tempdir();
    let big = vec![b'x'; (MAX_EDITABLE_BYTES + 1) as usize];
    std::fs::write(root.join("huge.txt"), &big).unwrap();

    let contents = read(&root, "huge.txt").unwrap();
    assert_eq!(contents.unreadable, Some(Unreadable::TooLarge));
    assert!(contents.text.is_none());
    assert_eq!(contents.size, MAX_EDITABLE_BYTES + 1);
}

#[test]
fn a_save_refuses_when_the_file_changed_underneath_it() {
    // The case this exists for: an agent is editing the same file in another
    // tab. Overwriting blind would delete its work with nothing left to show
    // it ever happened.
    let (_guard, root) = tempdir();
    std::fs::write(root.join("f.txt"), "original\n").unwrap();
    let opened = read(&root, "f.txt").unwrap();

    // Something else writes it. The sleep is not decoration: filesystem
    // timestamps have limited resolution, and two writes in the same
    // millisecond would be indistinguishable — which is the one case this
    // check genuinely cannot catch.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(root.join("f.txt"), "written by somebody else\n").unwrap();

    let outcome = write(&root, "f.txt", "mine\n", false, true, opened.modified_ms).unwrap();
    assert!(matches!(outcome, WriteOutcome::Stale { .. }));
    assert_eq!(
        std::fs::read_to_string(root.join("f.txt")).unwrap(),
        "written by somebody else\n",
        "a refused save must not have written anything"
    );

    // Saving again without an expectation is the "save anyway" the interface
    // offers once it has explained itself.
    let outcome = write(&root, "f.txt", "mine\n", false, true, None).unwrap();
    assert!(matches!(outcome, WriteOutcome::Written { .. }));
    assert_eq!(std::fs::read_to_string(root.join("f.txt")).unwrap(), "mine\n");
}

#[test]
fn an_untouched_file_saves_normally_and_reports_its_new_time() {
    let (_guard, root) = tempdir();
    std::fs::write(root.join("f.txt"), "original\n").unwrap();
    let opened = read(&root, "f.txt").unwrap();

    let outcome = write(&root, "f.txt", "edited\n", false, true, opened.modified_ms).unwrap();
    let WriteOutcome::Written { modified_ms } = outcome else {
        panic!("expected the save to go through, got {outcome:?}");
    };
    assert!(modified_ms.is_some());
    assert_eq!(std::fs::read_to_string(root.join("f.txt")).unwrap(), "edited\n");
}

#[test]
fn saving_a_file_that_was_deleted_writes_it_back() {
    // Not a conflict: there is nothing to lose, and the user is asking for the
    // file to exist again.
    let (_guard, root) = tempdir();
    std::fs::write(root.join("f.txt"), "original\n").unwrap();
    let opened = read(&root, "f.txt").unwrap();
    std::fs::remove_file(root.join("f.txt")).unwrap();

    let outcome = write(&root, "f.txt", "back\n", false, true, opened.modified_ms).unwrap();
    assert!(matches!(outcome, WriteOutcome::Written { .. }));
    assert_eq!(std::fs::read_to_string(root.join("f.txt")).unwrap(), "back\n");
}

#[test]
fn writing_creates_missing_parent_directories() {
    let (_guard, root) = tempdir();
    write(&root, "a/b/c/new.txt", "hello", false, true, None).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("a/b/c/new.txt")).unwrap(),
        "hello\n"
    );
}

#[test]
fn writing_outside_the_project_is_refused_before_anything_is_created() {
    let (_guard, base) = tempdir();
    let root = base.join("project");
    std::fs::create_dir_all(&root).unwrap();

    assert!(matches!(
        write(&root, "../escaped.txt", "x", false, true, None),
        Err(FileError::Outside)
    ));
    assert!(!base.join("escaped.txt").exists());
}

/// A symlink inside the project that points outside it is the one case the
/// component check cannot see, and the only reason the resolved path is
/// checked against the filesystem as well.
///
/// Creating a symlink on Windows needs Developer Mode or elevation, so the
/// test reports honestly rather than failing on a machine that cannot make
/// one — a skipped test that says so beats a green one that tested nothing.
#[test]
fn a_symlink_escaping_the_project_is_refused() {
    let (_guard, base) = tempdir();
    let root = base.join("project");
    let outside = base.join("outside");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "top secret").unwrap();

    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(&outside, root.join("link")).is_ok();
    #[cfg(not(windows))]
    let made = std::os::unix::fs::symlink(&outside, root.join("link")).is_ok();

    if !made {
        eprintln!("skipped: this machine cannot create symlinks (Developer Mode off)");
        return;
    }

    assert!(matches!(
        resolve(&root, "link/secret.txt"),
        Err(FileError::Outside)
    ));
    // And the same for a file that does not exist yet, which resolves through
    // the link's parent rather than the link's target.
    assert!(matches!(
        resolve(&root, "link/planted.txt"),
        Err(FileError::Outside)
    ));
}
