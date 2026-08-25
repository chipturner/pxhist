//! How pxh talks to a human.
//!
//! Every diagnostic line pxh prints -- as opposed to history rows, JSON, or
//! shell-config text, which are *data* -- goes through here, so a message's
//! severity is a named thing rather than a prefix typed at the call site.
//!
//! Styling is always emitted by `render`; whether it survives is the sink's
//! decision. The sinks print through `anstream`, which strips ANSI when stderr
//! is not a terminal and honors `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`, and
//! `TERM=dumb`. stdout is reserved for data and is never written from here.

/// What a message *is*. Decides the color and the leading word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    /// Something is wrong but the command continues.
    Warn,
    /// The command failed.
    Error,
    /// A suggestion subordinate to a nearby line (`run pxh maintenance ...`).
    Hint,
}

/// The palette. Nothing outside this module spells an SGR sequence.
pub mod sgr {
    pub const YELLOW: &str = "\x1b[33m";
    pub const RED: &str = "\x1b[31m";
    pub const DIM: &str = "\x1b[2m";
    pub const RESET: &str = "\x1b[0m";
}

use sgr::{DIM, RED, RESET, YELLOW};

/// Render one message: `<word>: <text>`, colored. Pure, so the vocabulary is
/// testable without a terminal.
fn render(level: Level, text: &str) -> String {
    let (sgr, word) = match level {
        Level::Warn => (YELLOW, "warning"),
        Level::Error => (RED, "error"),
        Level::Hint => (DIM, "hint"),
    };
    format!("{sgr}{word}: {text}{RESET}")
}

fn emit(level: Level, text: &str) {
    anstream::eprintln!("{}", render(level, text));
}

/// Something is wrong; the command continues.
pub fn warn(text: &str) {
    emit(Level::Warn, text);
}

/// The command failed. Rendered `error: <text>`.
pub fn error(text: &str) {
    emit(Level::Error, text);
}

/// A suggestion: `hint: run pxh maintenance to reclaim disk space`.
pub fn hint(text: &str) {
    emit(Level::Hint, text);
}

/// `1 secret` / `3 secrets` -- every counted noun pxh prints goes through
/// here, so nothing says `secret(s)`. Nouns pluralize with a plain `s`; that
/// is true of everything pxh counts (secret, issue, entry, command, day, hour).
pub fn count(n: usize, noun: &str) -> String {
    if n == 1 { format!("1 {noun}") } else { format!("{n} {noun}s") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_pluralizes_everything_but_one() {
        assert_eq!(count(0, "secret"), "0 secrets");
        assert_eq!(count(1, "secret"), "1 secret");
        assert_eq!(count(2, "issue"), "2 issues");
    }

    #[test]
    fn levels_say_the_lowercase_word() {
        assert_eq!(render(Level::Error, "boom"), format!("{RED}error: boom{RESET}"));
        assert_eq!(render(Level::Warn, "hmm"), format!("{YELLOW}warning: hmm{RESET}"));
        assert_eq!(render(Level::Hint, "try x"), format!("{DIM}hint: try x{RESET}"));
    }

    /// Whatever a sink later decides, the rendered bytes must strip back to
    /// exactly the text a non-terminal should see.
    #[test]
    fn stripping_a_rendered_line_leaves_plain_text() {
        let styled = render(Level::Error, "unable to open database");
        let plain = anstream::adapter::strip_str(&styled).to_string();
        assert_eq!(plain, "error: unable to open database");
    }
}
