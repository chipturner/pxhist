use std::io::{self, Write};
use std::time::Instant;
use std::{env, path::PathBuf};

use clap::Parser;
use rusqlite::Connection;

use super::config::Config;
use super::engine::{SearchEngine, format_relative_time};
use super::tui::RecallTui;
use crate::get_hostname;

#[derive(Parser, Debug)]
pub struct RecallCommand {
    #[clap(long, help = "Search only in current directory", conflicts_with = "global")]
    pub here: bool,
    #[clap(long, help = "Search across all directories (default)")]
    pub global: bool,
    #[clap(long, short = 'q', help = "Initial search query")]
    pub query: Option<String>,
    #[clap(long, short = 'p', help = "Print results instead of showing TUI")]
    pub print: bool,
    #[clap(long, short = 'l', help = "Limit results when printing", default_value = "20")]
    pub limit: usize,
    #[clap(long, help = "Show timing information")]
    pub timing: bool,
    #[clap(long, help = "Paint TUI once then exit (for profiling)")]
    pub paint_then_exit: bool,
    #[clap(long, help = "Shell integration mode (outputs command for shell to execute)")]
    pub shell_mode: bool,
}

/// Filter mode for recall search
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    /// Search only in current directory
    Directory,
    /// Search across all directories
    Global,
}

/// Host filter mode for recall search
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HostFilter {
    /// Search only on current host (default)
    #[default]
    ThisHost,
    /// Search across all hosts
    AllHosts,
}

impl RecallCommand {
    pub fn go(&self, conn: Connection) -> Result<(), Box<dyn std::error::Error>> {
        let start = Instant::now();

        let config_start = Instant::now();
        let config = Config::load();
        let config_time = config_start.elapsed();

        let initial_mode = if self.here { FilterMode::Directory } else { FilterMode::Global };

        let working_directory = env::var_os("PWD")
            .map(PathBuf::from)
            .or_else(|| env::current_dir().ok())
            .unwrap_or_default();

        let result_limit = if self.print { self.limit } else { config.recall.result_limit };
        let current_hostname = get_hostname();
        let engine = SearchEngine::new(conn, working_directory, current_hostname, result_limit);

        // Print mode: just query and print results, no TUI
        if self.print {
            let query_start = Instant::now();
            let query = self.query.as_deref();
            let entries = engine.load_entries(initial_mode, HostFilter::default(), query)?;
            let query_time = query_start.elapsed();

            let mut stdout = io::stdout().lock();
            for entry in &entries {
                let time_str = format_relative_time(entry.timestamp);
                writeln!(stdout, " {time_str}  {}", entry.command)?;
            }

            if self.timing {
                let total = start.elapsed();
                eprintln!("\nTiming:");
                eprintln!("  Config load:  {:?}", config_time);
                eprintln!("  DB query:     {:?}", query_time);
                eprintln!("  Total:        {:?}", total);
                eprintln!("  Entries:      {}", entries.len());
            }

            return Ok(());
        }

        // TUI mode
        let tui_start = Instant::now();
        let mut tui = RecallTui::new(
            engine,
            initial_mode,
            self.query.clone(),
            &config.recall,
            self.shell_mode,
        )?;
        let tui_init_time = tui_start.elapsed();

        if self.paint_then_exit {
            let draw_start = Instant::now();
            tui.draw_once()?;
            let draw_time = draw_start.elapsed();

            if self.timing {
                eprintln!("Timing:");
                eprintln!("  Config load:  {:?}", config_time);
                eprintln!("  TUI init:     {:?}", tui_init_time);
                eprintln!("  Draw:         {:?}", draw_time);
                eprintln!("  Total:        {:?}", start.elapsed());
            }
            return Ok(());
        }

        if self.timing {
            eprintln!("Timing (before run):");
            eprintln!("  Config load:  {:?}", config_time);
            eprintln!("  TUI init:     {:?}", tui_init_time);
            eprintln!("  Total:        {:?}", start.elapsed());
        }

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
