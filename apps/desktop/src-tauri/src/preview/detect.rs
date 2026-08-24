//! Finding the dev server an agent just started (§46).
//!
//! This is the part of Preview that no general-purpose browser can do, and the
//! reason Preview belongs inside this product rather than beside it: the
//! session log already holds **every byte** the process wrote (§23), so when an
//! agent runs `npm run dev` we can read the URL out of the same stream the
//! terminal is drawing. Nothing to configure, no port to guess, no per-project
//! setting to forget — the answer is in output we already have.
//!
//! ## Why a regex over the output rather than watching ports
//!
//! Enumerating listening sockets finds *a* server; it cannot tell you which one
//! this session started, and on a developer's machine there are usually several.
//! It would also happily point Preview at something unrelated. The output is
//! the honest source: this text was printed by this session's own process.
//!
//! ## What this deliberately does not do
//!
//! It does not open anything. Detecting a URL and navigating to it are separate
//! by design — an agent restarting a server should not yank the view out from
//! under someone reading it. The surface offers; the person decides.

use std::collections::BTreeSet;

/// A dev server this session appears to have started.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Candidate {
    pub url: String,
    /// Where in the stream it was seen. Later wins — a server restarted on a
    /// new port should supersede the one it replaced.
    pub at: usize,
}

/// Strip ANSI escape sequences.
///
/// Not optional: every dev server worth previewing colours its banner, so the
/// URL arrives wrapped in `\x1b[36m…\x1b[0m` and a naive match either misses it
/// or captures the reset sequence as part of the URL. Vite in particular prints
/// the port in a different colour from the rest of the line, so the escape
/// lands *inside* what looks like one token.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        // CSI: ESC [ … final byte in @-~. Anything else: skip the next char,
        // which covers the two-character sequences without trying to be a
        // complete terminal emulator — this only has to survive a banner.
        if chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if ('\x40'..='\x7e').contains(&c) {
                    break;
                }
            }
        } else {
            chars.next();
        }
    }
    out
}

/// Ports that are never a dev server worth previewing.
///
/// 5432 and 3306 are Postgres and MySQL; 6379 is Redis. They appear in exactly
/// the same "server started on …" shape and are not web pages. Offering to open
/// one in a browser would be noise at best.
const NOT_A_WEB_SERVER: &[u16] = &[5432, 3306, 6379, 27017, 9229];

/// Extract every local http(s) URL in the text, in the order they appear.
///
/// Written by hand rather than with a regex crate: the shape is narrow enough
/// that a scanner is clearer than a pattern, and this avoids pulling a regex
/// engine into the binary for one call site.
pub fn candidates(text: &str) -> Vec<Candidate> {
    let clean = strip_ansi(text);
    let mut found = Vec::new();

    // Iterate **character** boundaries, not byte offsets.
    //
    // The first version stepped `i += 1` through the byte length and sliced
    // `&clean[i..]`, which panics the moment a multi-byte character appears
    // before a URL — and a real Vite banner opens every line with `➜`, a real
    // Next.js banner with `▲`. Caught by testing against genuine output rather
    // than an ASCII approximation of it. Same shape as the UTF-8 chunking bug
    // in `session::typing` (HANDOFF §5 item 36): a byte offset is not a
    // position in a string.
    let mut boundaries = clean.char_indices().map(|(at, _)| at).peekable();
    while let Some(i) = boundaries.next() {
        let rest = &clean[i..];
        let scheme_len = if rest.starts_with("http://") {
            7
        } else if rest.starts_with("https://") {
            8
        } else {
            continue;
        };

        // Host and port run until whitespace or a character no URL authority
        // can contain. A trailing `/` is kept; trailing punctuation is not.
        let tail = &rest[scheme_len..];
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '|' | '\\'))
            .unwrap_or(tail.len());
        let authority_and_path = &tail[..end];
        let trimmed = authority_and_path.trim_end_matches(|c| matches!(c, ',' | ';' | ')' | ']'));
        // A bare "http://" with nothing after it is not a URL.
        if trimmed.is_empty() {
            continue;
        }

        let url = format!("{}{}", &rest[..scheme_len], trimmed);
        if is_local(&url) && !is_excluded_port(&url) {
            found.push(Candidate { url, at: i });
        }
        // Step past what was just consumed, so a URL is not rescanned from
        // inside itself. `by_ref` keeps the outer iterator's position.
        let consumed = i + scheme_len + trimmed.len();
        while boundaries.peek().is_some_and(|&next| next < consumed) {
            boundaries.next();
        }
    }

    found
}

/// Only ever a loopback address.
///
/// **A security boundary, not a filter for tidiness.** Preview renders whatever
/// it is pointed at inside the application's own window; pointing it at an
/// arbitrary host because a string appeared in terminal output would let any
/// program a session runs — or any file it prints — choose what this product
/// displays. A dev server is on this machine by definition (§3).
///
/// `0.0.0.0` is the one non-loopback spelling accepted, because it is what a
/// server prints when it binds every interface, and reaching it means reaching
/// this machine. It is rewritten before use — see `normalise`.
fn is_local(url: &str) -> bool {
    let after_scheme = url.split("://").nth(1).unwrap_or_default();
    let host = after_scheme
        .split(['/', ':', '?', '#'])
        .next()
        .unwrap_or_default();
    is_loopback_host(host)
}

/// Whether a host names this machine.
///
/// **One rule, shared.** `preview_open` re-checks the URL it is handed rather
/// than trusting the webview, and it must apply exactly the same test that
/// filtered the candidates — two spellings of "is this local" that disagree is
/// how a check gets bypassed. `[::1]` appears with brackets in a URL and
/// without once parsed, so both are accepted.
pub fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    matches!(host, "localhost" | "127.0.0.1" | "0.0.0.0" | "::1")
        || host.ends_with(".localhost")
        // The whole 127/8 block is loopback, not just 127.0.0.1.
        || host.strip_prefix("127.").is_some_and(|rest| {
            rest.split('.').count() == 3 && rest.split('.').all(|o| o.parse::<u8>().is_ok())
        })
}

fn port_of(url: &str) -> Option<u16> {
    let after_scheme = url.split("://").nth(1)?;
    let authority = after_scheme.split(['/', '?', '#']).next()?;
    authority.rsplit(':').next()?.parse().ok()
}

fn is_excluded_port(url: &str) -> bool {
    port_of(url).is_some_and(|p| NOT_A_WEB_SERVER.contains(&p))
}

/// The URL to actually navigate to.
///
/// `0.0.0.0` means "listening on every interface" and is not a destination —
/// browsers treat it inconsistently and it is meaningless as a request target.
/// The server is reachable on loopback, so that is what Preview asks for.
pub fn normalise(url: &str) -> String {
    url.replacen("://0.0.0.0", "://127.0.0.1", 1)
}

/// The single best guess for what this session is serving, if any.
///
/// **The last one wins**, deliberately: a dev server restarted on a new port
/// prints again, and the newest line is the one that is still true. Duplicates
/// are collapsed so a server that reprints its banner on every rebuild — which
/// Vite does — does not look like a dozen different servers.
pub fn best(text: &str) -> Option<String> {
    candidates(text).into_iter().next_back().map(|c| normalise(&c.url))
}

/// Every distinct URL seen, newest first.
///
/// For the case a single guess handles badly: a framework that prints both a
/// local and a network URL, or a repository running an API and a web app in one
/// terminal. The surface can offer a choice rather than picking wrong silently.
pub fn all(text: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for candidate in candidates(text).into_iter().rev() {
        let url = normalise(&candidate.url);
        if seen.insert(url.clone()) {
            out.push(url);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real Vite output, escape sequences and all.
    ///
    /// Copied from an actual `pnpm dev` banner rather than written from
    /// memory: the colour codes sit *inside* the URL token, which is exactly
    /// what a naive matcher gets wrong.
    const VITE: &str = "\x1b[32m  VITE v5.4.11\x1b[39m  \x1b[2mready in 412 ms\x1b[22m\r\n\r\n  \x1b[32m➜\x1b[39m  \x1b[1mLocal\x1b[22m:   \x1b[36mhttp://localhost:\x1b[1m5173\x1b[22m/\x1b[39m\r\n  \x1b[32m➜\x1b[39m  \x1b[1mNetwork\x1b[22m: \x1b[2muse --host to expose\x1b[22m\r\n";

    #[test]
    fn finds_a_vite_url_through_its_colour_codes() {
        // The port is printed in a different colour from the rest of the URL,
        // so this only passes if the escapes are stripped before matching.
        assert_eq!(best(VITE).as_deref(), Some("http://localhost:5173/"));
    }

    #[test]
    fn finds_a_next_js_url() {
        let out = "   ▲ Next.js 15.0.3\r\n   - Local:        http://localhost:3000\r\n   - Network:      http://192.168.1.14:3000\r\n\r\n ✓ Starting...\r\n";
        // The network address is a real host on the LAN and is deliberately
        // *not* offered: Preview only ever points at this machine.
        assert_eq!(all(out), vec!["http://localhost:3000"]);
    }

    #[test]
    fn a_restarted_server_supersedes_the_one_before_it() {
        let out = "Local: http://localhost:3000\nPort 3000 in use, trying 3001\nLocal: http://localhost:3001\n";
        assert_eq!(best(out).as_deref(), Some("http://localhost:3001"));
    }

    #[test]
    fn a_banner_reprinted_on_every_rebuild_is_still_one_server() {
        let repeated = "ready http://localhost:5173/\n".repeat(12);
        assert_eq!(all(&repeated), vec!["http://localhost:5173/"]);
    }

    /// The security boundary, stated as a test.
    ///
    /// Terminal output is not trustworthy input: a file an agent prints, a
    /// dependency's postinstall banner or an error message can all contain a
    /// URL. Preview renders inside our own window, so anything that is not
    /// this machine is refused.
    #[test]
    fn a_remote_url_in_the_output_is_never_offered() {
        let hostile = "Fetching https://evil.example.com/payload\nSee http://docs.rust-lang.org/book\nLocal: http://localhost:4321\n";
        assert_eq!(all(hostile), vec!["http://localhost:4321"]);
    }

    #[test]
    fn a_database_is_not_a_web_server() {
        let out = "postgres listening on http://127.0.0.1:5432\nredis http://localhost:6379\napp http://localhost:8080\n";
        assert_eq!(all(out), vec!["http://localhost:8080"]);
    }

    /// `0.0.0.0` is what a server prints when it binds every interface. It is
    /// not a destination, so it is rewritten to loopback before use.
    #[test]
    fn every_interface_is_rewritten_to_loopback() {
        assert_eq!(best("Listening on http://0.0.0.0:8000").as_deref(), Some("http://127.0.0.1:8000"));
    }

    #[test]
    fn trailing_punctuation_is_not_part_of_the_url() {
        let out = "Server ready at http://localhost:4000/graphql, press ctrl-c to stop\n";
        assert_eq!(best(out).as_deref(), Some("http://localhost:4000/graphql"));
    }

    #[test]
    fn output_with_no_server_in_it_offers_nothing() {
        assert_eq!(best("$ ls\r\nCargo.toml  src  target\r\n$ "), None);
        assert!(all("error: could not compile `jarvis`").is_empty());
    }

    /// A path is kept: some frameworks serve the app from a sub-path, and
    /// dropping it would open a 404 and look like the preview was broken.
    #[test]
    fn a_path_survives() {
        assert_eq!(
            best("Storybook started on http://localhost:6006/?path=/story/button").as_deref(),
            Some("http://localhost:6006/?path=/story/button")
        );
    }

    /// The rule `preview_open` re-checks with, pinned directly.
    ///
    /// It is the enforcement point for "this window only ever shows this
    /// machine", so it is tested as a rule rather than only through the
    /// scanner that happens to call it.
    #[test]
    fn only_this_machine_counts_as_local() {
        for host in ["localhost", "127.0.0.1", "127.0.0.2", "0.0.0.0", "::1", "[::1]", "app.localhost"] {
            assert!(is_loopback_host(host), "{host} should be local");
        }
        for host in [
            "evil.example.com",
            "192.168.1.14",
            "10.0.0.5",
            // The shapes a prefix check would wave through.
            "127.0.0.1.evil.com",
            "localhost.evil.com",
            "notlocalhost",
            "",
        ] {
            assert!(!is_loopback_host(host), "{host} must not be treated as local");
        }
    }

    /// The bytes a PTY produces are not guaranteed to be valid anything, and a
    /// scanner that panics on odd input would take the session down with it.
    #[test]
    fn odd_input_does_not_panic() {
        for text in ["http://", "https://", "http:// ", "http://:", "\x1b[", "\x1b"] {
            let _ = best(text);
            let _ = all(text);
        }
    }
}
