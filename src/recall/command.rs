use std::{env, path::PathBuf};

use clap::Parser;
use rusqlite::Connection;

use super::config::Config;
use super::engine::SearchEngine;
use super::tui::RecallTui;

#[derive(Parser, Debug)]
pub struct RecallCommand {
    #[clap(long, help = "Search only in current directory", conflicts_with = "global")]
    pub here: bool,
    #[clap(long, help = "Search across all directories (default)")]
    pub global: bool,
    #[clap(long, short = 'q', help = "Initial search query")]
    pub query: Option<String>,
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
        let config = Config::load();
        let initial_mode = if self.here { FilterMode::Directory } else { FilterMode::Global };

        let working_directory = env::var_os("PWD")
            .map(PathBuf::from)
            .or_else(|| env::current_dir().ok())
            .unwrap_or_default();

        let engine = SearchEngine::new(conn, working_directory, config.recall.result_limit);
        let mut tui = RecallTui::new(engine, initial_mode, self.query.clone(), &config.recall)?;

        match tui.run()? {
            Some(selection) => {
                // Output the selected command with mode prefix
                // The shell integration will parse and handle accordingly
                print!("{selection}");
                Ok(())
            }
            None => {
                // User cancelled - don't output anything
                Ok(())
            }
        }
    }
}
