use std::{env, path::PathBuf};

use clap::Parser;
use rusqlite::Connection;

use super::engine::SearchEngine;
use super::tui::RecallTui;

#[derive(Parser, Debug)]
pub struct RecallCommand {
    #[clap(long, help = "Search only in current directory", conflicts_with = "global")]
    pub here: bool,
    #[clap(long, help = "Search across all directories (default)")]
    pub global: bool,
}

/// Filter mode for recall search
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    /// Search only in current directory
    Directory,
    /// Search across all directories
    Global,
}

impl RecallCommand {
    pub fn go(&self, conn: Connection) -> Result<(), Box<dyn std::error::Error>> {
        let initial_mode = if self.here { FilterMode::Directory } else { FilterMode::Global };

        let working_directory = env::var_os("PWD")
            .map(PathBuf::from)
            .or_else(|| env::current_dir().ok())
            .unwrap_or_default();

        let engine = SearchEngine::new(conn, working_directory);
        let mut tui = RecallTui::new(engine, initial_mode)?;

        match tui.run()? {
            Some(selected_command) => {
                // Output the selected command to stdout
                // The shell integration will capture this and put it in the buffer
                print!("{selected_command}");
                Ok(())
            }
            None => {
                // User cancelled - don't output anything
                Ok(())
            }
        }
    }
}
