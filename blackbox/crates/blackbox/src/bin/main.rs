//! BlackBox CLI - Flight Recorder for trading systems.
//!
//! This binary provides command-line access to journal files for
//! analysis, verification, and debugging.
//!
//! ## Commands
//!
//! - `info` - Display journal file information
//! - `verify` - Run verification and generate report
//! - `dump` - Dump records from journal
//! - `stats` - Show journal statistics
//!
//! ## Examples
//!
//! ```bash
//! # Show journal info
//! blackbox info session.journal
//!
//! # Verify replay
//! blackbox verify session.journal --format json
//!
//! # Dump first 100 records
//! blackbox dump session.journal --limit 100
//!
//! # Show detailed statistics
//! blackbox stats session.journal --detailed
//! ```

use blackbox::cli::{
    execute_dump, execute_info, execute_stats, execute_verify, format_output, Cli, CliError,
    Commands,
};
use clap::Parser;
use std::fs::File;
use std::io::Write;

fn main() {
    let cli = Cli::parse();

    let result = run_command(cli.command);

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run_command(command: Commands) -> Result<(), CliError> {
    match command {
        Commands::Info { journal, schema } => {
            let info = execute_info(&journal, schema)?;
            println!("{}", info.to_text());
        }

        Commands::Verify {
            journal,
            format,
            output,
            stop_on_mismatch,
        } => {
            let report = execute_verify(&journal, stop_on_mismatch)?;
            let output_str = format_output(&report, format);

            if let Some(output_path) = output {
                let mut file = File::create(&output_path)?;
                writeln!(file, "{}", output_str)?;
                println!("Report written to: {}", output_path.display());
            } else {
                println!("{}", output_str);
            }

            // Exit codes: 0 = Pass, 1 = Incomplete (structure sound, state hashes
            // not compared: this command cannot Pass without a hasher), 2 = Fail.
            match report.status {
                blackbox::verify::ReportStatus::Pass => {}
                blackbox::verify::ReportStatus::Incomplete => std::process::exit(1),
                blackbox::verify::ReportStatus::Fail => std::process::exit(2),
            }
        }

        Commands::Dump {
            journal,
            limit,
            record_type,
            format,
        } => {
            let records = execute_dump(&journal, limit, record_type.as_deref())?;
            let output_str = format_output(&records, format);
            println!("{}", output_str);
        }

        Commands::Stats { journal, detailed } => {
            let stats = execute_stats(&journal)?;
            let output_str = if detailed {
                stats.to_text(true)
            } else {
                stats.to_text(false)
            };
            println!("{}", output_str);
        }
    }

    Ok(())
}
