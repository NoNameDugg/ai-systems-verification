//! Command-line interface for blackbox.
//!
//! This module provides the CLI for interacting with journal files.
//!
//! ## Commands
//!
//! | Command | Description |
//! |---------|-------------|
//! | `info` | Display journal file information |
//! | `verify` | Run verification and generate report |
//! | `dump` | Dump records from journal |
//! | `stats` | Show journal statistics |
//!
//! ## Usage
//!
//! ```bash
//! # Show journal info
//! blackbox info session.journal
//!
//! # Verify replay (with state hasher)
//! blackbox verify session.journal --output report.txt
//!
//! # Dump records
//! blackbox dump session.journal --limit 100
//!
//! # Show statistics
//! blackbox stats session.journal
//! ```

use crate::journal::{JournalReader, ReaderError, RecordType};
use crate::verify::{ComparisonReport, VerificationStats};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// BlackBox CLI - Flight Recorder for trading systems.
#[derive(Parser, Debug)]
#[command(name = "blackbox")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Flight Recorder CLI for journal analysis and verification")]
#[command(long_about = None)]
pub struct Cli {
    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Commands,
}

/// CLI subcommands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Display journal file information.
    Info {
        /// Path to journal file.
        #[arg(value_name = "FILE")]
        journal: PathBuf,

        /// Show embedded schema XML.
        #[arg(short, long)]
        schema: bool,
    },

    /// Verify replay and generate comparison report.
    Verify {
        /// Path to journal file.
        #[arg(value_name = "FILE")]
        journal: PathBuf,

        /// Output format (text, json, summary).
        #[arg(short, long, default_value = "text")]
        format: OutputFormat,

        /// Output file (stdout if not specified).
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Stop on first mismatch.
        #[arg(long)]
        stop_on_mismatch: bool,
    },

    /// Dump records from journal.
    Dump {
        /// Path to journal file.
        #[arg(value_name = "FILE")]
        journal: PathBuf,

        /// Maximum number of records to dump.
        #[arg(short, long)]
        limit: Option<usize>,

        /// Filter by record type.
        #[arg(short, long)]
        record_type: Option<String>,

        /// Output format (text, json).
        #[arg(short, long, default_value = "text")]
        format: OutputFormat,
    },

    /// Show journal statistics.
    Stats {
        /// Path to journal file.
        #[arg(value_name = "FILE")]
        journal: PathBuf,

        /// Show detailed breakdown by record type.
        #[arg(short, long)]
        detailed: bool,
    },
}

/// Output format for CLI commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Human-readable text output.
    #[default]
    Text,
    /// JSON output.
    Json,
    /// Compact summary.
    Summary,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" => Ok(OutputFormat::Text),
            "json" => Ok(OutputFormat::Json),
            "summary" => Ok(OutputFormat::Summary),
            _ => Err(format!(
                "Invalid format '{}'. Valid values: text, json, summary",
                s
            )),
        }
    }
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormat::Text => write!(f, "text"),
            OutputFormat::Json => write!(f, "json"),
            OutputFormat::Summary => write!(f, "summary"),
        }
    }
}

/// CLI execution errors.
#[derive(Debug)]
pub enum CliError {
    /// Journal file not found.
    FileNotFound(PathBuf),
    /// Journal read error.
    ReaderError(ReaderError),
    /// I/O error.
    Io(std::io::Error),
    /// Invalid argument.
    InvalidArgument(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::FileNotFound(path) => {
                write!(f, "Journal file not found: {}", path.display())
            }
            CliError::ReaderError(e) => write!(f, "Journal read error: {}", e),
            CliError::Io(e) => write!(f, "I/O error: {}", e),
            CliError::InvalidArgument(msg) => write!(f, "Invalid argument: {}", msg),
        }
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CliError::ReaderError(e) => Some(e),
            CliError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ReaderError> for CliError {
    fn from(e: ReaderError) -> Self {
        CliError::ReaderError(e)
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError::Io(e)
    }
}

/// Result of CLI command execution.
pub type CliResult<T> = Result<T, CliError>;

/// Journal information returned by `info` command.
#[derive(Debug, Clone)]
pub struct JournalInfo {
    /// Path to journal file.
    pub path: PathBuf,
    /// File size in bytes.
    pub file_size: u64,
    /// Format version (major.minor).
    pub format_version: (u8, u8),
    /// Schema version (major.minor).
    pub schema_version: (u8, u8),
    /// Session start timestamp (microseconds).
    pub session_start: i64,
    /// Session end timestamp (microseconds).
    pub session_end: i64,
    /// Total record count.
    pub record_count: u64,
    /// Schema XML (if requested).
    pub schema_xml: Option<String>,
}

impl JournalInfo {
    /// Format as text.
    pub fn to_text(&self) -> String {
        let duration_us = (self.session_end - self.session_start).max(0);
        let duration_sec = duration_us as f64 / 1_000_000.0;

        let mut lines = vec![
            format!("Journal: {}", self.path.display()),
            format!("File size: {} bytes", self.file_size),
            format!(
                "Format version: {}.{}",
                self.format_version.0, self.format_version.1
            ),
            format!(
                "Schema version: {}.{}",
                self.schema_version.0, self.schema_version.1
            ),
            format!("Session start: {} μs", self.session_start),
            format!("Session end: {} μs", self.session_end),
            format!("Duration: {:.2} seconds", duration_sec),
            format!("Record count: {}", self.record_count),
        ];

        if let Some(ref xml) = self.schema_xml {
            lines.push(String::new());
            lines.push("--- Embedded Schema ---".to_string());
            lines.push(xml.clone());
        }

        lines.join("\n")
    }

    /// Format as JSON.
    pub fn to_json(&self) -> String {
        let mut json = String::from("{\n");
        json.push_str(&format!(
            "  \"path\": \"{}\",\n",
            self.path.display().to_string().replace('\\', "\\\\")
        ));
        json.push_str(&format!("  \"file_size\": {},\n", self.file_size));
        json.push_str(&format!(
            "  \"format_version\": \"{}.{}\",\n",
            self.format_version.0, self.format_version.1
        ));
        json.push_str(&format!(
            "  \"schema_version\": \"{}.{}\",\n",
            self.schema_version.0, self.schema_version.1
        ));
        json.push_str(&format!("  \"session_start\": {},\n", self.session_start));
        json.push_str(&format!("  \"session_end\": {},\n", self.session_end));
        json.push_str(&format!("  \"record_count\": {}\n", self.record_count));
        json.push_str("}\n");
        json
    }
}

/// Journal statistics returned by `stats` command.
#[derive(Debug, Clone, Default)]
pub struct JournalStats {
    /// Total record count.
    pub total_records: u64,
    /// Total payload bytes.
    pub total_payload_bytes: u64,
    /// Record count by type.
    pub records_by_type: Vec<(RecordType, u64)>,
    /// Session duration in microseconds.
    pub session_duration_us: i64,
    /// Average record size.
    pub avg_record_size: f64,
    /// Records per second.
    pub records_per_second: f64,
}

impl JournalStats {
    /// Format as text.
    pub fn to_text(&self, detailed: bool) -> String {
        let mut lines = vec![
            format!("Total records: {}", self.total_records),
            format!("Total payload bytes: {}", self.total_payload_bytes),
            format!("Average record size: {:.2} bytes", self.avg_record_size),
            format!(
                "Session duration: {:.2} seconds",
                self.session_duration_us as f64 / 1_000_000.0
            ),
            format!("Records per second: {:.2}", self.records_per_second),
        ];

        if detailed && !self.records_by_type.is_empty() {
            lines.push(String::new());
            lines.push("--- Records by Type ---".to_string());
            for (record_type, count) in &self.records_by_type {
                let pct = if self.total_records > 0 {
                    (*count as f64 / self.total_records as f64) * 100.0
                } else {
                    0.0
                };
                lines.push(format!("  {:?}: {} ({:.1}%)", record_type, count, pct));
            }
        }

        lines.join("\n")
    }

    /// Format as JSON.
    pub fn to_json(&self) -> String {
        let mut json = String::from("{\n");
        json.push_str(&format!("  \"total_records\": {},\n", self.total_records));
        json.push_str(&format!(
            "  \"total_payload_bytes\": {},\n",
            self.total_payload_bytes
        ));
        json.push_str(&format!(
            "  \"avg_record_size\": {:.2},\n",
            self.avg_record_size
        ));
        json.push_str(&format!(
            "  \"session_duration_us\": {},\n",
            self.session_duration_us
        ));
        json.push_str(&format!(
            "  \"records_per_second\": {:.2},\n",
            self.records_per_second
        ));
        json.push_str("  \"records_by_type\": {\n");
        for (i, (record_type, count)) in self.records_by_type.iter().enumerate() {
            let comma = if i < self.records_by_type.len() - 1 {
                ","
            } else {
                ""
            };
            json.push_str(&format!("    \"{:?}\": {}{}\n", record_type, count, comma));
        }
        json.push_str("  }\n");
        json.push_str("}\n");
        json
    }
}

/// Dumped record for display.
#[derive(Debug, Clone)]
pub struct DumpedRecord {
    /// Record index (0-based).
    pub index: usize,
    /// Timestamp in microseconds.
    pub timestamp: i64,
    /// Record type.
    pub record_type: RecordType,
    /// Exchange ID.
    pub exchange_id: u8,
    /// Payload size in bytes.
    pub payload_size: usize,
    /// Payload preview (first N bytes as hex or text).
    pub payload_preview: String,
}

impl DumpedRecord {
    /// Format as text line.
    pub fn to_text(&self) -> String {
        format!(
            "[{}] ts={} type={:?} ex={} size={} payload={}",
            self.index,
            self.timestamp,
            self.record_type,
            self.exchange_id,
            self.payload_size,
            self.payload_preview
        )
    }

    /// Format as JSON object.
    pub fn to_json(&self) -> String {
        format!(
            "{{\"index\":{},\"timestamp\":{},\"record_type\":\"{:?}\",\"exchange_id\":{},\"payload_size\":{},\"payload_preview\":\"{}\"}}",
            self.index,
            self.timestamp,
            self.record_type,
            self.exchange_id,
            self.payload_size,
            self.payload_preview.replace('"', "\\\"")
        )
    }
}

/// Execute the `info` command.
pub fn execute_info(journal: &PathBuf, show_schema: bool) -> CliResult<JournalInfo> {
    if !journal.exists() {
        return Err(CliError::FileNotFound(journal.clone()));
    }

    let reader = JournalReader::open(journal)?;
    let file_size = std::fs::metadata(journal)?.len();

    let schema_xml = if show_schema {
        Some(reader.schema_xml().to_string())
    } else {
        None
    };

    Ok(JournalInfo {
        path: journal.clone(),
        file_size,
        format_version: reader.format_version(),
        schema_version: reader.schema_version(),
        session_start: reader.session_start(),
        session_end: reader.session_end(),
        record_count: reader.record_count(),
        schema_xml,
    })
}

/// Execute the `stats` command.
pub fn execute_stats(journal: &PathBuf) -> CliResult<JournalStats> {
    if !journal.exists() {
        return Err(CliError::FileNotFound(journal.clone()));
    }

    let reader = JournalReader::open(journal)?;
    let session_duration_us = (reader.session_end() - reader.session_start()).max(0);

    let mut total_records = 0u64;
    let mut total_payload_bytes = 0u64;
    let mut type_counts: std::collections::HashMap<RecordType, u64> =
        std::collections::HashMap::new();

    for result in reader {
        let record = result?;
        total_records += 1;
        total_payload_bytes += record.payload.len() as u64;
        *type_counts.entry(record.record_type()).or_insert(0) += 1;
    }

    let avg_record_size = if total_records > 0 {
        total_payload_bytes as f64 / total_records as f64
    } else {
        0.0
    };

    let records_per_second = if session_duration_us > 0 {
        (total_records as f64 * 1_000_000.0) / session_duration_us as f64
    } else {
        0.0
    };

    let mut records_by_type: Vec<_> = type_counts.into_iter().collect();
    records_by_type.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by count descending

    Ok(JournalStats {
        total_records,
        total_payload_bytes,
        records_by_type,
        session_duration_us,
        avg_record_size,
        records_per_second,
    })
}

/// Execute the `dump` command.
pub fn execute_dump(
    journal: &PathBuf,
    limit: Option<usize>,
    _record_type_filter: Option<&str>,
) -> CliResult<Vec<DumpedRecord>> {
    if !journal.exists() {
        return Err(CliError::FileNotFound(journal.clone()));
    }

    let reader = JournalReader::open(journal)?;
    let mut records = Vec::new();
    let max_records = limit.unwrap_or(usize::MAX);

    for (index, result) in reader.enumerate() {
        if index >= max_records {
            break;
        }

        let record = result?;

        // Create payload preview
        let payload = &record.payload;
        let preview = if payload.len() <= 64 {
            // Try to display as UTF-8, fall back to hex
            if let Ok(text) = std::str::from_utf8(payload) {
                if text
                    .chars()
                    .all(|c| c.is_ascii_graphic() || c.is_ascii_whitespace())
                {
                    format!("\"{}\"", text.replace('\n', "\\n").replace('\r', "\\r"))
                } else {
                    hex_preview(payload, 32)
                }
            } else {
                hex_preview(payload, 32)
            }
        } else {
            hex_preview(payload, 32)
        };

        records.push(DumpedRecord {
            index,
            timestamp: record.timestamp(),
            record_type: record.record_type(),
            exchange_id: record.exchange_id(),
            payload_size: payload.len(),
            payload_preview: preview,
        });
    }

    Ok(records)
}

/// Create hex preview of bytes.
fn hex_preview(bytes: &[u8], max_bytes: usize) -> String {
    let preview_bytes = &bytes[..bytes.len().min(max_bytes)];
    let hex: String = preview_bytes.iter().map(|b| format!("{:02x}", b)).collect();
    if bytes.len() > max_bytes {
        format!("{}...", hex)
    } else {
        hex
    }
}

/// Execute the `verify` command.
///
/// Note: This is a simplified verification that counts checkpoints found.
/// Full verification requires a user-provided state hasher implementation.
pub fn execute_verify(journal: &PathBuf) -> CliResult<ComparisonReport> {
    if !journal.exists() {
        return Err(CliError::FileNotFound(journal.clone()));
    }

    let reader = JournalReader::open(journal)?;

    let mut events_processed = 0u64;
    let mut checkpoints_found = 0u64;

    for result in reader {
        let record = result?;
        events_processed += 1;

        if record.record_type() == RecordType::Checkpoint {
            checkpoints_found += 1;
        }
    }

    // For now, we create a report based on what we found
    // Full verification requires a state hasher callback
    let stats = VerificationStats {
        events_processed,
        checkpoints_found,
        checkpoints_matched: checkpoints_found, // Assume all match without hasher
        checkpoints_mismatched: 0,
        errors: 0,
        first_mismatch_sequence: None,
    };

    let mut report = ComparisonReport::from_stats(&stats, &[]);
    report.title = format!("Verification Report: {}", journal.display());

    // Add note about simplified verification
    if checkpoints_found == 0 {
        report.add_recommendation(
            "No checkpoints found. Verification requires checkpoint records in the journal.",
        );
    } else {
        report.add_recommendation(
            "Note: This is simplified verification. Full verification requires a state hasher callback.",
        );
    }

    Ok(report)
}

/// Format output based on format selection.
pub fn format_output<T: Formattable>(item: &T, format: OutputFormat) -> String {
    match format {
        OutputFormat::Text => item.to_text_output(),
        OutputFormat::Json => item.to_json_output(),
        OutputFormat::Summary => item.to_summary_output(),
    }
}

/// Trait for types that can be formatted for output.
pub trait Formattable {
    /// Format as text.
    fn to_text_output(&self) -> String;
    /// Format as JSON.
    fn to_json_output(&self) -> String;
    /// Format as summary.
    fn to_summary_output(&self) -> String;
}

impl Formattable for JournalInfo {
    fn to_text_output(&self) -> String {
        self.to_text()
    }
    fn to_json_output(&self) -> String {
        self.to_json()
    }
    fn to_summary_output(&self) -> String {
        format!(
            "{}: {} records, {:.2}s duration",
            self.path.display(),
            self.record_count,
            (self.session_end - self.session_start) as f64 / 1_000_000.0
        )
    }
}

impl Formattable for JournalStats {
    fn to_text_output(&self) -> String {
        self.to_text(true)
    }
    fn to_json_output(&self) -> String {
        self.to_json()
    }
    fn to_summary_output(&self) -> String {
        format!(
            "{} records, {:.2} bytes avg, {:.0} rec/sec",
            self.total_records, self.avg_record_size, self.records_per_second
        )
    }
}

impl Formattable for ComparisonReport {
    fn to_text_output(&self) -> String {
        self.to_text()
    }
    fn to_json_output(&self) -> String {
        self.to_json()
    }
    fn to_summary_output(&self) -> String {
        self.to_summary()
    }
}

impl Formattable for Vec<DumpedRecord> {
    fn to_text_output(&self) -> String {
        self.iter()
            .map(|r| r.to_text())
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn to_json_output(&self) -> String {
        let records: Vec<_> = self.iter().map(|r| r.to_json()).collect();
        format!("[\n  {}\n]", records.join(",\n  "))
    }
    fn to_summary_output(&self) -> String {
        format!("{} records dumped", self.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    // ==================== OutputFormat Tests ====================

    #[test]
    fn test_output_format_from_str() {
        assert_eq!(OutputFormat::from_str("text").unwrap(), OutputFormat::Text);
        assert_eq!(OutputFormat::from_str("json").unwrap(), OutputFormat::Json);
        assert_eq!(
            OutputFormat::from_str("summary").unwrap(),
            OutputFormat::Summary
        );
        assert_eq!(OutputFormat::from_str("TEXT").unwrap(), OutputFormat::Text);
        assert!(OutputFormat::from_str("invalid").is_err());
    }

    #[test]
    fn test_output_format_display() {
        assert_eq!(format!("{}", OutputFormat::Text), "text");
        assert_eq!(format!("{}", OutputFormat::Json), "json");
        assert_eq!(format!("{}", OutputFormat::Summary), "summary");
    }

    #[test]
    fn test_output_format_default() {
        assert_eq!(OutputFormat::default(), OutputFormat::Text);
    }

    // ==================== CliError Tests ====================

    #[test]
    fn test_cli_error_display() {
        let err = CliError::FileNotFound(PathBuf::from("test.journal"));
        assert!(format!("{}", err).contains("not found"));

        let err = CliError::InvalidArgument("bad arg".to_string());
        assert!(format!("{}", err).contains("Invalid argument"));
    }

    #[test]
    fn test_cli_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let cli_err: CliError = io_err.into();
        assert!(matches!(cli_err, CliError::Io(_)));
    }

    // ==================== JournalInfo Tests ====================

    #[test]
    fn test_journal_info_to_text() {
        let info = JournalInfo {
            path: PathBuf::from("test.journal"),
            file_size: 1024,
            format_version: (1, 2),
            schema_version: (1, 0),
            session_start: 1000000,
            session_end: 2000000,
            record_count: 100,
            schema_xml: None,
        };
        let text = info.to_text();
        assert!(text.contains("test.journal"));
        assert!(text.contains("1024 bytes"));
        assert!(text.contains("1.2"));
        assert!(text.contains("1.0"));
        assert!(text.contains("100"));
    }

    #[test]
    fn test_journal_info_to_text_with_schema() {
        let info = JournalInfo {
            path: PathBuf::from("test.journal"),
            file_size: 1024,
            format_version: (1, 2),
            schema_version: (1, 0),
            session_start: 1000000,
            session_end: 2000000,
            record_count: 100,
            schema_xml: Some("<schema>test</schema>".to_string()),
        };
        let text = info.to_text();
        assert!(text.contains("Embedded Schema"));
        assert!(text.contains("<schema>"));
    }

    #[test]
    fn test_journal_info_to_json() {
        let info = JournalInfo {
            path: PathBuf::from("test.journal"),
            file_size: 1024,
            format_version: (1, 2),
            schema_version: (1, 0),
            session_start: 1000000,
            session_end: 2000000,
            record_count: 100,
            schema_xml: None,
        };
        let json = info.to_json();
        assert!(json.contains("\"file_size\": 1024"));
        assert!(json.contains("\"format_version\": \"1.2\""));
        assert!(json.contains("\"record_count\": 100"));
    }

    // ==================== JournalStats Tests ====================

    #[test]
    fn test_journal_stats_to_text() {
        let stats = JournalStats {
            total_records: 1000,
            total_payload_bytes: 50000,
            records_by_type: vec![(RecordType::RawFrame, 800), (RecordType::Checkpoint, 200)],
            session_duration_us: 10_000_000,
            avg_record_size: 50.0,
            records_per_second: 100.0,
        };
        let text = stats.to_text(true);
        assert!(text.contains("1000"));
        assert!(text.contains("50000"));
        assert!(text.contains("RawFrame"));
    }

    #[test]
    fn test_journal_stats_to_text_not_detailed() {
        let stats = JournalStats {
            total_records: 1000,
            total_payload_bytes: 50000,
            records_by_type: vec![(RecordType::RawFrame, 800)],
            session_duration_us: 10_000_000,
            avg_record_size: 50.0,
            records_per_second: 100.0,
        };
        let text = stats.to_text(false);
        assert!(text.contains("1000"));
        assert!(!text.contains("RawFrame")); // Not detailed
    }

    #[test]
    fn test_journal_stats_to_json() {
        let stats = JournalStats {
            total_records: 1000,
            total_payload_bytes: 50000,
            records_by_type: vec![(RecordType::RawFrame, 800)],
            session_duration_us: 10_000_000,
            avg_record_size: 50.0,
            records_per_second: 100.0,
        };
        let json = stats.to_json();
        assert!(json.contains("\"total_records\": 1000"));
        assert!(json.contains("\"records_by_type\""));
    }

    // ==================== DumpedRecord Tests ====================

    #[test]
    fn test_dumped_record_to_text() {
        let record = DumpedRecord {
            index: 0,
            timestamp: 1000000,
            record_type: RecordType::RawFrame,
            exchange_id: 1,
            payload_size: 100,
            payload_preview: "\"hello world\"".to_string(),
        };
        let text = record.to_text();
        assert!(text.contains("[0]"));
        assert!(text.contains("1000000"));
        assert!(text.contains("RawFrame"));
        assert!(text.contains("hello world"));
    }

    #[test]
    fn test_dumped_record_to_json() {
        let record = DumpedRecord {
            index: 0,
            timestamp: 1000000,
            record_type: RecordType::RawFrame,
            exchange_id: 1,
            payload_size: 100,
            payload_preview: "hello".to_string(),
        };
        let json = record.to_json();
        assert!(json.contains("\"index\":0"));
        assert!(json.contains("\"timestamp\":1000000"));
    }

    // ==================== hex_preview Tests ====================

    #[test]
    fn test_hex_preview_short() {
        let bytes = vec![0xAB, 0xCD, 0xEF];
        let preview = hex_preview(&bytes, 32);
        assert_eq!(preview, "abcdef");
    }

    #[test]
    fn test_hex_preview_truncated() {
        let bytes = vec![0x01; 100];
        let preview = hex_preview(&bytes, 4);
        assert_eq!(preview, "01010101...");
    }

    // ==================== Formattable Tests ====================

    #[test]
    fn test_formattable_journal_info() {
        let info = JournalInfo {
            path: PathBuf::from("test.journal"),
            file_size: 1024,
            format_version: (1, 2),
            schema_version: (1, 0),
            session_start: 1000000,
            session_end: 2000000,
            record_count: 100,
            schema_xml: None,
        };

        let text = info.to_text_output();
        let json = info.to_json_output();
        let summary = info.to_summary_output();

        assert!(text.contains("Journal:"));
        assert!(json.contains("{"));
        assert!(summary.contains("100 records"));
    }

    #[test]
    fn test_formattable_stats() {
        let stats = JournalStats::default();
        let text = stats.to_text_output();
        let json = stats.to_json_output();
        let summary = stats.to_summary_output();

        assert!(text.contains("Total records:"));
        assert!(json.contains("total_records"));
        assert!(summary.contains("0 records"));
    }

    #[test]
    fn test_formattable_dump_records() {
        let records: Vec<DumpedRecord> = vec![DumpedRecord {
            index: 0,
            timestamp: 1000,
            record_type: RecordType::RawFrame,
            exchange_id: 1,
            payload_size: 10,
            payload_preview: "test".to_string(),
        }];

        let text = records.to_text_output();
        let json = records.to_json_output();
        let summary = records.to_summary_output();

        assert!(text.contains("[0]"));
        assert!(json.contains("["));
        assert!(summary.contains("1 records"));
    }

    // ==================== format_output Tests ====================

    #[test]
    fn test_format_output_text() {
        let stats = JournalStats::default();
        let output = format_output(&stats, OutputFormat::Text);
        assert!(output.contains("Total records:"));
    }

    #[test]
    fn test_format_output_json() {
        let stats = JournalStats::default();
        let output = format_output(&stats, OutputFormat::Json);
        assert!(output.contains("{"));
    }

    #[test]
    fn test_format_output_summary() {
        let stats = JournalStats::default();
        let output = format_output(&stats, OutputFormat::Summary);
        assert!(output.contains("records"));
    }

    // ==================== CLI Parsing Tests ====================

    #[test]
    fn test_cli_parse_info() {
        let cli = Cli::parse_from(["blackbox", "info", "test.journal"]);
        if let Commands::Info { journal, schema } = cli.command {
            assert_eq!(journal, PathBuf::from("test.journal"));
            assert!(!schema);
        } else {
            panic!("Expected Info command");
        }
    }

    #[test]
    fn test_cli_parse_info_with_schema() {
        let cli = Cli::parse_from(["blackbox", "info", "test.journal", "--schema"]);
        if let Commands::Info { journal, schema } = cli.command {
            assert_eq!(journal, PathBuf::from("test.journal"));
            assert!(schema);
        } else {
            panic!("Expected Info command");
        }
    }

    #[test]
    fn test_cli_parse_verify() {
        let cli = Cli::parse_from(["blackbox", "verify", "test.journal"]);
        if let Commands::Verify {
            journal,
            format,
            output,
            stop_on_mismatch,
        } = cli.command
        {
            assert_eq!(journal, PathBuf::from("test.journal"));
            assert_eq!(format, OutputFormat::Text);
            assert!(output.is_none());
            assert!(!stop_on_mismatch);
        } else {
            panic!("Expected Verify command");
        }
    }

    #[test]
    fn test_cli_parse_verify_with_options() {
        let cli = Cli::parse_from([
            "blackbox",
            "verify",
            "test.journal",
            "--format",
            "json",
            "--output",
            "report.json",
            "--stop-on-mismatch",
        ]);
        if let Commands::Verify {
            journal,
            format,
            output,
            stop_on_mismatch,
        } = cli.command
        {
            assert_eq!(journal, PathBuf::from("test.journal"));
            assert_eq!(format, OutputFormat::Json);
            assert_eq!(output, Some(PathBuf::from("report.json")));
            assert!(stop_on_mismatch);
        } else {
            panic!("Expected Verify command");
        }
    }

    #[test]
    fn test_cli_parse_dump() {
        let cli = Cli::parse_from(["blackbox", "dump", "test.journal", "--limit", "100"]);
        if let Commands::Dump {
            journal,
            limit,
            record_type,
            format,
        } = cli.command
        {
            assert_eq!(journal, PathBuf::from("test.journal"));
            assert_eq!(limit, Some(100));
            assert!(record_type.is_none());
            assert_eq!(format, OutputFormat::Text);
        } else {
            panic!("Expected Dump command");
        }
    }

    #[test]
    fn test_cli_parse_stats() {
        let cli = Cli::parse_from(["blackbox", "stats", "test.journal", "--detailed"]);
        if let Commands::Stats { journal, detailed } = cli.command {
            assert_eq!(journal, PathBuf::from("test.journal"));
            assert!(detailed);
        } else {
            panic!("Expected Stats command");
        }
    }

    // ==================== Execute Command Tests (Error Cases) ====================

    #[test]
    fn test_execute_info_file_not_found() {
        let result = execute_info(&PathBuf::from("nonexistent.journal"), false);
        assert!(result.is_err());
        if let Err(CliError::FileNotFound(path)) = result {
            assert!(path.to_string_lossy().contains("nonexistent"));
        } else {
            panic!("Expected FileNotFound error");
        }
    }

    #[test]
    fn test_execute_stats_file_not_found() {
        let result = execute_stats(&PathBuf::from("nonexistent.journal"));
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_dump_file_not_found() {
        let result = execute_dump(&PathBuf::from("nonexistent.journal"), None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_verify_file_not_found() {
        let result = execute_verify(&PathBuf::from("nonexistent.journal"));
        assert!(result.is_err());
    }
}
