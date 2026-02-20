//! Shared colored output utilities for CLI commands.
//!
//! Uses `termcolor` for cross-platform colored terminal output.
//! Respects `NO_COLOR` environment variable and `--color` flag.

use std::io::Write;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

/// Resolve `ColorChoice` from CLI flag and environment.
///
/// Priority: `NO_COLOR` env > `--color` flag > auto-detect TTY.
pub fn resolve_color_choice(flag: Option<&str>) -> ColorChoice {
    if std::env::var_os("NO_COLOR").is_some() {
        return ColorChoice::Never;
    }
    match flag {
        Some("always") => ColorChoice::Always,
        Some("never") => ColorChoice::Never,
        _ => ColorChoice::Auto,
    }
}

/// Styled output writer for terminal.
#[allow(dead_code)]
pub struct StyledOutput {
    stdout: StandardStream,
    stderr: StandardStream,
}

#[allow(dead_code)]
impl StyledOutput {
    /// Create a new styled output with the given color choice.
    pub fn new(choice: ColorChoice) -> Self {
        Self {
            stdout: StandardStream::stdout(choice),
            stderr: StandardStream::stderr(choice),
        }
    }

    // ── Generic styled writes ────────────────────────────────────────

    /// Write text with a specific color and style.
    pub fn write_styled(
        &mut self,
        text: &str,
        color: Option<Color>,
        bold: bool,
        intense: bool,
    ) {
        let mut spec = ColorSpec::new();
        spec.set_fg(color).set_bold(bold).set_intense(intense);
        let _ = self.stdout.set_color(&spec);
        let _ = write!(self.stdout, "{}", text);
        let _ = self.stdout.reset();
    }

    /// Write text followed by newline with a specific color and style.
    pub fn writeln_styled(
        &mut self,
        text: &str,
        color: Option<Color>,
        bold: bool,
        intense: bool,
    ) {
        let mut spec = ColorSpec::new();
        spec.set_fg(color).set_bold(bold).set_intense(intense);
        let _ = self.stdout.set_color(&spec);
        let _ = writeln!(self.stdout, "{}", text);
        let _ = self.stdout.reset();
    }

    // ── Convenience helpers ──────────────────────────────────────────

    /// Green bold text.
    pub fn success(&mut self, text: &str) {
        self.write_styled(text, Some(Color::Green), true, false);
    }

    /// Red bold text.
    pub fn error(&mut self, text: &str) {
        self.write_styled(text, Some(Color::Red), true, false);
    }

    /// Yellow bold text.
    pub fn warning(&mut self, text: &str) {
        self.write_styled(text, Some(Color::Yellow), true, false);
    }

    /// Cyan text.
    pub fn info(&mut self, text: &str) {
        self.write_styled(text, Some(Color::Cyan), false, false);
    }

    /// Dim/gray text.
    pub fn dim(&mut self, text: &str) {
        self.write_styled(text, Some(Color::White), false, false);
    }

    /// Bold white text.
    pub fn bold(&mut self, text: &str) {
        self.write_styled(text, None, true, false);
    }

    /// Plain text (no color).
    pub fn plain(&mut self, text: &str) {
        let _ = write!(self.stdout, "{}", text);
    }

    /// Newline.
    pub fn newline(&mut self) {
        let _ = writeln!(self.stdout);
    }

    /// Flush stdout.
    pub fn flush(&mut self) {
        let _ = self.stdout.flush();
    }

    // ── Test-specific badges ─────────────────────────────────────────

    /// " PASS " badge (green background, white text).
    pub fn pass_badge(&mut self) {
        let mut spec = ColorSpec::new();
        spec.set_bg(Some(Color::Green))
            .set_fg(Some(Color::White))
            .set_bold(true);
        let _ = self.stdout.set_color(&spec);
        let _ = write!(self.stdout, " PASS ");
        let _ = self.stdout.reset();
    }

    /// " FAIL " badge (red background, white text).
    pub fn fail_badge(&mut self) {
        let mut spec = ColorSpec::new();
        spec.set_bg(Some(Color::Red))
            .set_fg(Some(Color::White))
            .set_bold(true);
        let _ = self.stdout.set_color(&spec);
        let _ = write!(self.stdout, " FAIL ");
        let _ = self.stdout.reset();
    }

    /// " SKIP " badge (yellow background, black text).
    pub fn skip_badge(&mut self) {
        let mut spec = ColorSpec::new();
        spec.set_bg(Some(Color::Yellow))
            .set_fg(Some(Color::Black))
            .set_bold(true);
        let _ = self.stdout.set_color(&spec);
        let _ = write!(self.stdout, " SKIP ");
        let _ = self.stdout.reset();
    }

    // ── Error output (stderr) ────────────────────────────────────────

    /// Write error message to stderr.
    pub fn stderr_error(&mut self, text: &str) {
        let mut spec = ColorSpec::new();
        spec.set_fg(Some(Color::Red)).set_bold(true);
        let _ = self.stderr.set_color(&spec);
        let _ = write!(self.stderr, "{}", text);
        let _ = self.stderr.reset();
    }
}
