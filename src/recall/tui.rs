use std::fs::File;
use std::io::Write;

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::{Color, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

use super::command::FilterMode;
use super::engine::{HistoryEntry, SearchEngine, format_relative_time};

const SCROLL_MARGIN: usize = 5;

/// Sanitize a string for safe terminal display by removing ANSI escape sequences
/// and control characters that could affect cursor position or terminal state.
fn sanitize_for_display(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            // ESC - start of ANSI escape sequence
            '\x1b' => {
                // Skip the escape sequence
                if let Some(&next) = chars.peek()
                    && next == '['
                {
                    chars.next(); // consume '['
                    // Skip until we hit a letter (end of CSI sequence)
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
            }
            // Newline, carriage return - would break row containment
            '\n' | '\r' => result.push(' '),
            // Other control characters that could affect display
            '\x00'..='\x08' | '\x0b'..='\x0c' | '\x0e'..='\x1f' | '\x7f' => {}
            // Tab - convert to space
            '\t' => result.push(' '),
            // Everything else passes through
            _ => result.push(c),
        }
    }

    result
}

pub struct RecallTui {
    engine: SearchEngine,
    filter_mode: FilterMode,
    entries: Vec<HistoryEntry>,
    filtered_indices: Vec<usize>,
    query: String,
    cursor_position: usize,
    selected_index: usize,
    scroll_offset: usize, // Index of entry at top of visible area
    tty: File,
    term_height: u16,
    term_width: u16,
}

impl RecallTui {
    pub fn new(
        engine: SearchEngine,
        initial_mode: FilterMode,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let entries = engine.load_entries(initial_mode)?;

        terminal::enable_raw_mode()?;
        let mut tty = File::options().read(true).write(true).open("/dev/tty")?;
        execute!(
            tty,
            EnterAlternateScreen,
            Hide,
            Clear(ClearType::All),
            Clear(ClearType::Purge),
            MoveTo(0, 0)
        )?;

        let (term_width, term_height) = terminal::size()?;

        // Explicitly clear every line to ensure no residual content
        for row in 0..term_height {
            execute!(tty, MoveTo(0, row), Clear(ClearType::CurrentLine))?;
        }
        tty.flush()?;

        let mut tui = RecallTui {
            engine,
            filter_mode: initial_mode,
            entries,
            filtered_indices: Vec::new(),
            query: String::new(),
            cursor_position: 0,
            selected_index: 0,
            scroll_offset: 0,
            tty,
            term_height,
            term_width,
        };

        tui.update_filtered_indices();
        tui.adjust_scroll_for_selection();
        Ok(tui)
    }

    fn results_height(&self) -> usize {
        self.term_height.saturating_sub(2) as usize
    }

    fn adjust_scroll_for_selection(&mut self) {
        let results_height = self.results_height();
        if results_height == 0 || self.filtered_indices.is_empty() {
            self.scroll_offset = 0;
            return;
        }

        // In our layout, entry 0 (most recent) is at the bottom visually.
        // scroll_offset is the entry index shown at the TOP of the visible area.
        // Higher scroll_offset means we're showing older entries.
        //
        // Visible range: scroll_offset.saturating_sub(results_height - 1) to scroll_offset
        // But actually, let's think of it differently:
        //   - The bottom of the visible area shows entry index `bottom_visible`
        //   - The top shows entry index `bottom_visible + results_height - 1`
        //
        // Let's use `view_bottom` as the entry index shown at the bottom of results area.
        // Visible entries: view_bottom to view_bottom + results_height - 1

        // Calculate current view bounds based on scroll_offset
        // scroll_offset represents the entry at the bottom of the visible area
        let view_bottom = self.scroll_offset;
        let view_top = view_bottom + results_height.saturating_sub(1);

        // Check if selected is within the visible range with margins
        if self.selected_index < view_bottom + SCROLL_MARGIN {
            // Selection is too close to bottom, scroll down (show newer entries)
            self.scroll_offset = self.selected_index.saturating_sub(SCROLL_MARGIN);
        } else if self.selected_index > view_top.saturating_sub(SCROLL_MARGIN) {
            // Selection is too close to top, scroll up (show older entries)
            let new_view_top = self.selected_index + SCROLL_MARGIN;
            self.scroll_offset = new_view_top.saturating_sub(results_height.saturating_sub(1));
        }

        // Clamp scroll_offset to valid range
        let max_scroll = self.filtered_indices.len().saturating_sub(results_height);
        self.scroll_offset = self.scroll_offset.min(max_scroll);
    }

    fn update_filtered_indices(&mut self) {
        if self.query.is_empty() {
            self.filtered_indices = (0..self.entries.len()).collect();
        } else {
            let query_lower = self.query.to_lowercase();
            self.filtered_indices = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.command.to_lowercase().contains(&query_lower))
                .map(|(i, _)| i)
                .collect();
        }

        if self.selected_index >= self.filtered_indices.len() {
            self.selected_index = 0;
        }
        self.adjust_scroll_for_selection();
    }

    fn reload_entries(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.entries = self.engine.load_entries(self.filter_mode)?;
        self.update_filtered_indices();
        Ok(())
    }

    fn toggle_filter_mode(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.filter_mode = match self.filter_mode {
            FilterMode::Directory => FilterMode::Global,
            FilterMode::Global => FilterMode::Directory,
        };
        self.reload_entries()
    }

    pub fn run(&mut self) -> Result<Option<String>, Box<dyn std::error::Error>> {
        loop {
            self.draw()?;

            if let Event::Key(key) = event::read()? {
                match self.handle_key(key)? {
                    KeyAction::Continue => continue,
                    KeyAction::Select => {
                        let result = self.get_selected_command();
                        self.cleanup()?;
                        return Ok(result);
                    }
                    KeyAction::Cancel => {
                        self.cleanup()?;
                        return Ok(None);
                    }
                }
            }
        }
    }

    fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        execute!(self.tty, Show, LeaveAlternateScreen)?;
        terminal::disable_raw_mode()?;
        Ok(())
    }

    fn get_selected_command(&self) -> Option<String> {
        self.filtered_indices
            .get(self.selected_index)
            .and_then(|&idx| self.entries.get(idx))
            .map(|e| e.command.clone())
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<KeyAction, Box<dyn std::error::Error>> {
        match key.code {
            KeyCode::Esc => Ok(KeyAction::Cancel),
            KeyCode::Enter => Ok(KeyAction::Select),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Ok(KeyAction::Cancel)
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_filter_mode()?;
                Ok(KeyAction::Continue)
            }
            KeyCode::Up | KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.selected_index + 1 < self.filtered_indices.len() {
                    self.selected_index += 1;
                    self.adjust_scroll_for_selection();
                }
                Ok(KeyAction::Continue)
            }
            KeyCode::Down | KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                    self.adjust_scroll_for_selection();
                }
                Ok(KeyAction::Continue)
            }
            KeyCode::Up => {
                if self.selected_index + 1 < self.filtered_indices.len() {
                    self.selected_index += 1;
                    self.adjust_scroll_for_selection();
                }
                Ok(KeyAction::Continue)
            }
            KeyCode::Down => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                    self.adjust_scroll_for_selection();
                }
                Ok(KeyAction::Continue)
            }
            KeyCode::Backspace => {
                if self.cursor_position > 0 {
                    self.query.remove(self.cursor_position - 1);
                    self.cursor_position -= 1;
                    self.update_filtered_indices();
                }
                Ok(KeyAction::Continue)
            }
            KeyCode::Delete => {
                if self.cursor_position < self.query.len() {
                    self.query.remove(self.cursor_position);
                    self.update_filtered_indices();
                }
                Ok(KeyAction::Continue)
            }
            KeyCode::Left => {
                if self.cursor_position > 0 {
                    self.cursor_position -= 1;
                }
                Ok(KeyAction::Continue)
            }
            KeyCode::Right => {
                if self.cursor_position < self.query.len() {
                    self.cursor_position += 1;
                }
                Ok(KeyAction::Continue)
            }
            KeyCode::Home | KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cursor_position = 0;
                Ok(KeyAction::Continue)
            }
            KeyCode::End | KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cursor_position = self.query.len();
                Ok(KeyAction::Continue)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.query = self.query[self.cursor_position..].to_string();
                self.cursor_position = 0;
                self.update_filtered_indices();
                Ok(KeyAction::Continue)
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.cursor_position > 0 {
                    let before_cursor = &self.query[..self.cursor_position];
                    let word_start = before_cursor
                        .trim_end()
                        .rfind(char::is_whitespace)
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    self.query = format!(
                        "{}{}",
                        &self.query[..word_start],
                        &self.query[self.cursor_position..]
                    );
                    self.cursor_position = word_start;
                    self.update_filtered_indices();
                }
                Ok(KeyAction::Continue)
            }
            KeyCode::Char(c) => {
                self.query.insert(self.cursor_position, c);
                self.cursor_position += 1;
                self.update_filtered_indices();
                Ok(KeyAction::Continue)
            }
            _ => Ok(KeyAction::Continue),
        }
    }

    fn draw(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Get fresh terminal size each frame
        let (term_width, term_height) = terminal::size()?;
        self.term_width = term_width;
        self.term_height = term_height;

        let results_height = term_height.saturating_sub(2) as usize;
        let input_y = term_height.saturating_sub(2);
        let help_y = term_height.saturating_sub(1);

        // Draw each line, clearing as we go (avoids full-screen clear flicker)
        // Results area: rows 0 to results_height-1
        // Layout: oldest at top (row 0), newest at bottom (row results_height-1)
        // scroll_offset is the entry index shown at the bottom of the visible area
        for row in 0..results_height {
            execute!(self.tty, MoveTo(0, row as u16), Clear(ClearType::CurrentLine))?;

            // Calculate which entry to show at this row
            // Row 0 (top) shows oldest visible entry
            // Row results_height-1 (bottom) shows entry at scroll_offset
            let offset_from_bottom = results_height - 1 - row;
            let entry_index = self.scroll_offset + offset_from_bottom;

            if entry_index >= self.filtered_indices.len() {
                continue;
            }

            let idx = self.filtered_indices[entry_index];
            let entry = &self.entries[idx];
            let time_str = format_relative_time(entry.timestamp);
            let is_selected = entry_index == self.selected_index;

            if is_selected {
                execute!(self.tty, SetBackgroundColor(Color::DarkGrey))?;
                write!(self.tty, "> ")?;
            } else {
                write!(self.tty, "  ")?;
            }

            execute!(self.tty, SetForegroundColor(Color::DarkGrey))?;
            write!(self.tty, "{time_str}  ")?;
            execute!(self.tty, ResetColor)?;

            if is_selected {
                execute!(self.tty, SetBackgroundColor(Color::DarkGrey))?;
            }

            // Sanitize and truncate command to fit (handle UTF-8 safely)
            let safe_cmd = sanitize_for_display(&entry.command);
            let prefix_len = 8; // "> " + "XXx  "
            let max_cmd_len = term_width.saturating_sub(prefix_len) as usize;
            let cmd: String = if safe_cmd.chars().count() > max_cmd_len {
                let truncated: String =
                    safe_cmd.chars().take(max_cmd_len.saturating_sub(3)).collect();
                format!("{truncated}...")
            } else {
                safe_cmd
            };
            write!(self.tty, "{cmd}")?;

            execute!(self.tty, ResetColor)?;
        }

        // Draw input line
        execute!(self.tty, MoveTo(0, input_y), Clear(ClearType::CurrentLine))?;
        write!(self.tty, "> {}", self.query)?;

        // Draw mode indicator on same line
        let mode_str = match self.filter_mode {
            FilterMode::Directory => {
                let dir = self.engine.working_directory();
                let name = dir
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "?".to_string());
                format!("[Dir: {name}]")
            }
            FilterMode::Global => "[Global]".to_string(),
        };
        let mode_x = term_width.saturating_sub(mode_str.len() as u16 + 1);
        execute!(self.tty, MoveTo(mode_x, input_y), SetForegroundColor(Color::Cyan))?;
        write!(self.tty, "{mode_str}")?;
        execute!(self.tty, ResetColor)?;

        // Draw help line
        execute!(self.tty, MoveTo(0, help_y), Clear(ClearType::CurrentLine))?;
        execute!(self.tty, SetForegroundColor(Color::DarkGrey))?;
        write!(self.tty, "↑↓ Navigate  Enter Select  Esc Cancel  ^R Toggle filter")?;
        execute!(self.tty, ResetColor)?;

        // Position cursor at end of query in input line
        execute!(self.tty, MoveTo(2 + self.cursor_position as u16, input_y))?;

        self.tty.flush()?;
        Ok(())
    }
}

enum KeyAction {
    Continue,
    Select,
    Cancel,
}

impl Drop for RecallTui {
    fn drop(&mut self) {
        let _ = execute!(self.tty, Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_preserves_normal_text() {
        assert_eq!(sanitize_for_display("hello world"), "hello world");
        assert_eq!(sanitize_for_display("ls -la /tmp"), "ls -la /tmp");
    }

    #[test]
    fn test_sanitize_preserves_box_drawing() {
        // Box-drawing characters should pass through
        assert_eq!(sanitize_for_display("┌History───┐"), "┌History───┐");
        assert_eq!(sanitize_for_display("│ cell │"), "│ cell │");
        assert_eq!(sanitize_for_display("└───────┘"), "└───────┘");
    }

    #[test]
    fn test_sanitize_preserves_unicode() {
        assert_eq!(sanitize_for_display("héllo wörld"), "héllo wörld");
        assert_eq!(sanitize_for_display("日本語"), "日本語");
        assert_eq!(sanitize_for_display("emoji 🎉 test"), "emoji 🎉 test");
    }

    #[test]
    fn test_sanitize_strips_ansi_escape_sequences() {
        // Color codes
        assert_eq!(sanitize_for_display("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(sanitize_for_display("\x1b[1;32mbold green\x1b[0m"), "bold green");

        // Cursor movement
        assert_eq!(sanitize_for_display("\x1b[H"), ""); // cursor home
        assert_eq!(sanitize_for_display("\x1b[2J"), ""); // clear screen
        assert_eq!(sanitize_for_display("\x1b[10;20H"), ""); // cursor position

        // Mixed content
        assert_eq!(sanitize_for_display("before\x1b[31mred\x1b[0mafter"), "beforeredafter");
    }

    #[test]
    fn test_sanitize_converts_newlines_to_spaces() {
        assert_eq!(sanitize_for_display("line1\nline2"), "line1 line2");
        assert_eq!(sanitize_for_display("line1\r\nline2"), "line1  line2");
        assert_eq!(sanitize_for_display("a\nb\nc"), "a b c");
    }

    #[test]
    fn test_sanitize_converts_tabs_to_spaces() {
        assert_eq!(sanitize_for_display("col1\tcol2"), "col1 col2");
        assert_eq!(sanitize_for_display("\t\tindented"), "  indented");
    }

    #[test]
    fn test_sanitize_strips_control_characters() {
        // Bell, backspace, etc.
        assert_eq!(sanitize_for_display("hello\x07world"), "helloworld"); // bell
        assert_eq!(sanitize_for_display("hello\x08world"), "helloworld"); // backspace
        assert_eq!(sanitize_for_display("a\x00b\x01c"), "abc"); // null and other low controls
        assert_eq!(sanitize_for_display("test\x7fdelete"), "testdelete"); // DEL
    }

    #[test]
    fn test_sanitize_handles_binary_garbage() {
        // Simulate binary data that might corrupt terminal
        let binary_garbage = "cmd\x1b[2J\x1b[H\x00\x01\x02\x03visible\x1b[31m";
        assert_eq!(sanitize_for_display(binary_garbage), "cmdvisible");
    }

    #[test]
    fn test_sanitize_handles_incomplete_escape_sequences() {
        // Incomplete escape at end of string
        assert_eq!(sanitize_for_display("text\x1b"), "text");
        assert_eq!(sanitize_for_display("text\x1b["), "text");
        assert_eq!(sanitize_for_display("text\x1b[123"), "text");
    }

    #[test]
    fn test_sanitize_empty_string() {
        assert_eq!(sanitize_for_display(""), "");
    }
}
