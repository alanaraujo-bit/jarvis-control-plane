//! Turning a slice of terminal output back into the text a person would read.
//!
//! ## Why this is not `strip_ansi`
//!
//! `preview/detect.rs` strips escape sequences and keeps the rest, which is
//! right for a dev-server banner: the banner is ordinary text with colour
//! wrapped round it.
//!
//! An agent CLI is not ordinary text. Claude Code and Codex draw their
//! interface by **positioning the cursor**, and they emit `CSI n C`
//! (cursor-forward) where a person sees a space. Strip the escapes and
//! `Do you want to create hello.txt?` comes out as `Doyouwanttocreatehello.txt?`
//! — every word run together, and every pattern written against the readable
//! form silently matches nothing. That was measured from a real capture, not
//! reasoned about (see `capture`).
//!
//! So this is a *renderer*, deliberately a small one:
//!
//! | Sequence | Meaning here |
//! |---|---|
//! | `CSI n C` | n spaces |
//! | `CSI r;c H` / `f` | a line break |
//! | `CSI n A/B/E/F` | a line break, for vertical moves |
//! | `OSC 0;text BEL` | collected as a window title, not as content |
//! | everything else | dropped |
//!
//! It is **not** a terminal emulator and must never grow into one. It answers
//! one question: what words are on the screen, in roughly what order. A real
//! emulator with a screen buffer would answer it better and would cost a
//! dependency, a grid, and a scrollback model to decide "is a question showing".

/// What one slice of terminal output says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rendered {
    /// Non-empty lines, in the order they were drawn.
    pub lines: Vec<String>,
    /// Window titles the program set, in order. Claude Code puts a label for
    /// what it is doing here; see the note in `capture`.
    pub titles: Vec<String>,
}

/// The most a single render will produce, in lines.
///
/// A guard rather than a tuning knob: a burst of build output can be tens of
/// thousands of lines and nothing here needs more than the last screenful.
const MAX_LINES: usize = 400;

/// The widest run of spaces a single cursor-forward will be believed for.
///
/// A corrupt or hostile parameter should not turn into a multi-megabyte line.
const MAX_CURSOR_FORWARD: usize = 200;

/// Render terminal output into readable lines.
pub fn render(bytes: &[u8]) -> Rendered {
    // Lossy on purpose. The caller hands us a *tail* of a byte stream, so the
    // first character is very likely half of a UTF-8 sequence. A hard failure
    // there would mean the detector goes blind exactly when output is busiest.
    let text = String::from_utf8_lossy(bytes);
    render_str(&text)
}

pub fn render_str(text: &str) -> Rendered {
    const ESC: char = '\u{1b}';
    const BEL: char = '\u{7}';
    const BACKSLASH: char = '\u{5c}';

    let mut out = String::with_capacity(text.len());
    let mut titles: Vec<String> = Vec::new();
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c != ESC {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('[') => {
                chars.next();
                let mut params = String::new();
                let mut final_byte = None;
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        final_byte = Some(c);
                        break;
                    }
                    params.push(c);
                }
                match final_byte {
                    // Cursor forward — this is a run of spaces to a reader.
                    Some('C') => {
                        let n = params
                            .parse::<usize>()
                            .unwrap_or(1)
                            .clamp(1, MAX_CURSOR_FORWARD);
                        for _ in 0..n {
                            out.push(' ');
                        }
                    }
                    // Any move that can change the row starts a new line. This
                    // over-splits — a redraw of one row emits a break — and
                    // that is the right way to be wrong: an extra line boundary
                    // costs nothing, a missing one glues two rows together and
                    // hides the shape the detector is looking for.
                    Some('H') | Some('f') | Some('A') | Some('B') | Some('E') | Some('F') => {
                        out.push('\n');
                    }
                    _ => {}
                }
            }
            // OSC: ESC ] … terminated by BEL or ST (ESC backslash).
            Some(']') => {
                chars.next();
                let mut body = String::new();
                while let Some(c) = chars.next() {
                    if c == BEL {
                        break;
                    }
                    if c == ESC {
                        if chars.peek().copied() == Some(BACKSLASH) {
                            chars.next();
                        }
                        break;
                    }
                    body.push(c);
                }
                // `0;title` and `2;title` both set a window title.
                if let Some(title) = body.strip_prefix("0;").or_else(|| body.strip_prefix("2;")) {
                    if !title.is_empty() {
                        titles.push(title.to_string());
                    }
                }
            }
            // Two-character escapes: consume the second character.
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }

    let mut lines: Vec<String> = Vec::new();
    for raw in out.replace('\r', "\n").split('\n') {
        let line = raw.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        lines.push(line.to_string());
    }
    if lines.len() > MAX_LINES {
        lines.drain(..lines.len() - MAX_LINES);
    }

    Rendered { lines, titles }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this module exists for, in one assertion.
    #[test]
    fn cursor_forward_is_a_space_not_nothing() {
        let raw = "Do\u{1b}[1Cyou\u{1b}[1Cwant\u{1b}[1Cto\u{1b}[1Ccreate\u{1b}[1Chello.txt?";
        assert_eq!(render_str(raw).lines, vec!["Do you want to create hello.txt?"]);
    }

    #[test]
    fn a_wider_jump_is_a_run_of_spaces() {
        assert_eq!(render_str("a\u{1b}[4Cb").lines, vec!["a    b"]);
    }

    #[test]
    fn absolute_positioning_starts_a_line() {
        let raw = "\u{1b}[25;2HDo\u{1b}[1Cyou?\u{1b}[26;2H\u{276f}\u{1b}[1C1.\u{1b}[1CYes";
        assert_eq!(render_str(raw).lines, vec!["Do you?", "\u{276f} 1. Yes"]);
    }

    #[test]
    fn colours_and_erases_leave_no_trace() {
        let raw = "\u{1b}[38;2;215;119;87mhello\u{1b}[K\u{1b}[m world";
        assert_eq!(render_str(raw).lines, vec!["hello world"]);
    }

    #[test]
    fn window_titles_are_collected_and_never_rendered_as_content() {
        let r = render_str("\u{1b}]0;\u{25d0} Claude Code\u{7}on screen");
        assert_eq!(r.titles, vec!["\u{25d0} Claude Code"]);
        assert_eq!(r.lines, vec!["on screen"]);
    }

    #[test]
    fn a_title_terminated_by_st_is_understood_too() {
        let r = render_str("\u{1b}]0;work\u{1b}\u{5c}rest");
        assert_eq!(r.titles, vec!["work"]);
        assert_eq!(r.lines, vec!["rest"]);
    }

    /// The caller hands over a tail of a byte stream, so byte zero is very
    /// likely the middle of a character.
    #[test]
    fn a_tail_that_starts_mid_character_still_renders() {
        let full = "ol\u{e1} \u{276f} 1. Sim".as_bytes();
        let severed = &full[1..];
        assert!(!render(severed).lines.is_empty());
    }

    /// A malformed parameter must not become an enormous line.
    #[test]
    fn an_implausible_cursor_jump_is_bounded() {
        let rendered = render_str("a\u{1b}[99999Cb");
        assert!(rendered.lines[0].chars().count() <= MAX_CURSOR_FORWARD + 2);
    }
}
