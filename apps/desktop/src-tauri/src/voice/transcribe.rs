//! Spawning whisper.cpp and reading back what it heard (§54).

use std::path::Path;
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

use super::{Result, VoiceError};

/// `resources/whisper/` — bundled alongside the app (see `tauri.conf.json`),
/// not installed system-wide. `whisper-cli.exe` and its dependencies
/// (`whisper.dll`, `ggml*.dll`, and the VC++ runtime DLLs it needs that a
/// pure-Rust binary like this app's own does not) all live together here so
/// nothing about running it depends on what happens to already be on a given
/// machine. See D29 in docs/DECISIONS.md.
const BINARY_NAME: &str = "whisper-cli.exe";

/// Transcribe a 16kHz mono WAV file already on disk, primed with `prompt`.
///
/// `whisper_language` is whisper.cpp's own code ("en", "pt", ...), driven by
/// the app's current locale (§65) — never fixed to one language.
pub fn transcribe(
    resource_dir: &Path,
    model_path: &Path,
    wav_path: &Path,
    prompt: &str,
    whisper_language: &str,
) -> Result<String> {
    let binary = resource_dir.join("whisper").join(BINARY_NAME);
    if !binary.is_file() {
        return Err(VoiceError::Transcribe(format!(
            "whisper-cli.exe not found at {}",
            binary.display()
        )));
    }

    let out_dir = scratch_dir()?;
    let out_base = out_dir.join("transcript");

    let mut cmd = Command::new(&binary);
    cmd.arg("-m")
        .arg(model_path)
        .arg("-f")
        .arg(wav_path)
        .arg("-l")
        .arg(whisper_language)
        .arg("--prompt")
        .arg(prompt)
        // Quiet: only the result, no progress banner or timestamps — this is
        // read back from a file, not scraped from stdout (see rule 27 in
        // docs/HANDOFF.md on why scraping process output is a bad idea).
        .arg("-np")
        .arg("-nt")
        .arg("-otxt")
        .arg("-of")
        .arg(&out_base);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let status = cmd
        .status()
        .map_err(|e| VoiceError::Transcribe(e.to_string()))?;
    let _ = std::fs::remove_file(wav_path);

    if !status.success() {
        let _ = std::fs::remove_dir_all(&out_dir);
        return Err(VoiceError::Transcribe(format!(
            "whisper-cli exited with {status}"
        )));
    }

    let txt_path = out_base.with_extension("txt");
    let text = std::fs::read_to_string(&txt_path)?;
    let _ = std::fs::remove_dir_all(&out_dir);
    Ok(text.trim().to_string())
}

fn scratch_dir() -> Result<std::path::PathBuf> {
    let dir = std::env::temp_dir().join(format!("jarvis-voice-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Map an app locale (§65) to the language code whisper.cpp expects.
pub fn whisper_language_for(locale: &str) -> &'static str {
    match locale {
        "pt-BR" => "pt",
        _ => "en",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_maps_to_a_short_whisper_language_code() {
        assert_eq!(whisper_language_for("pt-BR"), "pt");
        assert_eq!(whisper_language_for("en"), "en");
        assert_eq!(whisper_language_for("something-unknown"), "en");
    }

    #[test]
    fn a_missing_binary_is_reported_rather_than_spawned() {
        let dir = tempfile::tempdir().unwrap();
        let result = transcribe(
            dir.path(),
            Path::new("model.bin"),
            Path::new("audio.wav"),
            "vocab",
            "en",
        );
        assert!(matches!(result, Err(VoiceError::Transcribe(_))));
    }
}
