//! The on-disk session log.
//!
//! Append-only framed storage for one session. Raw PTY bytes are deliberately
//! **not** kept in SQLite: a busy agent session produces tens of megabytes of
//! terminal output, and storing that as rows makes both writes and replay slow.
//! SQLite holds metadata and the searchable structured events; the bytes live
//! here.
//!
//! ## File format
//!
//! ```text
//! stream.log
//!   header  : "JVSL" | version:u8 | reserved:3
//!   frame*  : payload_len:u32 | seq:u64 | ts_ms:i64 | kind:u8 | payload
//!
//! index.bin  (sparse — one entry every INDEX_INTERVAL frames)
//!   header  : "JVSI" | version:u8 | reserved:3
//!   entry*  : seq:u64 | offset:u64
//! ```
//!
//! All integers are little-endian.
//!
//! ## Crash safety
//!
//! A process killed mid-append leaves a partial trailing frame. On open the log
//! scans from the last index checkpoint, finds the last **complete** frame, and
//! truncates anything after it. A torn write therefore costs at most one frame
//! rather than corrupting the session.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use super::event::{EventKind, SessionEvent};

const STREAM_MAGIC: &[u8; 4] = b"JVSL";
const INDEX_MAGIC: &[u8; 4] = b"JVSI";
const FORMAT_VERSION: u8 = 1;
const HEADER_LEN: u64 = 8;
const FRAME_HEADER_LEN: usize = 4 + 8 + 8 + 1;

/// How often a checkpoint is written to the sparse index. Small enough that
/// opening a long session stays fast, large enough that the index stays tiny.
const INDEX_INTERVAL: u64 = 256;

/// Guards against a corrupt length field causing a huge allocation.
const MAX_PAYLOAD_LEN: u32 = 64 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("session log I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a J.A.R.V.I.S. session log")]
    BadMagic,
    #[error("unsupported session log version {0}")]
    UnsupportedVersion(u8),
    #[error("frame declares an implausible payload length ({0} bytes)")]
    ImplausibleLength(u32),
}

pub type Result<T> = std::result::Result<T, LogError>;

pub struct SessionLog {
    dir: PathBuf,
    stream: BufWriter<File>,
    index: BufWriter<File>,
    /// Byte offset at which the next frame will be written.
    write_offset: u64,
    next_seq: u64,
}

impl SessionLog {
    /// Open (or create) the log for a session directory.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;

        let stream_path = dir.join("stream.log");
        let index_path = dir.join("index.bin");

        let mut stream = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&stream_path)?;
        Self::ensure_header(&mut stream, STREAM_MAGIC)?;

        let mut index = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&index_path)?;
        Self::ensure_header(&mut index, INDEX_MAGIC)?;

        // Resume from the last checkpoint rather than rescanning the whole
        // file, so opening an hours-long session is O(INDEX_INTERVAL).
        let (mut offset, mut seq) = last_checkpoint(&mut index)?.unwrap_or((HEADER_LEN, 0));

        let end = stream.seek(SeekFrom::End(0))?;
        stream.seek(SeekFrom::Start(offset))?;

        while offset < end {
            match read_frame_at(&mut stream, offset, end)? {
                Some((event, next_offset)) => {
                    seq = event.seq + 1;
                    offset = next_offset;
                }
                // Partial trailing frame from an interrupted write.
                None => break,
            }
        }

        if offset < end {
            tracing::warn!(
                truncated_bytes = end - offset,
                "session log had an incomplete trailing frame; truncating"
            );
            stream.set_len(offset)?;
        }

        stream.seek(SeekFrom::Start(offset))?;
        let index_end = index.seek(SeekFrom::End(0))?;
        index.seek(SeekFrom::Start(index_end))?;

        Ok(Self {
            dir,
            stream: BufWriter::with_capacity(64 * 1024, stream),
            index: BufWriter::new(index),
            write_offset: offset,
            next_seq: seq,
        })
    }

    fn ensure_header(file: &mut File, magic: &[u8; 4]) -> Result<()> {
        let len = file.seek(SeekFrom::End(0))?;
        if len == 0 {
            file.seek(SeekFrom::Start(0))?;
            file.write_all(magic)?;
            file.write_all(&[FORMAT_VERSION, 0, 0, 0])?;
            file.flush()?;
            return Ok(());
        }

        file.seek(SeekFrom::Start(0))?;
        let mut header = [0u8; 8];
        file.read_exact(&mut header)?;
        if &header[0..4] != magic {
            return Err(LogError::BadMagic);
        }
        if header[4] != FORMAT_VERSION {
            return Err(LogError::UnsupportedVersion(header[4]));
        }
        Ok(())
    }

    /// Append a frame and return its sequence number.
    pub fn append(&mut self, kind: EventKind, payload: &[u8]) -> Result<u64> {
        if payload.len() as u64 > MAX_PAYLOAD_LEN as u64 {
            return Err(LogError::ImplausibleLength(payload.len() as u32));
        }
        let seq = self.next_seq;
        let ts_ms = now_ms();

        let mut header = [0u8; FRAME_HEADER_LEN];
        header[0..4].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        header[4..12].copy_from_slice(&seq.to_le_bytes());
        header[12..20].copy_from_slice(&ts_ms.to_le_bytes());
        header[20] = kind as u8;

        self.stream.write_all(&header)?;
        self.stream.write_all(payload)?;
        // Flush every frame: an agent session that survives a crash but loses
        // its last minute of output is a data-integrity bug, and integrity
        // outranks throughput (§91).
        self.stream.flush()?;

        if seq % INDEX_INTERVAL == 0 {
            self.index.write_all(&seq.to_le_bytes())?;
            self.index.write_all(&self.write_offset.to_le_bytes())?;
            self.index.flush()?;
        }

        self.write_offset += (FRAME_HEADER_LEN + payload.len()) as u64;
        self.next_seq += 1;
        Ok(seq)
    }

    /// Sequence number the next appended frame will receive.
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Read every frame with `seq >= from_seq`.
    pub fn read_from(&self, from_seq: u64) -> Result<Vec<SessionEvent>> {
        SessionLogReader::open(&self.dir)?.read_from(from_seq)
    }
}

/// Independent reader, so projections can read a log while it is being written.
pub struct SessionLogReader {
    stream: File,
    index_path: PathBuf,
    end: u64,
}

impl SessionLogReader {
    pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let mut stream = File::open(dir.join("stream.log"))?;
        let end = stream.seek(SeekFrom::End(0))?;
        Ok(Self {
            stream,
            index_path: dir.join("index.bin"),
            end,
        })
    }

    /// Find the closest checkpoint at or before `seq` so reads can skip ahead.
    fn seek_hint(&self, seq: u64) -> Result<u64> {
        let Ok(mut index) = File::open(&self.index_path) else {
            return Ok(HEADER_LEN);
        };
        let len = index.seek(SeekFrom::End(0))?;
        if len <= HEADER_LEN {
            return Ok(HEADER_LEN);
        }

        let mut best = HEADER_LEN;
        index.seek(SeekFrom::Start(HEADER_LEN))?;
        let mut entry = [0u8; 16];
        while index.read_exact(&mut entry).is_ok() {
            let entry_seq = u64::from_le_bytes(entry[0..8].try_into().unwrap());
            let offset = u64::from_le_bytes(entry[8..16].try_into().unwrap());
            if entry_seq > seq {
                break;
            }
            best = offset;
        }
        Ok(best)
    }

    pub fn read_from(&self, from_seq: u64) -> Result<Vec<SessionEvent>> {
        let mut file = self.stream.try_clone()?;
        let mut offset = self.seek_hint(from_seq)?;
        let mut events = Vec::new();

        while offset < self.end {
            let Some((event, next)) = read_frame_at(&mut file, offset, self.end)? else {
                break;
            };
            offset = next;
            if event.seq >= from_seq {
                events.push(event);
            }
        }
        Ok(events)
    }

    /// Concatenate the terminal output for replay, keeping at most
    /// `max_bytes` from the end of the session.
    ///
    /// Replaying an entire long session into xterm would stall the UI and
    /// overflow its scrollback anyway, so old output is dropped from the front.
    /// Frames are never split: a truncated escape sequence would corrupt the
    /// terminal state.
    pub fn replay_pty(&self, max_bytes: usize) -> Result<Vec<u8>> {
        let mut file = self.stream.try_clone()?;
        let mut chunks: Vec<Vec<u8>> = Vec::new();
        let mut total = 0usize;
        let mut offset = HEADER_LEN;

        while offset < self.end {
            let Some((event, next)) = read_frame_at(&mut file, offset, self.end)? else {
                break;
            };
            offset = next;
            if event.kind == EventKind::PtyOutput {
                total += event.payload.len();
                chunks.push(event.payload);
            }
        }

        // Drop whole frames from the front until the tail fits.
        let mut start = 0usize;
        while total > max_bytes && start < chunks.len() {
            total -= chunks[start].len();
            start += 1;
        }

        Ok(chunks[start..].concat())
    }
}

/// Read one frame. Returns `None` for a partial frame at end-of-file, which is
/// the normal signature of an interrupted write rather than corruption.
fn read_frame_at(file: &mut File, offset: u64, end: u64) -> Result<Option<(SessionEvent, u64)>> {
    if end.saturating_sub(offset) < FRAME_HEADER_LEN as u64 {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(offset))?;

    let mut header = [0u8; FRAME_HEADER_LEN];
    if file.read_exact(&mut header).is_err() {
        return Ok(None);
    }

    let payload_len = u32::from_le_bytes(header[0..4].try_into().unwrap());
    if payload_len > MAX_PAYLOAD_LEN {
        return Err(LogError::ImplausibleLength(payload_len));
    }
    let seq = u64::from_le_bytes(header[4..12].try_into().unwrap());
    let ts_ms = i64::from_le_bytes(header[12..20].try_into().unwrap());

    let Some(kind) = EventKind::from_u8(header[20]) else {
        // An unknown kind means a newer version wrote this log. The frame is
        // still correctly framed, so skip it instead of failing the session.
        let next = offset + FRAME_HEADER_LEN as u64 + payload_len as u64;
        return Ok(if next <= end {
            read_frame_at(file, next, end)?
        } else {
            None
        });
    };

    let frame_end = offset + FRAME_HEADER_LEN as u64 + payload_len as u64;
    if frame_end > end {
        return Ok(None);
    }

    let mut payload = vec![0u8; payload_len as usize];
    if file.read_exact(&mut payload).is_err() {
        return Ok(None);
    }

    Ok(Some((SessionEvent::new(seq, ts_ms, kind, payload), frame_end)))
}

fn last_checkpoint(index: &mut File) -> Result<Option<(u64, u64)>> {
    let len = index.seek(SeekFrom::End(0))?;
    if len < HEADER_LEN + 16 {
        return Ok(None);
    }
    let entries = (len - HEADER_LEN) / 16;
    let last = HEADER_LEN + (entries - 1) * 16;
    index.seek(SeekFrom::Start(last))?;

    let mut entry = [0u8; 16];
    index.read_exact(&mut entry)?;
    let seq = u64::from_le_bytes(entry[0..8].try_into().unwrap());
    let offset = u64::from_le_bytes(entry[8..16].try_into().unwrap());
    Ok(Some((offset, seq)))
}

pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn round_trips_frames_in_order() {
        let dir = temp();
        let mut log = SessionLog::open(dir.path()).unwrap();
        log.append(EventKind::PtyOutput, b"hello ").unwrap();
        log.append(EventKind::Message, br#"{"role":"user"}"#).unwrap();
        log.append(EventKind::PtyOutput, b"world").unwrap();

        let events = log.read_from(0).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].seq, 0);
        assert_eq!(events[2].seq, 2);
        assert_eq!(events[0].payload, b"hello ");
        assert_eq!(events[1].kind, EventKind::Message);
    }

    #[test]
    fn reopening_continues_the_sequence() {
        let dir = temp();
        {
            let mut log = SessionLog::open(dir.path()).unwrap();
            log.append(EventKind::PtyOutput, b"a").unwrap();
            log.append(EventKind::PtyOutput, b"b").unwrap();
        }
        let mut log = SessionLog::open(dir.path()).unwrap();
        assert_eq!(log.next_seq(), 2);
        assert_eq!(log.append(EventKind::PtyOutput, b"c").unwrap(), 2);
        assert_eq!(log.read_from(0).unwrap().len(), 3);
    }

    #[test]
    fn survives_a_torn_trailing_write() {
        let dir = temp();
        {
            let mut log = SessionLog::open(dir.path()).unwrap();
            log.append(EventKind::PtyOutput, b"complete").unwrap();
        }
        // Simulate a process killed mid-append: a frame header claiming more
        // payload than was actually written.
        {
            let mut f = OpenOptions::new()
                .append(true)
                .open(dir.path().join("stream.log"))
                .unwrap();
            let mut header = [0u8; FRAME_HEADER_LEN];
            header[0..4].copy_from_slice(&1000u32.to_le_bytes());
            header[4..12].copy_from_slice(&1u64.to_le_bytes());
            header[20] = EventKind::PtyOutput as u8;
            f.write_all(&header).unwrap();
            f.write_all(b"only a few bytes").unwrap();
        }

        let mut log = SessionLog::open(dir.path()).unwrap();
        // The complete frame survives, the torn one is discarded, and the
        // sequence resumes where the intact data ended.
        assert_eq!(log.next_seq(), 1);
        let events = log.read_from(0).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload, b"complete");

        log.append(EventKind::PtyOutput, b"after recovery").unwrap();
        assert_eq!(log.read_from(0).unwrap().len(), 2);
    }

    #[test]
    fn read_from_uses_the_sparse_index() {
        let dir = temp();
        let mut log = SessionLog::open(dir.path()).unwrap();
        for i in 0..(INDEX_INTERVAL * 3) {
            log.append(EventKind::PtyOutput, format!("frame {i}").as_bytes())
                .unwrap();
        }
        let tail = log.read_from(INDEX_INTERVAL * 2 + 5).unwrap();
        assert_eq!(tail.len() as u64, INDEX_INTERVAL - 5);
        assert_eq!(tail[0].seq, INDEX_INTERVAL * 2 + 5);
        assert_eq!(tail[0].payload, format!("frame {}", INDEX_INTERVAL * 2 + 5).as_bytes());
    }

    #[test]
    fn replay_keeps_only_terminal_bytes_and_respects_the_cap() {
        let dir = temp();
        let mut log = SessionLog::open(dir.path()).unwrap();
        log.append(EventKind::PtyOutput, b"1111").unwrap();
        log.append(EventKind::Message, b"{}").unwrap();
        log.append(EventKind::PtyOutput, b"2222").unwrap();
        log.append(EventKind::PtyOutput, b"3333").unwrap();

        let reader = SessionLogReader::open(dir.path()).unwrap();
        // Structured frames are excluded from terminal replay entirely.
        assert_eq!(reader.replay_pty(usize::MAX).unwrap(), b"111122223333");
        // Whole frames are dropped from the front, never split mid-frame.
        assert_eq!(reader.replay_pty(9).unwrap(), b"22223333");
    }

    #[test]
    fn preserves_binary_payloads_exactly() {
        let dir = temp();
        let mut log = SessionLog::open(dir.path()).unwrap();
        // Escape sequences and a UTF-8 sequence split across two frames, which
        // is what a real PTY read boundary looks like.
        let a = b"\x1b[38;5;208mwarn\x1b[0m \xE2\x9C";
        let b = b"\x93 done\x00\xff";
        log.append(EventKind::PtyOutput, a).unwrap();
        log.append(EventKind::PtyOutput, b).unwrap();

        let reader = SessionLogReader::open(dir.path()).unwrap();
        let replayed = reader.replay_pty(usize::MAX).unwrap();
        assert_eq!(replayed, [a.as_slice(), b.as_slice()].concat());
    }

    #[test]
    fn rejects_a_foreign_file() {
        let dir = temp();
        std::fs::write(dir.path().join("stream.log"), b"NOTJARVISDATA").unwrap();
        assert!(matches!(
            SessionLog::open(dir.path()),
            Err(LogError::BadMagic)
        ));
    }
}
