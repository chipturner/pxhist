use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::Duration;

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute, queue,
    style::{Color, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

#[cfg(not(target_os = "windows"))]
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};

use super::command::{FilterMode, HostFilter};
use super::config::{KeymapMode, PreviewConfig, RecallConfig};
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

const PREVIEW_HEIGHT: usize = 5; // Height of preview pane in lines

pub struct RecallTui {
    engine: SearchEngine,
    filter_mode: FilterMode,
    host_filter: HostFilter,
    entries: Vec<HistoryEntry>,
    filtered_indices: Vec<usize>,
    query: String,
    cursor_position: usize,
    selected_index: usize,
    scroll_offset: usize, // Index of entry at top of visible area
    tty: File,
    term_height: u16,
    term_width: u16,
    keymap_mode: KeymapMode,
    show_preview: bool,
    preview_config: PreviewConfig,
    #[cfg(not(target_os = "windows"))]
    keyboard_enhanced: bool,
}

impl RecallTui {
    pub fn new(
        engine: SearchEngine,
        initial_mode: FilterMode,
        initial_query: Option<String>,
        config: &RecallConfig,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let query = initial_query.as_deref().unwrap_or("");
        let query_for_load = if query.is_empty() { None } else { Some(query) };
        let host_filter = HostFilter::default();
        let entries = engine.load_entries(initial_mode, host_filter, query_for_load)?;
        let query = query.to_string();

        terminal::enable_raw_mode()?;
        let mut tty = File::options().read(true).write(true).open("/dev/tty")?;

        // Enable keyboard enhancement for instant Escape key response (non-Windows)
        #[cfg(not(target_os = "windows"))]
        let keyboard_enhanced = execute!(
            tty,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )
        .is_ok();

        execute!(
            tty,
            EnterAlternateScreen,
            Hide,
            Clear(ClearType::All),
            Clear(ClearType::Purge),
            MoveTo(0, 0)
        )?;
        tty.flush()?;

        let (term_width, term_height) = terminal::size()?;

        let cursor_position = query.len();

        let filtered_indices = (0..entries.len()).collect();

        let mut tui = RecallTui {
            engine,
            filter_mode: initial_mode,
            host_filter,
            entries,
            filtered_indices,
            query,
            cursor_position,
            selected_index: 0,
            scroll_offset: 0,
            tty,
            term_height,
            term_width,
            keymap_mode: config.initial_keymap_mode(),
            show_preview: config.show_preview,
            preview_config: config.preview.clone(),
            #[cfg(not(target_os = "windows"))]
            keyboard_enhanced,
        };

        tui.adjust_scroll_for_selection();
        Ok(tui)
    }

    fn results_height(&self) -> usize {
        let base = self.term_height.saturating_sub(2) as usize;
        if self.show_preview { base.saturating_sub(PREVIEW_HEIGHT) } else { base }
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
        // Re-query the database with the current search query
        // This ensures we search the full database, not just a cached subset
        let query = if self.query.is_empty() { None } else { Some(self.query.as_str()) };
        if let Ok(entries) = self.engine.load_entries(self.filter_mode, self.host_filter, query) {
            self.entries = entries;
        }
        // All loaded entries match the query (filtered in SQL)
        self.filtered_indices = (0..self.entries.len()).collect();

        if self.selected_index >= self.filtered_indices.len() {
            self.selected_index = 0;
        }
        self.adjust_scroll_for_selection();
    }

    fn toggle_host_filter(&mut self) {
        self.host_filter = match self.host_filter {
            HostFilter::ThisHost => HostFilter::AllHosts,
            HostFilter::AllHosts => HostFilter::ThisHost,
        };
        self.update_filtered_indices();
    }

    pub fn run(&mut self) -> Result<Option<String>, Box<dyn std::error::Error>> {
        loop {
            self.draw()?;

            // Poll with timeout for responsive cancellation and future async features
            if !event::poll(Duration::from_millis(100))? {
                continue;
            }

            if let Event::Key(key) = event::read()? {
                match self.handle_key(key)? {
                    KeyAction::Continue => continue,
                    KeyAction::Select => {
                        let result = self.get_selected_command().map(|cmd| format!("run:{cmd}"));
                        self.cleanup()?;
                        return Ok(result);
                    }
                    KeyAction::Edit => {
                        let result = self.get_selected_command().map(|cmd| format!("edit:{cmd}"));
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

    /// Draw once and exit (for profiling)
    pub fn draw_once(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.draw()?;
        self.cleanup()?;
        Ok(())
    }

    fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(not(target_os = "windows"))]
        if self.keyboard_enhanced {
            let _ = execute!(self.tty, PopKeyboardEnhancementFlags);
        }
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
        match self.keymap_mode {
            KeymapMode::Emacs => self.handle_key_emacs(key),
            KeymapMode::VimInsert => self.handle_key_vim_insert(key),
            KeymapMode::VimNormal => self.handle_key_vim_normal(key),
        }
    }

    /// Handle common keys that work in all modes
    fn handle_common_key(&mut self, key: KeyEvent) -> Option<KeyAction> {
        match key.code {
            KeyCode::Enter => Some(KeyAction::Select),
            KeyCode::Tab => Some(KeyAction::Edit),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(KeyAction::Cancel)
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection_up();
                Some(KeyAction::Continue)
            }
            KeyCode::Up => {
                self.move_selection_up();
                Some(KeyAction::Continue)
            }
            KeyCode::Down => {
                self.move_selection_down();
                Some(KeyAction::Continue)
            }
            KeyCode::PageUp => {
                self.page_up();
                Some(KeyAction::Continue)
            }
            KeyCode::PageDown => {
                self.page_down();
                Some(KeyAction::Continue)
            }
            KeyCode::Char(c @ '1'..='9') if key.modifiers.contains(KeyModifiers::ALT) => {
                let num = c.to_digit(10).unwrap() as usize;
                // Alt-1 selects current, Alt-2 selects next older, etc.
                let target_index = self.selected_index + (num - 1);
                if target_index < self.filtered_indices.len() {
                    self.selected_index = target_index;
                    return Some(KeyAction::Select);
                }
                Some(KeyAction::Continue)
            }
            KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_host_filter();
                Some(KeyAction::Continue)
            }
            _ => None,
        }
    }

    fn handle_key_emacs(&mut self, key: KeyEvent) -> Result<KeyAction, Box<dyn std::error::Error>> {
        // Check common keys first
        if let Some(action) = self.handle_common_key(key) {
            return Ok(action);
        }

        match key.code {
            KeyCode::Esc => Ok(KeyAction::Cancel),
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection_up();
                Ok(KeyAction::Continue)
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection_down();
                Ok(KeyAction::Continue)
            }
            KeyCode::Backspace => {
                self.delete_char_before_cursor();
                Ok(KeyAction::Continue)
            }
            KeyCode::Delete => {
                self.delete_char_at_cursor();
                Ok(KeyAction::Continue)
            }
            KeyCode::Left => {
                self.move_cursor_left();
                Ok(KeyAction::Continue)
            }
            KeyCode::Right => {
                self.move_cursor_right();
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
                self.delete_to_line_start();
                Ok(KeyAction::Continue)
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.delete_word_before_cursor();
                Ok(KeyAction::Continue)
            }
            KeyCode::Char(c) => {
                self.insert_char(c);
                Ok(KeyAction::Continue)
            }
            _ => Ok(KeyAction::Continue),
        }
    }

    fn handle_key_vim_insert(
        &mut self,
        key: KeyEvent,
    ) -> Result<KeyAction, Box<dyn std::error::Error>> {
        // Check common keys first
        if let Some(action) = self.handle_common_key(key) {
            return Ok(action);
        }

        match key.code {
            KeyCode::Esc => {
                // Switch to normal mode
                self.keymap_mode = KeymapMode::VimNormal;
                // Move cursor back one if not at start (vim behavior)
                if self.cursor_position > 0 {
                    self.cursor_position -= 1;
                }
                Ok(KeyAction::Continue)
            }
            KeyCode::Backspace => {
                self.delete_char_before_cursor();
                Ok(KeyAction::Continue)
            }
            KeyCode::Delete => {
                self.delete_char_at_cursor();
                Ok(KeyAction::Continue)
            }
            KeyCode::Left => {
                self.move_cursor_left();
                Ok(KeyAction::Continue)
            }
            KeyCode::Right => {
                self.move_cursor_right();
                Ok(KeyAction::Continue)
            }
            KeyCode::Home => {
                self.cursor_position = 0;
                Ok(KeyAction::Continue)
            }
            KeyCode::End => {
                self.cursor_position = self.query.len();
                Ok(KeyAction::Continue)
            }
            KeyCode::Char(c) => {
                self.insert_char(c);
                Ok(KeyAction::Continue)
            }
            _ => Ok(KeyAction::Continue),
        }
    }

    fn handle_key_vim_normal(
        &mut self,
        key: KeyEvent,
    ) -> Result<KeyAction, Box<dyn std::error::Error>> {
        // Check common keys first
        if let Some(action) = self.handle_common_key(key) {
            return Ok(action);
        }

        match key.code {
            KeyCode::Esc => Ok(KeyAction::Cancel),
            // Navigation in results
            KeyCode::Char('j') => {
                self.move_selection_down();
                Ok(KeyAction::Continue)
            }
            KeyCode::Char('k') => {
                self.move_selection_up();
                Ok(KeyAction::Continue)
            }
            // Cursor movement in query
            KeyCode::Char('h') | KeyCode::Left => {
                self.move_cursor_left();
                Ok(KeyAction::Continue)
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.move_cursor_right();
                Ok(KeyAction::Continue)
            }
            KeyCode::Char('0') | KeyCode::Home => {
                self.cursor_position = 0;
                Ok(KeyAction::Continue)
            }
            KeyCode::Char('$') | KeyCode::End => {
                self.cursor_position = self.query.len().saturating_sub(1).max(0);
                Ok(KeyAction::Continue)
            }
            KeyCode::Char('w') => {
                self.move_cursor_word_forward();
                Ok(KeyAction::Continue)
            }
            KeyCode::Char('b') => {
                self.move_cursor_word_backward();
                Ok(KeyAction::Continue)
            }
            // Enter insert mode
            KeyCode::Char('i') => {
                self.keymap_mode = KeymapMode::VimInsert;
                Ok(KeyAction::Continue)
            }
            KeyCode::Char('a') => {
                self.keymap_mode = KeymapMode::VimInsert;
                if self.cursor_position < self.query.len() {
                    self.cursor_position += 1;
                }
                Ok(KeyAction::Continue)
            }
            KeyCode::Char('A') => {
                self.keymap_mode = KeymapMode::VimInsert;
                self.cursor_position = self.query.len();
                Ok(KeyAction::Continue)
            }
            KeyCode::Char('I') => {
                self.keymap_mode = KeymapMode::VimInsert;
                self.cursor_position = 0;
                Ok(KeyAction::Continue)
            }
            // Delete operations
            KeyCode::Char('x') => {
                self.delete_char_at_cursor();
                Ok(KeyAction::Continue)
            }
            KeyCode::Char('X') => {
                self.delete_char_before_cursor();
                Ok(KeyAction::Continue)
            }
            _ => Ok(KeyAction::Continue),
        }
    }

    // Helper methods for cursor/selection movement

    fn move_selection_up(&mut self) {
        if self.selected_index + 1 < self.filtered_indices.len() {
            self.selected_index += 1;
            self.adjust_scroll_for_selection();
        }
    }

    fn move_selection_down(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
            self.adjust_scroll_for_selection();
        }
    }

    fn page_up(&mut self) {
        let page = self.results_height().saturating_sub(2);
        let max_index = self.filtered_indices.len().saturating_sub(1);
        self.selected_index = (self.selected_index + page).min(max_index);
        self.adjust_scroll_for_selection();
    }

    fn page_down(&mut self) {
        let page = self.results_height().saturating_sub(2);
        self.selected_index = self.selected_index.saturating_sub(page);
        self.adjust_scroll_for_selection();
    }

    fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    fn move_cursor_right(&mut self) {
        if self.cursor_position < self.query.len() {
            self.cursor_position += 1;
        }
    }

    fn move_cursor_word_forward(&mut self) {
        let chars: Vec<char> = self.query.chars().collect();
        let mut pos = self.cursor_position;
        // Skip current word (non-whitespace)
        while pos < chars.len() && !chars[pos].is_whitespace() {
            pos += 1;
        }
        // Skip whitespace
        while pos < chars.len() && chars[pos].is_whitespace() {
            pos += 1;
        }
        self.cursor_position = pos;
    }

    fn move_cursor_word_backward(&mut self) {
        let chars: Vec<char> = self.query.chars().collect();
        let mut pos = self.cursor_position.saturating_sub(1);
        // Skip whitespace
        while pos > 0 && chars[pos].is_whitespace() {
            pos -= 1;
        }
        // Skip word (non-whitespace)
        while pos > 0 && !chars[pos - 1].is_whitespace() {
            pos -= 1;
        }
        self.cursor_position = pos;
    }

    fn insert_char(&mut self, c: char) {
        self.query.insert(self.cursor_position, c);
        self.cursor_position += 1;
        self.update_filtered_indices();
    }

    fn delete_char_before_cursor(&mut self) {
        if self.cursor_position > 0 {
            self.query.remove(self.cursor_position - 1);
            self.cursor_position -= 1;
            self.update_filtered_indices();
        }
    }

    fn delete_char_at_cursor(&mut self) {
        if self.cursor_position < self.query.len() {
            self.query.remove(self.cursor_position);
            self.update_filtered_indices();
        }
    }

    fn delete_to_line_start(&mut self) {
        self.query = self.query[self.cursor_position..].to_string();
        self.cursor_position = 0;
        self.update_filtered_indices();
    }

    fn delete_word_before_cursor(&mut self) {
        if self.cursor_position > 0 {
            let before_cursor = &self.query[..self.cursor_position];
            let word_start =
                before_cursor.trim_end().rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0);
            self.query =
                format!("{}{}", &self.query[..word_start], &self.query[self.cursor_position..]);
            self.cursor_position = word_start;
            self.update_filtered_indices();
        }
    }

    fn draw_preview<W: Write>(
        &self,
        w: &mut W,
        start_y: u16,
        width: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Get the selected entry
        let entry =
            self.filtered_indices.get(self.selected_index).and_then(|&idx| self.entries.get(idx));

        // Draw separator line
        queue!(w, MoveTo(0, start_y), Clear(ClearType::CurrentLine))?;
        queue!(w, SetForegroundColor(Color::DarkGrey))?;
        write!(w, "{}", "─".repeat(width as usize))?;
        queue!(w, ResetColor)?;

        // If no entry selected, clear the rest and return
        let Some(entry) = entry else {
            for row in 1..PREVIEW_HEIGHT {
                queue!(w, MoveTo(0, start_y + row as u16), Clear(ClearType::CurrentLine))?;
            }
            return Ok(());
        };

        // Line 1: Full command (can truncate)
        queue!(w, MoveTo(0, start_y + 1), Clear(ClearType::CurrentLine))?;
        let safe_cmd = sanitize_for_display(&entry.command);
        let cmd_display: String = if safe_cmd.chars().count() > width as usize - 2 {
            let truncated: String = safe_cmd.chars().take(width as usize - 5).collect();
            format!("{truncated}...")
        } else {
            safe_cmd
        };
        write!(w, "  {cmd_display}")?;

        // Line 2: Directory and timestamp
        queue!(w, MoveTo(0, start_y + 2), Clear(ClearType::CurrentLine))?;
        let mut info_parts: Vec<String> = Vec::new();

        if self.preview_config.show_directory
            && let Some(ref dir) = entry.working_directory
        {
            info_parts.push(format!("Dir: {}", String::from_utf8_lossy(dir)));
        }

        if self.preview_config.show_timestamp
            && let Some(ts) = entry.timestamp
        {
            let datetime = chrono::DateTime::from_timestamp(ts, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "?".to_string());
            info_parts.push(format!("Time: {datetime}"));
        }

        queue!(w, SetForegroundColor(Color::DarkGrey))?;
        write!(w, "  {}", info_parts.join("  "))?;
        queue!(w, ResetColor)?;

        // Line 3: Exit status, duration, hostname
        queue!(w, MoveTo(0, start_y + 3), Clear(ClearType::CurrentLine))?;
        let mut status_parts: Vec<String> = Vec::new();

        if self.preview_config.show_exit_status
            && let Some(status) = entry.exit_status
        {
            let status_str = if status == 0 {
                "Status: 0 (ok)".to_string()
            } else {
                format!("Status: {status} (error)")
            };
            status_parts.push(status_str);
        }

        if self.preview_config.show_duration
            && let Some(secs) = entry.duration_secs
        {
            let duration_str = if secs < 60 {
                format!("Duration: {secs}s")
            } else if secs < 3600 {
                format!("Duration: {}m {}s", secs / 60, secs % 60)
            } else {
                format!("Duration: {}h {}m", secs / 3600, (secs % 3600) / 60)
            };
            status_parts.push(duration_str);
        }

        if self.preview_config.show_hostname
            && let Some(ref host) = entry.hostname
        {
            status_parts.push(format!("Host: {}", String::from_utf8_lossy(host)));
        }

        queue!(w, SetForegroundColor(Color::DarkGrey))?;
        write!(w, "  {}", status_parts.join("  "))?;
        queue!(w, ResetColor)?;

        // Line 4: Bottom separator (blank or separator)
        queue!(w, MoveTo(0, start_y + 4), Clear(ClearType::CurrentLine))?;

        Ok(())
    }

    fn draw(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Get fresh terminal size each frame
        let (term_width, term_height) = terminal::size()?;
        self.term_width = term_width;
        self.term_height = term_height;

        let results_height = self.results_height();
        let preview_start_y = results_height as u16;
        let input_y = term_height.saturating_sub(2);
        let help_y = term_height.saturating_sub(1);

        // Use buffered writer to batch all terminal writes into a single syscall
        let mut w = BufWriter::new(&self.tty);

        // Disable line wrap during render to prevent visual glitches
        write!(w, "\x1b[?7l")?;

        // Draw each line, clearing as we go (avoids full-screen clear flicker)
        // Results area: rows 0 to results_height-1
        // Layout: oldest at top (row 0), newest at bottom (row results_height-1)
        // scroll_offset is the entry index shown at the bottom of the visible area
        for row in 0..results_height {
            queue!(w, MoveTo(0, row as u16), Clear(ClearType::CurrentLine))?;

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

            // Calculate quick-select number (1-9) relative to selection
            // Alt-1 = selected, Alt-2 = selected+1 (next older), etc.
            let quick_num =
                if entry_index >= self.selected_index && entry_index < self.selected_index + 9 {
                    Some(entry_index - self.selected_index + 1)
                } else {
                    None
                };

            if is_selected {
                queue!(w, SetBackgroundColor(Color::DarkGrey))?;
            }

            // Draw quick-select indicator or selection marker
            if let Some(n) = quick_num {
                queue!(w, SetForegroundColor(Color::Yellow))?;
                write!(w, "{n}")?;
                queue!(w, ResetColor)?;
                if is_selected {
                    queue!(w, SetBackgroundColor(Color::DarkGrey))?;
                    write!(w, ">")?;
                } else {
                    write!(w, " ")?;
                }
            } else if is_selected {
                write!(w, " >")?;
            } else {
                write!(w, "  ")?;
            }

            queue!(w, SetForegroundColor(Color::DarkGrey))?;
            write!(w, "{time_str}  ")?;
            queue!(w, ResetColor)?;

            if is_selected {
                queue!(w, SetBackgroundColor(Color::DarkGrey))?;
            }

            // Show host prefix for entries from other hosts (in AllHosts mode)
            let host_prefix = if self.host_filter == HostFilter::AllHosts {
                entry.hostname.as_ref().and_then(|h| {
                    let current = self.engine.current_hostname();
                    if h != current {
                        let short =
                            String::from_utf8_lossy(h).split('.').next().unwrap_or("?").to_string();
                        Some(format!("@{short}: "))
                    } else {
                        None
                    }
                })
            } else {
                None
            };

            // Draw host prefix if present
            let host_prefix_len = host_prefix.as_ref().map_or(0, |p| p.chars().count());
            if let Some(ref prefix) = host_prefix {
                queue!(w, SetForegroundColor(Color::Magenta))?;
                write!(w, "{prefix}")?;
                queue!(w, ResetColor)?;
                if is_selected {
                    queue!(w, SetBackgroundColor(Color::DarkGrey))?;
                }
            }

            // Sanitize and truncate command to fit (handle UTF-8 safely)
            let safe_cmd = sanitize_for_display(&entry.command);
            let prefix_len = 9 + host_prefix_len; // "n>" + " XXx  " + host prefix
            let max_cmd_len = term_width.saturating_sub(prefix_len as u16) as usize;
            let cmd: String = if safe_cmd.chars().count() > max_cmd_len {
                let truncated: String =
                    safe_cmd.chars().take(max_cmd_len.saturating_sub(3)).collect();
                format!("{truncated}...")
            } else {
                safe_cmd
            };
            write!(w, "{cmd}")?;

            queue!(w, ResetColor)?;
        }

        // Draw preview pane if enabled
        if self.show_preview {
            self.draw_preview(&mut w, preview_start_y, term_width)?;
        }

        // Draw input line
        queue!(w, MoveTo(0, input_y), Clear(ClearType::CurrentLine))?;
        write!(w, "> {}", self.query)?;

        // Draw mode indicators on same line (host filter + dir/global)
        let host_str = match self.host_filter {
            HostFilter::ThisHost => {
                let hostname = self.engine.current_hostname();
                let short_host =
                    String::from_utf8_lossy(hostname).split('.').next().unwrap_or("?").to_string();
                format!("[{short_host}]")
            }
            HostFilter::AllHosts => "[All Hosts]".to_string(),
        };
        let dir_str = match self.filter_mode {
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
        let mode_str = format!("{host_str} {dir_str}");
        let mode_x = term_width.saturating_sub(mode_str.len() as u16 + 1);
        queue!(w, MoveTo(mode_x, input_y), SetForegroundColor(Color::Cyan))?;
        write!(w, "{mode_str}")?;
        queue!(w, ResetColor)?;

        // Draw help line
        queue!(w, MoveTo(0, help_y), Clear(ClearType::CurrentLine))?;
        queue!(w, SetForegroundColor(Color::DarkGrey))?;
        write!(w, "↑↓/^R Nav  Enter Run  Tab Edit  ^H Host  Alt-1-9 Quick")?;
        queue!(w, ResetColor)?;

        // Position cursor at end of query in input line
        queue!(w, MoveTo(2 + self.cursor_position as u16, input_y))?;

        // Re-enable line wrap
        write!(w, "\x1b[?7h")?;

        // Single flush writes all buffered content
        w.flush()?;
        Ok(())
    }
}

enum KeyAction {
    Continue,
    Select,
    Edit,
    Cancel,
}

impl Drop for RecallTui {
    fn drop(&mut self) {
        #[cfg(not(target_os = "windows"))]
        if self.keyboard_enhanced {
            let _ = execute!(self.tty, PopKeyboardEnhancementFlags);
        }
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
