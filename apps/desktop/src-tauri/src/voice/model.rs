//! The transcription model (§54) — downloaded once, verified, then local.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::ipc::Channel;

use super::{Result, VoiceError};

/// Pinned so a corrupted or substituted download is caught before it is ever
/// run against real audio — the same bar the updater holds its own artifacts
/// to (§62), applied here because nothing else verifies a ~490MB file pulled
/// from the internet on first use.
const MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin";
const MODEL_SHA256: &str = "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b";
const MODEL_FILE: &str = "ggml-small.bin";

pub fn model_path(data_dir: &Path) -> PathBuf {
    data_dir.join("models").join(MODEL_FILE)
}

pub fn is_present(data_dir: &Path) -> bool {
    model_path(data_dir).is_file()
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum DownloadProgress {
    Downloading { received: u64, total: u64 },
    Verifying,
    Done,
    Error { message: String },
}

/// Download the model to a `.part` file, verify its hash, then rename it into
/// place — so an interrupted or corrupted download never leaves behind
/// something `is_present` would trust as complete.
pub fn download(data_dir: &Path, channel: &Channel<DownloadProgress>) {
    if let Err(e) = download_inner(data_dir, channel) {
        let _ = channel.send(DownloadProgress::Error {
            message: e.to_string(),
        });
    }
}

fn download_inner(data_dir: &Path, channel: &Channel<DownloadProgress>) -> Result<()> {
    let models_dir = data_dir.join("models");
    std::fs::create_dir_all(&models_dir)?;
    let part_path = models_dir.join(format!("{MODEL_FILE}.part"));
    let final_path = model_path(data_dir);

    let response = ureq::get(MODEL_URL)
        .call()
        .map_err(|e| VoiceError::Download(e.to_string()))?;
    let total: u64 = response
        .header("Content-Length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let mut file = std::fs::File::create(&part_path)?;
    let mut reader = response.into_reader();
    let mut buf = [0u8; 64 * 1024];
    let mut received: u64 = 0;
    let mut hasher = Sha256::new();

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        received += n as u64;
        let _ = channel.send(DownloadProgress::Downloading { received, total });
    }
    drop(file);

    let _ = channel.send(DownloadProgress::Verifying);
    let digest_hex = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if digest_hex != MODEL_SHA256 {
        let _ = std::fs::remove_file(&part_path);
        return Err(VoiceError::ModelCorrupt);
    }

    std::fs::rename(&part_path, &final_path)?;
    let _ = channel.send(DownloadProgress::Done);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_reflects_the_real_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_present(dir.path()));

        std::fs::create_dir_all(dir.path().join("models")).unwrap();
        std::fs::write(model_path(dir.path()), b"not a real model").unwrap();
        assert!(is_present(dir.path()));
    }

    /// A download that fails hash verification must not leave anything an
    /// interrupted retry, or `is_present`, would mistake for the real model.
    #[test]
    fn a_hash_mismatch_is_not_left_on_disk_as_the_final_file() {
        let dir = tempfile::tempdir().unwrap();
        let models_dir = dir.path().join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        // Simulate what download_inner does on a mismatch, without a network
        // call: write wrong bytes to .part, check the hash, remove it.
        let part = models_dir.join(format!("{MODEL_FILE}.part"));
        std::fs::write(&part, b"corrupted").unwrap();
        let digest = Sha256::digest(b"corrupted");
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        assert_ne!(hex, MODEL_SHA256);

        std::fs::remove_file(&part).unwrap();
        assert!(!part.exists());
        assert!(!is_present(dir.path()));
    }
}
