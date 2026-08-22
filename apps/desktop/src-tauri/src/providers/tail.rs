//! Following a JSONL transcript while the provider writes it.
//!
//! Polling, not filesystem notifications. Checking one file's length every few
//! hundred milliseconds costs almost nothing, whereas directory-change
//! notifications on Windows are coalesced, can be missed under load, and would
//! need the same length bookkeeping anyway. The simpler mechanism is also the
//! one that cannot silently stop delivering.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Follows a growing line-delimited file from a byte offset.
pub struct JsonlTailer {
    path: PathBuf,
    offset: u64,
    /// Bytes read after the last newline, held until the line is complete.
    partial: String,
}

impl JsonlTailer {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            offset: 0,
            partial: String::new(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read whatever has been appended and return the **complete** lines.
    ///
    /// A transcript is read while it is being written, so the last line is
    /// routinely half-flushed. Emitting it would hand the parser a truncated
    /// JSON object on every single poll, so an incomplete tail is buffered
    /// until its newline arrives.
    pub fn poll(&mut self) -> std::io::Result<Vec<String>> {
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            // Not written yet is the normal state early in a session.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };

        let len = file.seek(SeekFrom::End(0))?;

        // A shorter file means it was replaced or truncated; start over rather
        // than reading from a stale offset into the middle of a line.
        if len < self.offset {
            self.offset = 0;
            self.partial.clear();
        }
        if len == self.offset {
            return Ok(Vec::new());
        }

        file.seek(SeekFrom::Start(self.offset))?;
        let mut buffer = Vec::with_capacity((len - self.offset) as usize);
        file.take(len - self.offset).read_to_end(&mut buffer)?;
        self.offset = len;

        // A multi-byte character can straddle a read boundary; keep the
        // undecodable tail for the next poll instead of corrupting it.
        let (text, keep) = match std::str::from_utf8(&buffer) {
            Ok(text) => (text.to_string(), Vec::new()),
            Err(error) => {
                let valid = error.valid_up_to();
                let text = String::from_utf8_lossy(&buffer[..valid]).into_owned();
                (text, buffer[valid..].to_vec())
            }
        };
        if !keep.is_empty() {
            self.offset -= keep.len() as u64;
        }

        self.partial.push_str(&text);

        let mut lines = Vec::new();
        while let Some(index) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=index).collect();
            let trimmed = line.trim_end_matches(['\n', '\r']).to_string();
            if !trimmed.is_empty() {
                lines.push(trimmed);
            }
        }
        Ok(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn append(path: &Path, text: &str) {
        append_bytes(path, text.as_bytes());
    }

    fn append_bytes(path: &Path, bytes: &[u8]) {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        f.write_all(bytes).unwrap();
    }

    #[test]
    fn returns_nothing_before_the_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let mut tailer = JsonlTailer::new(dir.path().join("later.jsonl"));
        assert!(tailer.poll().unwrap().is_empty());
    }

    #[test]
    fn emits_only_complete_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut tailer = JsonlTailer::new(&path);

        append(&path, "{\"a\":1}\n{\"b\":2}\n{\"c\":");
        let first = tailer.poll().unwrap();
        assert_eq!(first, vec!["{\"a\":1}", "{\"b\":2}"]);

        // The half-written line is withheld until it is finished.
        assert!(tailer.poll().unwrap().is_empty());

        append(&path, "3}\n");
        assert_eq!(tailer.poll().unwrap(), vec!["{\"c\":3}"]);
    }

    #[test]
    fn does_not_re_emit_lines_it_has_already_returned() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut tailer = JsonlTailer::new(&path);

        append(&path, "one\n");
        assert_eq!(tailer.poll().unwrap(), vec!["one"]);
        assert!(tailer.poll().unwrap().is_empty());

        append(&path, "two\n");
        assert_eq!(tailer.poll().unwrap(), vec!["two"]);
    }

    #[test]
    fn survives_a_multibyte_character_split_across_a_poll() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut tailer = JsonlTailer::new(&path);

        // "é" is two bytes. Write everything up to its first byte, poll, then
        // deliver the remainder — a real PTY/transcript boundary looks like this.
        let bytes = "café\n".as_bytes().to_vec();
        let split = bytes.len() - 2; // lands mid-character
        std::fs::write(&path, &bytes[..split]).unwrap();

        // The dangling byte must be held back, not decoded lossily into U+FFFD.
        assert!(tailer.poll().unwrap().is_empty());

        append_bytes(&path, &bytes[split..]);
        assert_eq!(tailer.poll().unwrap(), vec!["café"]);
    }

    #[test]
    fn restarts_when_the_file_is_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut tailer = JsonlTailer::new(&path);

        append(&path, "first\nsecond\n");
        assert_eq!(tailer.poll().unwrap().len(), 2);

        std::fs::write(&path, "replaced\n").unwrap();
        assert_eq!(tailer.poll().unwrap(), vec!["replaced"]);
    }

    #[test]
    fn handles_crlf_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut tailer = JsonlTailer::new(&path);
        append(&path, "alpha\r\nbeta\r\n");
        assert_eq!(tailer.poll().unwrap(), vec!["alpha", "beta"]);
    }
}
