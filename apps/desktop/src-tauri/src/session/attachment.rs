//! Images pasted into a session (§22).
//!
//! An agent CLI reads a terminal, and a terminal carries bytes. There is no
//! way to hand a picture *through* a PTY — so what actually reaches the agent
//! is a **path**, written to disk first and then typed at the prompt like
//! anything else. Claude Code reads an image file when a path points at one;
//! that is the whole mechanism, and it is worth being plain about it rather
//! than implying the terminal grew a new capability.
//!
//! ## Where the file goes, and why not the project
//!
//! Into the session's own log directory, beside the guardrail snapshot and
//! the project brief — the same choice D23 made for the brief, for the same
//! reason. A screenshot dropped into the user's repository would show up in
//! `git status`, in the Review surface this product also ships, and quite
//! possibly in their next commit. Pasting a picture must not dirty a working
//! tree.
//!
//! ## What is not done here
//!
//! No re-encoding, no thumbnailing, no format conversion. The bytes the
//! clipboard produced are the bytes written. Re-encoding a screenshot to save
//! a few hundred kilobytes would change what the agent is shown, which is the
//! one thing this must not do.

use std::path::PathBuf;

use serde::Serialize;

/// Largest image accepted from a paste.
///
/// Generous for a screenshot — a lossless 4K capture lands around 10 MB — and
/// still a bound rather than "whatever the clipboard had". The failure above
/// it is a sentence, not a hang (§81).
pub const MAX_BYTES: usize = 20 * 1024 * 1024;

/// What a pasted image turned into.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    /// Absolute path on disk, which is what gets typed into the prompt.
    pub path: String,
    /// Name only, for the chip the surface shows.
    pub name: String,
    pub bytes: usize,
    /// `image/png`, `image/jpeg`, … — carried so the preview can build a
    /// `data:` URL without sniffing the file a second time.
    pub mime: String,
}

/// Image formats a paste is allowed to produce.
///
/// A closed list, matched on the **bytes** rather than on a MIME type the
/// webview supplied: the renderer naming its own content type is the renderer
/// choosing what lands on disk. Sniffing is the check.
fn sniff(data: &[u8]) -> Option<(&'static str, &'static str)> {
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    const GIF87: &[u8] = b"GIF87a";
    const GIF89: &[u8] = b"GIF89a";

    if data.starts_with(PNG) {
        return Some(("png", "image/png"));
    }
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(("jpg", "image/jpeg"));
    }
    if data.starts_with(GIF87) || data.starts_with(GIF89) {
        return Some(("gif", "image/gif"));
    }
    // RIFF....WEBP — the size field sits between the two markers.
    if data.len() > 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        return Some(("webp", "image/webp"));
    }
    // BMP is what Windows' own clipboard hands over for a plain screenshot,
    // so leaving it out would fail on the single most common way to paste one.
    if data.starts_with(b"BM") {
        return Some(("bmp", "image/bmp"));
    }
    None
}

/// Write pasted image bytes into a session's own directory.
///
/// `log_dir` is the session's log directory — the caller reads it from the
/// database, so the webview never names a directory and cannot write outside
/// one (§3, the same boundary `files::resolve` exists for).
pub fn save(log_dir: &str, data: &[u8]) -> Result<Attachment, String> {
    if data.is_empty() {
        return Err("attachment.empty".into());
    }
    if data.len() > MAX_BYTES {
        return Err("attachment.tooLarge".into());
    }
    let Some((extension, mime)) = sniff(data) else {
        return Err("attachment.unsupported".into());
    };

    let dir = PathBuf::from(log_dir).join("attachments");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    // The **whole** id, not a prefix of it.
    //
    // The first attempt used `&id[..8]`, which reads as a reasonable short
    // name and is not: UUIDv7's leading hex digits *are* a millisecond
    // timestamp, so any two pastes inside the same millisecond produce the
    // identical prefix and the second silently overwrites the first. Caught by
    // `two_pastes_produce_two_files`, which is fast enough to land both in one
    // millisecond — the truncation is only unique as long as nobody is quick.
    let id = uuid::Uuid::now_v7().to_string();
    let name = format!("pasted-{id}.{extension}");
    let path = dir.join(&name);
    std::fs::write(&path, data).map_err(|e| e.to_string())?;

    Ok(Attachment {
        path: path.to_string_lossy().into_owned(),
        name,
        bytes: data.len(),
        mime: mime.into(),
    })
}

/// Take an image off the system clipboard, as PNG bytes.
///
/// **The webview cannot do this**, which is the whole reason this exists in
/// the core. WebView2 delivers a `paste` event for text and does not raise one
/// at all for image data — verified by instrumenting the real app: the Ctrl+V
/// `keydown` arrives at xterm's textarea and no `paste` event ever follows, so
/// `clipboardData.items` is never reached and there is nothing to read. Every
/// browser-shaped approach fails the same way for the same reason.
///
/// `arboard` reads the platform clipboard directly, which on Windows hands
/// back a raw RGBA buffer rather than a file in any particular format. It is
/// encoded as PNG here — lossless, so nothing the person pasted is lost, and
/// it means the file on disk is one an agent can actually open.
pub fn from_clipboard() -> Result<Vec<u8>, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    // Not an error worth a scary sentence: a paste with no image on the
    // clipboard is an ordinary text paste, and the caller falls back to
    // letting the terminal handle it.
    let image = clipboard.get_image().map_err(|_| "attachment.noImage".to_string())?;

    let width = u32::try_from(image.width).map_err(|_| "attachment.unsupported".to_string())?;
    let height = u32::try_from(image.height).map_err(|_| "attachment.unsupported".to_string())?;
    let buffer = image::RgbaImage::from_raw(width, height, image.bytes.into_owned())
        .ok_or_else(|| "attachment.unsupported".to_string())?;

    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(buffer)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(png)
}

/// Read an attachment back, for the preview.
///
/// Confined to the session's own `attachments` directory: the webview passes
/// back the path it was given, and a path that does not live there is refused
/// rather than read. Without this check the preview would be an arbitrary file
/// reader with a friendly name — exactly what `files::resolve`'s own doc
/// comment warns about.
pub fn read(log_dir: &str, path: &str) -> Result<Vec<u8>, String> {
    let root = PathBuf::from(log_dir).join("attachments");
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let target = PathBuf::from(path).canonicalize().map_err(|e| e.to_string())?;
    if !target.starts_with(&root) {
        return Err("attachment.outsideSession".into());
    }
    std::fs::read(&target).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real 1x1 PNG, byte for byte — not a stub with the right first eight
    /// bytes, so the file on disk is one an image decoder would actually open.
    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn a_pasted_png_lands_on_disk_with_its_bytes_intact() {
        let dir = tempfile::tempdir().unwrap();
        let saved = save(&dir.path().to_string_lossy(), PNG_1X1).unwrap();

        assert_eq!(saved.mime, "image/png");
        assert_eq!(saved.bytes, PNG_1X1.len());
        assert!(saved.name.ends_with(".png"));
        // The bytes the agent will be shown must be the bytes that were
        // pasted — nothing here re-encodes, and this is what pins that.
        assert_eq!(std::fs::read(&saved.path).unwrap(), PNG_1X1);
    }

    /// The format Windows' own clipboard produces for a plain screenshot.
    /// Leaving it out would fail the single most common way to paste one.
    #[test]
    fn a_windows_clipboard_bitmap_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let mut bmp = b"BM".to_vec();
        bmp.extend_from_slice(&[0u8; 64]);
        let saved = save(&dir.path().to_string_lossy(), &bmp).unwrap();
        assert_eq!(saved.mime, "image/bmp");
    }

    /// The type is decided by the bytes, never by anything the webview says.
    #[test]
    fn something_that_is_not_an_image_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let err = save(&dir.path().to_string_lossy(), b"#!/bin/sh\nrm -rf /\n").unwrap_err();
        assert_eq!(err, "attachment.unsupported");
    }

    #[test]
    fn an_oversized_paste_is_refused_rather_than_written() {
        let dir = tempfile::tempdir().unwrap();
        let mut huge = PNG_1X1.to_vec();
        huge.resize(MAX_BYTES + 1, 0);
        assert_eq!(save(&dir.path().to_string_lossy(), &huge).unwrap_err(), "attachment.tooLarge");
        assert!(!dir.path().join("attachments").exists(), "nothing should have been written");
    }

    /// Two pastes in the same session must not overwrite each other.
    #[test]
    fn two_pastes_produce_two_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        let a = save(&root, PNG_1X1).unwrap();
        let b = save(&root, PNG_1X1).unwrap();
        assert_ne!(a.path, b.path);
        assert!(std::fs::metadata(&a.path).is_ok() && std::fs::metadata(&b.path).is_ok());
    }

    /// The preview reads back what was written, and **only** from inside the
    /// session's own attachments directory.
    #[test]
    fn reading_back_is_confined_to_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        let saved = save(&root, PNG_1X1).unwrap();
        assert_eq!(read(&root, &saved.path).unwrap(), PNG_1X1);

        // A real file, outside the session. The preview is not a file reader.
        let outside = dir.path().join("secret.png");
        std::fs::write(&outside, PNG_1X1).unwrap();
        let err = read(&root, &outside.to_string_lossy()).unwrap_err();
        assert_eq!(err, "attachment.outsideSession");
    }

    /// Traversal is caught by canonicalising both sides, not by string
    /// matching — `attachments/../../elsewhere` resolves out of the root and
    /// a naive prefix check on the unresolved path would have allowed it.
    #[test]
    fn a_traversal_path_does_not_escape_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        save(&root, PNG_1X1).unwrap();

        let outside = dir.path().join("escape.png");
        std::fs::write(&outside, PNG_1X1).unwrap();
        let sneaky = dir.path().join("attachments").join("..").join("escape.png");
        assert!(read(&root, &sneaky.to_string_lossy()).is_err());
    }
}
