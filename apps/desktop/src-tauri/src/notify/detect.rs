//! Recognising that an agent has stopped and is asking (§49).
//!
//! ## The gap this fills
//!
//! Everything else J.A.R.V.I.S. knows about a stopped agent comes from
//! something the provider *stated*: a finished turn in the transcript, a
//! guardrail decision, an autopilot stop reason. Those are Official (§28) and
//! nothing here competes with them.
//!
//! But the most ordinary "the agent is waiting for you" moment of all — Claude
//! Code asking whether it may write a file — is stated nowhere. The guardrail
//! hook returns early for any tool that is not `Bash`, and again for a `Bash`
//! command that classifies as nothing sensitive. The transcript records the
//! answer, not the question. The only place that moment exists is **on the
//! screen**, and the session log already holds every byte of it (§23).
//!
//! So this reads it off the screen, and labels what it reads **Observed** —
//! never Official. Same discipline as usage figures: the confidence travels
//! with the fact.
//!
//! ## What it looks for, and what it deliberately does not
//!
//! Captured from real CLIs (see `capture` and `docs/M14-NOTIFICATIONS.md`), the
//! wording varies wildly and carries no reliable invariant:
//!
//! * `Do you want to create hello.txt?` — a file write
//! * `Do you want to proceed?` — a shell command, with the command above it
//! * `Choose the text style that looks best with your terminal` — no question
//!   mark at all, and no footer
//! * `Do you trust the contents of this directory?` — Codex, a different glyph
//!
//! One thing is common to all four:
//!
//! > **A numbered choice list, of which exactly one row carries a cursor glyph.**
//!
//! That is the whole test. The wording is used only to write the preview, and
//! is never what decides a prompt is showing. Matching on "Do you want" would
//! have missed the theme picker and would break the day a provider rewords a
//! sentence — which providers do, and are entitled to.
//!
//! ## Why a match alone is not enough
//!
//! An agent that runs `cat` on a file containing a numbered list has just drawn
//! something this recognises. Being wrong here is expensive: a notification
//! that says an agent is waiting when it is working hard is worse than no
//! notification, because it teaches the person to ignore the next one.
//!
//! So a match is only a *candidate*. `watch` requires it to hold while the
//! terminal has been **quiet** and **nothing has been typed** since. A file
//! being `cat`-ed scrolls past in milliseconds; a question sits there. See
//! `watch` for the conjunction.

use super::render::{render, Rendered};

/// The glyphs an agent CLI uses to mark the selected row.
///
/// `❯` (U+276F) is Claude Code and `›` (U+203A) is Codex, both from real
/// captures. The two ornament variants are their near neighbours in the same
/// family, which a CLI could reasonably use instead.
///
/// **A plain `>` is deliberately not here**, though it was at first. It is the
/// quotation marker in every markdown file, mail body and diff on earth, so
/// this — a completely ordinary thing for an agent to print —
///
/// ```text
/// > 1. First item
///   2. Second item
/// ```
///
/// parses as a live selection with the first row chosen. The quiet conjunction
/// in `watch` hides that while the list scrolls past mid-turn, and does not
/// hide it when the list is the last thing on screen as a turn ends: the
/// notification would then say an agent needs approval when it has finished,
/// which is the wrong-notification case that teaches people to ignore the next
/// one. Accepting `>` bought nothing — no captured CLI uses it — and cost that.
const CURSOR_GLYPHS: &[char] = &['\u{276f}', '\u{203a}', '\u{2771}', '\u{25b6}'];

/// How far back from the choice list to look for the question it belongs to.
const QUESTION_LOOKBACK: usize = 8;

/// The width at which a line of pure decoration counts as a panel edge.
///
/// Both CLIs box a question in with a full-width rule. That rule is the honest
/// boundary of "what this question is about": everything above it is the
/// conversation, everything below it is the prompt. Using it rather than a
/// line count is what stops the preview quoting the status bar.
const RULE_WIDTH: usize = 20;

/// Lines that are drawing, not saying anything.
fn is_decoration(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return true;
    }
    // A rule, a box edge, or a diff gutter: no letters or digits in it at all.
    !trimmed.chars().any(|c| c.is_alphanumeric())
}

/// A decoration line wide enough to be the edge of the question's own panel.
fn is_rule(line: &str) -> bool {
    is_decoration(line) && line.trim().chars().count() >= RULE_WIDTH
}

/// Lines telling you which key to press, rather than what is being asked.
///
/// `Esc to cancel · Tab to amend`, `Press enter to continue`, `To change this
/// later, run /theme`. Every capture has at least one, and reading one out as
/// the question is the difference between a notification that says what the
/// agent wants and one that says "run /theme".
fn is_key_hint(line: &str) -> bool {
    let lower = line.trim().to_lowercase();
    const OPENERS: &[&str] = &["press ", "to change ", "enter to ", "esc to ", "tab to "];
    const ANYWHERE: &[&str] = &[
        " to cancel",
        " to confirm",
        " to continue",
        " to amend",
        " to explain",
        " for shortcuts",
    ];
    OPENERS.iter().any(|p| lower.starts_with(p))
        || ANYWHERE.iter().any(|p| lower.contains(p))
}

/// A question short enough that it cannot be naming what it is about.
///
/// `Do you want to proceed?` is true of every command ever run. `Do you want to
/// create hello.txt?` already says everything. Counting words is a blunt way to
/// tell those apart, and it is the right blunt instrument: it needs no wording
/// list, so it keeps working when a provider rewrites the sentence.
fn needs_a_subject(question: &str) -> bool {
    question.split_whitespace().count() <= 5
}

/// One row of a choice list: `❯ 1. Yes`, `  2. No`, `3) Continue`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Choice {
    number: u32,
    label: String,
    selected: bool,
}

fn parse_choice(line: &str) -> Option<Choice> {
    let mut rest = line.trim_start();
    let mut selected = false;
    if let Some(first) = rest.chars().next() {
        if CURSOR_GLYPHS.contains(&first) {
            selected = true;
            rest = rest[first.len_utf8()..].trim_start();
        }
    }
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 2 {
        return None;
    }
    let rest = &rest[digits.len()..];
    let rest = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')'))?;
    // A separator is required. Without it `1.5 seconds` in ordinary output
    // parses as choice 1 labelled "5 seconds".
    if !rest.starts_with(' ') {
        return None;
    }
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    Some(Choice {
        number: digits.parse().ok()?,
        label: rest.to_string(),
        selected,
    })
}

/// A question an agent is waiting on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// The question as drawn, e.g. `Do you want to create hello.txt?`.
    pub question: String,
    /// What the question is about, when the screen said so above it — the file
    /// being written, the command being run. Empty when there was nothing.
    pub subject: Vec<String>,
    /// The options offered, in order.
    pub options: Vec<String>,
}

impl Prompt {
    /// One line a person can read in a notification.
    ///
    /// The question first, because it is what was asked. The subject after it,
    /// because the question alone can say nothing — `Do you want to proceed?`
    /// is true of every shell command ever run and is not worth waking someone
    /// for on its own.
    pub fn preview(&self, max_chars: usize) -> String {
        let mut text = self.question.clone();
        if !self.subject.is_empty() {
            if !text.is_empty() {
                text.push_str(" — ");
            }
            text.push_str(&self.subject.join(" · "));
        }
        crate::providers::conversation::truncate(&text, max_chars)
    }
}

/// Look for a question on the last screen of terminal output.
pub fn prompt(bytes: &[u8]) -> Option<Prompt> {
    prompt_rendered(&render(bytes))
}

pub fn prompt_rendered(screen: &Rendered) -> Option<Prompt> {
    let lines = &screen.lines;

    // Walk back from the end to the last complete choice list. Back rather
    // than forward: a session's output holds every list it ever drew, and only
    // the last one can still be on screen.
    let mut end = lines.len();
    while end > 0 {
        let mut start = end;
        let mut choices: Vec<Choice> = Vec::new();
        while start > 0 {
            match parse_choice(&lines[start - 1]) {
                Some(choice) => {
                    choices.push(choice);
                    start -= 1;
                }
                None => break,
            }
        }
        choices.reverse();

        if is_a_live_choice_list(&choices) {
            let (question, subject) = question_and_subject(lines, start);
            return Some(Prompt {
                question,
                subject,
                options: choices.into_iter().map(|c| c.label).collect(),
            });
        }
        end -= 1;
    }
    None
}

/// Whether a run of parsed rows is a list somebody is being asked to choose from.
///
/// Three conditions, each earning its place:
///
/// * **At least two options.** A single `1. something` is a numbered note.
/// * **Numbered 1, 2, 3 with nothing missing.** Ordinary output full of
///   numbers does not usually arrive in sequence from one.
/// * **Exactly one row carries the cursor.** This is the one that says a
///   *selection* is happening rather than a list being printed. Without it, a
///   numbered list a script wrote out would qualify.
fn is_a_live_choice_list(choices: &[Choice]) -> bool {
    if choices.len() < 2 {
        return false;
    }
    if choices.iter().filter(|c| c.selected).count() != 1 {
        return false;
    }
    choices
        .iter()
        .enumerate()
        .all(|(i, c)| c.number as usize == i + 1)
}

/// Read the question, and what it is about, off the panel above the list.
///
/// The panel is everything between the choice list and the rule the CLI drew
/// above it (or `QUESTION_LOOKBACK` lines, if it drew none). Within it:
///
/// * the **question** is the last line that ends in `?`, and failing that the
///   last line that is not telling you which key to press. The theme picker is
///   why the second rule exists — it asks `Choose the text style…` with no
///   question mark and a `To change this later, run /theme` hint underneath,
///   and taking the nearest line read the hint out as the question.
/// * the **subject** is the top of the panel — `Bash command`, then the command
///   itself — and only when the question is too short to be naming anything.
///   Claude Code puts the header at the top and its own commentary at the
///   bottom, so nearest-first picks up `This command requires approval` and
///   loses `git --version`, which is the only part worth reading.
fn question_and_subject(lines: &[String], start: usize) -> (String, Vec<String>) {
    let floor = start.saturating_sub(QUESTION_LOOKBACK);
    let mut panel: Vec<&str> = Vec::new();
    for line in lines[floor..start].iter().rev() {
        if is_rule(line) {
            break;
        }
        if is_decoration(line) {
            continue;
        }
        panel.push(line.trim());
    }
    panel.reverse();

    let question_at = panel
        .iter()
        .rposition(|line| line.ends_with('?'))
        .or_else(|| panel.iter().rposition(|line| !is_key_hint(line)));

    let Some(question_at) = question_at else {
        return (String::new(), Vec::new());
    };
    let question = panel[question_at].to_string();

    let subject = if needs_a_subject(&question) {
        panel[..question_at]
            .iter()
            .filter(|line| !is_key_hint(line))
            .take(2)
            .map(|line| line.to_string())
            .collect()
    } else {
        Vec::new()
    };

    (question, subject)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bytes recorded from a real CLI. See `capture` for how they were made;
    /// they are trimmed to the prompt and scrubbed of this machine's own paths.
    const CLAUDE_WRITE: &[u8] = include_bytes!("prompts/claude-write-prompt.bin");
    const CLAUDE_COMMAND: &[u8] = include_bytes!("prompts/claude-command-prompt.bin");
    const CLAUDE_THEME: &[u8] = include_bytes!("prompts/claude-theme-prompt.bin");
    const CODEX_TRUST: &[u8] = include_bytes!("prompts/codex-trust-prompt.bin");
    const CLAUDE_WORKING: &[u8] = include_bytes!("prompts/claude-working.bin");

    #[test]
    fn claude_code_asking_to_write_a_file() {
        let found = prompt(CLAUDE_WRITE).expect("a question is on screen");
        assert_eq!(found.question, "Do you want to create hello.txt?");
        assert_eq!(found.options, vec!["Yes", "No"]);
    }

    /// The case the preview exists for: the question says nothing, the lines
    /// above it say everything.
    #[test]
    fn claude_code_asking_to_run_a_command_carries_the_command() {
        let found = prompt(CLAUDE_COMMAND).expect("a question is on screen");
        assert_eq!(found.question, "Do you want to proceed?");
        let preview = found.preview(120);
        assert!(
            preview.contains("git --version"),
            "the command has to reach the preview, or the notification says \
             nothing at all: {preview}"
        );
        assert_eq!(found.options.len(), 3);
    }

    /// No question mark, no footer, and still a stopped agent.
    #[test]
    fn a_question_that_is_not_phrased_as_one_is_still_a_question() {
        let found = prompt(CLAUDE_THEME).expect("a choice is on screen");
        assert!(
            found.question.contains("Choose"),
            "unexpected question: {}",
            found.question
        );
        assert!(found.options.len() >= 3);
    }

    /// A different provider, a different glyph, the same shape.
    #[test]
    fn codex_asking_whether_to_trust_a_folder() {
        let found = prompt(CODEX_TRUST).expect("a question is on screen");
        assert!(
            found.preview(160).to_lowercase().contains("trust"),
            "unexpected preview: {}",
            found.preview(160)
        );
        assert_eq!(found.options.len(), 2);
    }

    /// The negative that matters most: a busy agent must never look like a
    /// waiting one.
    #[test]
    fn an_agent_that_is_working_is_not_asking() {
        assert_eq!(prompt(CLAUDE_WORKING), None);
    }

    #[test]
    fn ordinary_output_that_happens_to_be_numbered_is_not_a_question() {
        // A printed list. Nothing is selected, so nothing is being chosen.
        let text = "Files changed:\n1. src/main.rs\n2. src/lib.rs\n3. Cargo.toml\n";
        assert_eq!(prompt(text.as_bytes()), None);
    }

    /// A quoted numbered list — markdown, a mail body, a diff — is the most
    /// ordinary thing an agent can print, and it must never read as a choice.
    #[test]
    fn a_quoted_markdown_list_is_not_a_question() {
        let text = "Steps to reproduce:\n> 1. Open the file\n  2. Press save\n  3. Watch it hang\n";
        assert_eq!(prompt(text.as_bytes()), None);
    }

    #[test]
    fn a_single_numbered_line_is_not_a_question() {
        assert_eq!(prompt("\u{2771} 1. Yes\n".as_bytes()), None);
    }

    #[test]
    fn a_list_that_does_not_start_at_one_is_not_a_question() {
        assert_eq!(prompt("\u{2771} 2. Yes\n  3. No\n".as_bytes()), None);
    }

    #[test]
    fn two_cursors_is_not_a_selection() {
        assert_eq!(prompt("\u{2771} 1. Yes\n\u{2771} 2. No\n".as_bytes()), None);
    }

    #[test]
    fn a_decimal_number_in_prose_is_not_a_choice() {
        assert_eq!(parse_choice("1.5 seconds elapsed"), None);
        assert_eq!(parse_choice("  finished in 2.3s"), None);
    }

    /// What a person would actually read on the toast, spelled out.
    ///
    /// Asserted verbatim rather than by `contains`, because the whole feature
    /// is the sentence in this string. If it changes, somebody should have to
    /// look at the new one and agree it still reads well.
    #[test]
    fn the_previews_read_the_way_they_will_be_shown() {
        assert_eq!(
            prompt(CLAUDE_WRITE).unwrap().preview(120),
            "Do you want to create hello.txt?"
        );
        assert_eq!(
            prompt(CLAUDE_COMMAND).unwrap().preview(120),
            "Do you want to proceed? \u{2014} Bash command \u{b7} git --version"
        );
    }

    #[test]
    fn the_preview_never_splits_a_character() {
        let question = Prompt {
            question: "Você quer criar ação.txt?".into(),
            subject: vec![],
            options: vec![],
        };
        let preview = question.preview(10);
        assert!(preview.chars().count() <= 11, "{preview}");
    }
}
