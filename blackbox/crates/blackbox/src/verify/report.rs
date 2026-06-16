//! Comparison report generator for replay verification (T4.4).
//!
//! This module provides `ComparisonReport`, which generates detailed reports
//! from verification results. Reports can be output in multiple formats
//! (text, JSON) for analysis and debugging.
//!
//! ## Report Contents
//!
//! | Section | Description |
//! |---------|-------------|
//! | Summary | Overall pass/fail, match rate |
//! | Statistics | Events, checkpoints, timings |
//! | Mismatches | Detailed list of divergences |
//! | Recommendations | Suggested actions |
//!
//! ## Usage
//!
//! ```rust,ignore
//! use blackbox::verify::{ComparisonReport, VerificationStats};
//!
//! let stats = engine.run_with_verification();
//! let report = ComparisonReport::from_stats(&stats, engine.results());
//!
//! // Output as text
//! println!("{}", report.to_text());
//!
//! // Output as JSON
//! println!("{}", report.to_json());
//! ```

use super::{StateHash, VerificationResult, VerificationStats};

/// Severity level for report entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Informational message.
    Info,
    /// Warning - non-critical issue.
    Warning,
    /// Error - verification failed.
    Error,
    /// Critical - major divergence.
    Critical,
}

impl Severity {
    /// Get display string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Info => "INFO",
            Severity::Warning => "WARN",
            Severity::Error => "ERROR",
            Severity::Critical => "CRITICAL",
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A single entry in the report.
#[derive(Debug, Clone)]
pub struct ReportEntry {
    /// Entry severity.
    pub severity: Severity,
    /// Sequence number (if applicable).
    pub sequence: Option<u64>,
    /// Timestamp (if applicable).
    pub timestamp: Option<i64>,
    /// Entry message.
    pub message: String,
    /// Additional details.
    pub details: Option<String>,
}

impl ReportEntry {
    /// Create an info entry.
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Info,
            sequence: None,
            timestamp: None,
            message: message.into(),
            details: None,
        }
    }

    /// Create a warning entry.
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            sequence: None,
            timestamp: None,
            message: message.into(),
            details: None,
        }
    }

    /// Create an error entry.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            sequence: None,
            timestamp: None,
            message: message.into(),
            details: None,
        }
    }

    /// Create a critical entry.
    pub fn critical(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Critical,
            sequence: None,
            timestamp: None,
            message: message.into(),
            details: None,
        }
    }

    /// Add sequence number.
    pub fn with_sequence(mut self, seq: u64) -> Self {
        self.sequence = Some(seq);
        self
    }

    /// Add timestamp.
    pub fn with_timestamp(mut self, ts: i64) -> Self {
        self.timestamp = Some(ts);
        self
    }

    /// Add details.
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    /// Format as text line.
    pub fn to_text(&self) -> String {
        let mut parts = vec![format!("[{}]", self.severity)];

        if let Some(seq) = self.sequence {
            parts.push(format!("seq={}", seq));
        }

        if let Some(ts) = self.timestamp {
            parts.push(format!("ts={}", ts));
        }

        parts.push(self.message.clone());

        let line = parts.join(" ");

        if let Some(ref details) = self.details {
            format!("{}\n  {}", line, details)
        } else {
            line
        }
    }
}

/// Report status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportStatus {
    /// All verifications passed.
    Pass,
    /// Some verifications failed.
    Fail,
    /// Verification could not complete.
    Incomplete,
}

impl ReportStatus {
    /// Get display string.
    pub fn as_str(&self) -> &'static str {
        match self {
            ReportStatus::Pass => "PASS",
            ReportStatus::Fail => "FAIL",
            ReportStatus::Incomplete => "INCOMPLETE",
        }
    }
}

impl std::fmt::Display for ReportStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Mismatch detail in the report.
#[derive(Debug, Clone)]
pub struct MismatchDetail {
    /// Checkpoint sequence.
    pub sequence: u64,
    /// Checkpoint timestamp.
    pub timestamp: i64,
    /// Expected hash (hex).
    pub expected_hash: String,
    /// Actual hash (hex).
    pub actual_hash: String,
}

impl MismatchDetail {
    /// Create from verification result.
    pub fn from_result(result: &VerificationResult) -> Option<Self> {
        if let VerificationResult::Mismatch {
            sequence,
            timestamp,
            expected,
            actual,
        } = result
        {
            Some(Self {
                sequence: *sequence,
                timestamp: *timestamp,
                expected_hash: StateHash::to_hex(expected),
                actual_hash: StateHash::to_hex(actual),
            })
        } else {
            None
        }
    }

    /// Format as text.
    pub fn to_text(&self) -> String {
        format!(
            "Checkpoint {} (ts={})\n  Expected: {}\n  Actual:   {}",
            self.sequence, self.timestamp, self.expected_hash, self.actual_hash
        )
    }
}

/// Comprehensive comparison report.
#[derive(Debug, Clone)]
pub struct ComparisonReport {
    /// Report title.
    pub title: String,
    /// Report status.
    pub status: ReportStatus,
    /// Statistics from verification.
    pub stats: VerificationStats,
    /// Report entries.
    pub entries: Vec<ReportEntry>,
    /// Mismatch details.
    pub mismatches: Vec<MismatchDetail>,
    /// Recommendations.
    pub recommendations: Vec<String>,
}

impl ComparisonReport {
    /// Create a new empty report.
    pub fn new() -> Self {
        Self {
            title: "Replay Verification Report".to_string(),
            status: ReportStatus::Incomplete,
            stats: VerificationStats::new(),
            entries: Vec::new(),
            mismatches: Vec::new(),
            recommendations: Vec::new(),
        }
    }

    /// Create from verification stats and results.
    pub fn from_stats(stats: &VerificationStats, results: &[VerificationResult]) -> Self {
        let mut report = Self::new();
        report.stats = stats.clone();

        // Determine status
        report.status = if stats.is_successful() {
            ReportStatus::Pass
        } else {
            ReportStatus::Fail
        };

        // Add summary entry
        report.entries.push(ReportEntry::info(format!(
            "Processed {} events with {} checkpoints",
            stats.events_processed, stats.checkpoints_found
        )));

        // Add match rate entry
        let rate_severity = if stats.match_rate() >= 100.0 {
            Severity::Info
        } else if stats.match_rate() >= 95.0 {
            Severity::Warning
        } else {
            Severity::Error
        };
        report.entries.push(ReportEntry {
            severity: rate_severity,
            sequence: None,
            timestamp: None,
            message: format!("Match rate: {:.2}%", stats.match_rate()),
            details: None,
        });

        // Extract mismatches
        for result in results {
            if let Some(detail) = MismatchDetail::from_result(result) {
                report.mismatches.push(detail);
            }
        }

        // Add mismatch entries
        for mismatch in &report.mismatches {
            report.entries.push(
                ReportEntry::error(format!("Mismatch at checkpoint {}", mismatch.sequence))
                    .with_sequence(mismatch.sequence)
                    .with_timestamp(mismatch.timestamp)
                    .with_details(format!(
                        "Expected: {}, Actual: {}",
                        &mismatch.expected_hash[..16],
                        &mismatch.actual_hash[..16]
                    )),
            );
        }

        // Add error entries
        for result in results {
            if let VerificationResult::Error { message } = result {
                report
                    .entries
                    .push(ReportEntry::critical(format!("Error: {}", message)));
            }
        }

        // Generate recommendations
        report.generate_recommendations();

        report
    }

    /// Create a passing report.
    pub fn pass(events: u64, checkpoints: u64) -> Self {
        Self::from_stats(
            &VerificationStats {
                events_processed: events,
                checkpoints_found: checkpoints,
                checkpoints_matched: checkpoints,
                ..Default::default()
            },
            &[],
        )
    }

    /// Set custom title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Add an entry.
    pub fn add_entry(&mut self, entry: ReportEntry) {
        self.entries.push(entry);
    }

    /// Add a recommendation.
    pub fn add_recommendation(&mut self, rec: impl Into<String>) {
        self.recommendations.push(rec.into());
    }

    /// Generate recommendations based on results.
    fn generate_recommendations(&mut self) {
        if !self.stats.is_successful() {
            if self.stats.checkpoints_mismatched > 0 {
                self.recommendations.push(
                    "Review state computation at the first mismatched checkpoint".to_string(),
                );

                if let Some(seq) = self.stats.first_mismatch_sequence {
                    self.recommendations
                        .push(format!("Debug starting from checkpoint sequence {}", seq));
                }

                if self.stats.checkpoints_mismatched > 5 {
                    self.recommendations.push(
                        "Multiple mismatches suggest a systematic issue in state computation"
                            .to_string(),
                    );
                }
            }

            if self.stats.errors > 0 {
                self.recommendations
                    .push("Check journal file integrity".to_string());
                self.recommendations
                    .push("Verify checkpoint record format".to_string());
            }
        } else if self.stats.checkpoints_found == 0 {
            self.recommendations
                .push("No checkpoints found - consider adding checkpoint recording".to_string());
        }
    }

    /// Check if verification passed.
    pub fn is_pass(&self) -> bool {
        self.status == ReportStatus::Pass
    }

    /// Get total mismatch count.
    pub fn mismatch_count(&self) -> usize {
        self.mismatches.len()
    }

    /// Format as text report.
    pub fn to_text(&self) -> String {
        let mut lines = Vec::new();

        // Header
        lines.push("=".repeat(60));
        lines.push(self.title.clone());
        lines.push("=".repeat(60));
        lines.push(String::new());

        // Status
        lines.push(format!("Status: {}", self.status));
        lines.push(String::new());

        // Statistics
        lines.push("--- Statistics ---".to_string());
        lines.push(format!(
            "Events processed:      {}",
            self.stats.events_processed
        ));
        lines.push(format!(
            "Checkpoints found:     {}",
            self.stats.checkpoints_found
        ));
        lines.push(format!(
            "Checkpoints matched:   {}",
            self.stats.checkpoints_matched
        ));
        lines.push(format!(
            "Checkpoints mismatched: {}",
            self.stats.checkpoints_mismatched
        ));
        lines.push(format!("Errors:                {}", self.stats.errors));
        lines.push(format!(
            "Match rate:            {:.2}%",
            self.stats.match_rate()
        ));
        lines.push(String::new());

        // Entries
        if !self.entries.is_empty() {
            lines.push("--- Log ---".to_string());
            for entry in &self.entries {
                lines.push(entry.to_text());
            }
            lines.push(String::new());
        }

        // Mismatches
        if !self.mismatches.is_empty() {
            lines.push("--- Mismatch Details ---".to_string());
            for (i, mismatch) in self.mismatches.iter().enumerate() {
                lines.push(format!("{}. {}", i + 1, mismatch.to_text()));
                lines.push(String::new());
            }
        }

        // Recommendations
        if !self.recommendations.is_empty() {
            lines.push("--- Recommendations ---".to_string());
            for (i, rec) in self.recommendations.iter().enumerate() {
                lines.push(format!("{}. {}", i + 1, rec));
            }
            lines.push(String::new());
        }

        // Footer
        lines.push("=".repeat(60));

        lines.join("\n")
    }

    /// Format as JSON report.
    pub fn to_json(&self) -> String {
        let mut json = String::from("{\n");

        // Title and status
        json.push_str(&format!("  \"title\": \"{}\",\n", self.title));
        json.push_str(&format!("  \"status\": \"{}\",\n", self.status));

        // Stats
        json.push_str("  \"stats\": {\n");
        json.push_str(&format!(
            "    \"events_processed\": {},\n",
            self.stats.events_processed
        ));
        json.push_str(&format!(
            "    \"checkpoints_found\": {},\n",
            self.stats.checkpoints_found
        ));
        json.push_str(&format!(
            "    \"checkpoints_matched\": {},\n",
            self.stats.checkpoints_matched
        ));
        json.push_str(&format!(
            "    \"checkpoints_mismatched\": {},\n",
            self.stats.checkpoints_mismatched
        ));
        json.push_str(&format!("    \"errors\": {},\n", self.stats.errors));
        json.push_str(&format!(
            "    \"match_rate\": {:.2}\n",
            self.stats.match_rate()
        ));
        json.push_str("  },\n");

        // Mismatches
        json.push_str("  \"mismatches\": [\n");
        for (i, m) in self.mismatches.iter().enumerate() {
            json.push_str("    {\n");
            json.push_str(&format!("      \"sequence\": {},\n", m.sequence));
            json.push_str(&format!("      \"timestamp\": {},\n", m.timestamp));
            json.push_str(&format!(
                "      \"expected_hash\": \"{}\",\n",
                m.expected_hash
            ));
            json.push_str(&format!("      \"actual_hash\": \"{}\"\n", m.actual_hash));
            if i < self.mismatches.len() - 1 {
                json.push_str("    },\n");
            } else {
                json.push_str("    }\n");
            }
        }
        json.push_str("  ],\n");

        // Recommendations
        json.push_str("  \"recommendations\": [\n");
        for (i, r) in self.recommendations.iter().enumerate() {
            if i < self.recommendations.len() - 1 {
                json.push_str(&format!("    \"{}\",\n", r.replace('"', "\\\"")));
            } else {
                json.push_str(&format!("    \"{}\"\n", r.replace('"', "\\\"")));
            }
        }
        json.push_str("  ]\n");

        json.push_str("}\n");
        json
    }

    /// Format as compact summary.
    pub fn to_summary(&self) -> String {
        format!(
            "{}: {} events, {}/{} checkpoints matched ({:.1}%)",
            self.status,
            self.stats.events_processed,
            self.stats.checkpoints_matched,
            self.stats.checkpoints_found,
            self.stats.match_rate()
        )
    }
}

impl Default for ComparisonReport {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ComparisonReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::VerificationResult;

    // ==================== Severity Tests ====================

    #[test]
    fn test_severity_as_str() {
        assert_eq!(Severity::Info.as_str(), "INFO");
        assert_eq!(Severity::Warning.as_str(), "WARN");
        assert_eq!(Severity::Error.as_str(), "ERROR");
        assert_eq!(Severity::Critical.as_str(), "CRITICAL");
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(format!("{}", Severity::Info), "INFO");
        assert_eq!(format!("{}", Severity::Error), "ERROR");
    }

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
        assert!(Severity::Error < Severity::Critical);
    }

    // ==================== ReportEntry Tests ====================

    #[test]
    fn test_report_entry_info() {
        let entry = ReportEntry::info("Test message");
        assert_eq!(entry.severity, Severity::Info);
        assert_eq!(entry.message, "Test message");
    }

    #[test]
    fn test_report_entry_warning() {
        let entry = ReportEntry::warning("Warning message");
        assert_eq!(entry.severity, Severity::Warning);
    }

    #[test]
    fn test_report_entry_error() {
        let entry = ReportEntry::error("Error message");
        assert_eq!(entry.severity, Severity::Error);
    }

    #[test]
    fn test_report_entry_critical() {
        let entry = ReportEntry::critical("Critical message");
        assert_eq!(entry.severity, Severity::Critical);
    }

    #[test]
    fn test_report_entry_with_sequence() {
        let entry = ReportEntry::info("Test").with_sequence(42);
        assert_eq!(entry.sequence, Some(42));
    }

    #[test]
    fn test_report_entry_with_timestamp() {
        let entry = ReportEntry::info("Test").with_timestamp(1000);
        assert_eq!(entry.timestamp, Some(1000));
    }

    #[test]
    fn test_report_entry_with_details() {
        let entry = ReportEntry::info("Test").with_details("More info");
        assert_eq!(entry.details, Some("More info".to_string()));
    }

    #[test]
    fn test_report_entry_to_text_simple() {
        let entry = ReportEntry::info("Test message");
        let text = entry.to_text();
        assert!(text.contains("[INFO]"));
        assert!(text.contains("Test message"));
    }

    #[test]
    fn test_report_entry_to_text_with_sequence() {
        let entry = ReportEntry::error("Test").with_sequence(5);
        let text = entry.to_text();
        assert!(text.contains("seq=5"));
    }

    #[test]
    fn test_report_entry_to_text_with_details() {
        let entry = ReportEntry::warning("Main").with_details("Extra details");
        let text = entry.to_text();
        assert!(text.contains("Extra details"));
    }

    // ==================== ReportStatus Tests ====================

    #[test]
    fn test_report_status_as_str() {
        assert_eq!(ReportStatus::Pass.as_str(), "PASS");
        assert_eq!(ReportStatus::Fail.as_str(), "FAIL");
        assert_eq!(ReportStatus::Incomplete.as_str(), "INCOMPLETE");
    }

    #[test]
    fn test_report_status_display() {
        assert_eq!(format!("{}", ReportStatus::Pass), "PASS");
    }

    // ==================== MismatchDetail Tests ====================

    #[test]
    fn test_mismatch_detail_from_result_mismatch() {
        let result = VerificationResult::Mismatch {
            sequence: 5,
            timestamp: 1000,
            expected: [0xAA; 32],
            actual: [0xBB; 32],
        };
        let detail = MismatchDetail::from_result(&result).unwrap();
        assert_eq!(detail.sequence, 5);
        assert_eq!(detail.timestamp, 1000);
    }

    #[test]
    fn test_mismatch_detail_from_result_match() {
        let result = VerificationResult::Match {
            sequence: 5,
            timestamp: 1000,
        };
        let detail = MismatchDetail::from_result(&result);
        assert!(detail.is_none());
    }

    #[test]
    fn test_mismatch_detail_from_result_error() {
        let result = VerificationResult::Error {
            message: "Test".to_string(),
        };
        let detail = MismatchDetail::from_result(&result);
        assert!(detail.is_none());
    }

    #[test]
    fn test_mismatch_detail_to_text() {
        let detail = MismatchDetail {
            sequence: 1,
            timestamp: 1000,
            expected_hash: "aa".repeat(32),
            actual_hash: "bb".repeat(32),
        };
        let text = detail.to_text();
        assert!(text.contains("Checkpoint 1"));
        assert!(text.contains("Expected:"));
        assert!(text.contains("Actual:"));
    }

    // ==================== ComparisonReport Basic Tests ====================

    #[test]
    fn test_comparison_report_new() {
        let report = ComparisonReport::new();
        assert_eq!(report.status, ReportStatus::Incomplete);
        assert!(report.entries.is_empty());
        assert!(report.mismatches.is_empty());
    }

    #[test]
    fn test_comparison_report_default() {
        let report = ComparisonReport::default();
        assert_eq!(report.status, ReportStatus::Incomplete);
    }

    #[test]
    fn test_comparison_report_pass() {
        let report = ComparisonReport::pass(100, 5);
        assert_eq!(report.status, ReportStatus::Pass);
        assert!(report.is_pass());
        assert_eq!(report.stats.events_processed, 100);
    }

    #[test]
    fn test_comparison_report_with_title() {
        let report = ComparisonReport::new().with_title("Custom Title");
        assert_eq!(report.title, "Custom Title");
    }

    #[test]
    fn test_comparison_report_add_entry() {
        let mut report = ComparisonReport::new();
        report.add_entry(ReportEntry::info("Test"));
        assert_eq!(report.entries.len(), 1);
    }

    #[test]
    fn test_comparison_report_add_recommendation() {
        let mut report = ComparisonReport::new();
        report.add_recommendation("Do this");
        assert_eq!(report.recommendations.len(), 1);
        assert_eq!(report.recommendations[0], "Do this");
    }

    // ==================== from_stats Tests ====================

    #[test]
    fn test_from_stats_pass() {
        let stats = VerificationStats {
            events_processed: 100,
            checkpoints_found: 5,
            checkpoints_matched: 5,
            ..Default::default()
        };
        let report = ComparisonReport::from_stats(&stats, &[]);

        assert_eq!(report.status, ReportStatus::Pass);
        assert!(report.is_pass());
        assert_eq!(report.mismatch_count(), 0);
    }

    #[test]
    fn test_from_stats_fail() {
        let stats = VerificationStats {
            events_processed: 100,
            checkpoints_found: 5,
            checkpoints_matched: 3,
            checkpoints_mismatched: 2,
            first_mismatch_sequence: Some(2),
            ..Default::default()
        };
        let results = vec![
            VerificationResult::Match {
                sequence: 1,
                timestamp: 1000,
            },
            VerificationResult::Mismatch {
                sequence: 2,
                timestamp: 2000,
                expected: [0xAA; 32],
                actual: [0xBB; 32],
            },
        ];
        let report = ComparisonReport::from_stats(&stats, &results);

        assert_eq!(report.status, ReportStatus::Fail);
        assert!(!report.is_pass());
        assert_eq!(report.mismatch_count(), 1);
    }

    #[test]
    fn test_from_stats_with_errors() {
        let stats = VerificationStats {
            errors: 1,
            ..Default::default()
        };
        let results = vec![VerificationResult::Error {
            message: "Parse error".to_string(),
        }];
        let report = ComparisonReport::from_stats(&stats, &results);

        assert_eq!(report.status, ReportStatus::Fail);
    }

    #[test]
    fn test_from_stats_generates_recommendations() {
        let stats = VerificationStats {
            checkpoints_found: 5,
            checkpoints_matched: 3,
            checkpoints_mismatched: 2,
            first_mismatch_sequence: Some(2),
            ..Default::default()
        };
        let report = ComparisonReport::from_stats(&stats, &[]);

        assert!(!report.recommendations.is_empty());
        // Should recommend debugging from first mismatch
        let has_debug_rec = report
            .recommendations
            .iter()
            .any(|r| r.contains("checkpoint sequence 2"));
        assert!(has_debug_rec);
    }

    #[test]
    fn test_from_stats_no_checkpoints_recommendation() {
        let stats = VerificationStats {
            events_processed: 100,
            checkpoints_found: 0,
            ..Default::default()
        };
        let report = ComparisonReport::from_stats(&stats, &[]);

        let has_rec = report
            .recommendations
            .iter()
            .any(|r| r.contains("No checkpoints"));
        assert!(has_rec);
    }

    // ==================== Output Format Tests ====================

    #[test]
    fn test_to_text() {
        let report = ComparisonReport::pass(100, 5);
        let text = report.to_text();

        assert!(text.contains("Replay Verification Report"));
        assert!(text.contains("Status: PASS"));
        assert!(text.contains("Events processed:"));
        assert!(text.contains("100"));
    }

    #[test]
    fn test_to_text_with_mismatches() {
        let stats = VerificationStats {
            checkpoints_found: 2,
            checkpoints_mismatched: 1,
            ..Default::default()
        };
        let results = vec![VerificationResult::Mismatch {
            sequence: 1,
            timestamp: 1000,
            expected: [0xAA; 32],
            actual: [0xBB; 32],
        }];
        let report = ComparisonReport::from_stats(&stats, &results);
        let text = report.to_text();

        assert!(text.contains("Mismatch Details"));
        assert!(text.contains("Checkpoint 1"));
    }

    #[test]
    fn test_to_json() {
        let report = ComparisonReport::pass(100, 5);
        let json = report.to_json();

        assert!(json.contains("\"title\":"));
        assert!(json.contains("\"status\": \"PASS\""));
        assert!(json.contains("\"events_processed\": 100"));
    }

    #[test]
    fn test_to_json_with_mismatches() {
        let stats = VerificationStats {
            checkpoints_found: 1,
            checkpoints_mismatched: 1,
            ..Default::default()
        };
        let results = vec![VerificationResult::Mismatch {
            sequence: 1,
            timestamp: 1000,
            expected: [0xAA; 32],
            actual: [0xBB; 32],
        }];
        let report = ComparisonReport::from_stats(&stats, &results);
        let json = report.to_json();

        assert!(json.contains("\"mismatches\": ["));
        assert!(json.contains("\"sequence\": 1"));
    }

    #[test]
    fn test_to_summary() {
        let report = ComparisonReport::pass(100, 5);
        let summary = report.to_summary();

        assert!(summary.contains("PASS"));
        assert!(summary.contains("100 events"));
        assert!(summary.contains("5/5"));
    }

    #[test]
    fn test_display_impl() {
        let report = ComparisonReport::pass(10, 1);
        let display = format!("{}", report);
        assert!(display.contains("Replay Verification Report"));
    }

    // ==================== Edge Cases ====================

    #[test]
    fn test_empty_report_to_text() {
        let report = ComparisonReport::new();
        let text = report.to_text();
        // Should not panic and should produce valid output
        assert!(!text.is_empty());
    }

    #[test]
    fn test_empty_report_to_json() {
        let report = ComparisonReport::new();
        let json = report.to_json();
        // Should produce valid JSON structure
        assert!(json.starts_with('{'));
        assert!(json.ends_with("}\n"));
    }

    #[test]
    fn test_multiple_mismatches() {
        let stats = VerificationStats {
            checkpoints_found: 6,
            checkpoints_mismatched: 6,
            first_mismatch_sequence: Some(1),
            ..Default::default()
        };
        let results: Vec<_> = (1..=6)
            .map(|i| VerificationResult::Mismatch {
                sequence: i,
                timestamp: i as i64 * 1000,
                expected: [0xAA; 32],
                actual: [0xBB; 32],
            })
            .collect();
        let report = ComparisonReport::from_stats(&stats, &results);

        assert_eq!(report.mismatch_count(), 6);

        // Should have systematic issue recommendation (triggers when mismatches > 5)
        let has_systematic = report
            .recommendations
            .iter()
            .any(|r| r.contains("systematic"));
        assert!(has_systematic);
    }

    #[test]
    fn test_match_rate_severity_levels() {
        // 100% match rate -> Info
        let stats_100 = VerificationStats {
            checkpoints_found: 10,
            checkpoints_matched: 10,
            ..Default::default()
        };
        let report_100 = ComparisonReport::from_stats(&stats_100, &[]);
        let has_info = report_100
            .entries
            .iter()
            .any(|e| e.message.contains("Match rate") && e.severity == Severity::Info);
        assert!(has_info);

        // 95% match rate -> Warning
        let stats_95 = VerificationStats {
            checkpoints_found: 20,
            checkpoints_matched: 19,
            checkpoints_mismatched: 1,
            ..Default::default()
        };
        let report_95 = ComparisonReport::from_stats(&stats_95, &[]);
        let has_warning = report_95
            .entries
            .iter()
            .any(|e| e.message.contains("Match rate") && e.severity == Severity::Warning);
        assert!(has_warning);

        // 50% match rate -> Error
        let stats_50 = VerificationStats {
            checkpoints_found: 10,
            checkpoints_matched: 5,
            checkpoints_mismatched: 5,
            ..Default::default()
        };
        let report_50 = ComparisonReport::from_stats(&stats_50, &[]);
        let has_error = report_50
            .entries
            .iter()
            .any(|e| e.message.contains("Match rate") && e.severity == Severity::Error);
        assert!(has_error);
    }

    // ==================== Debug Tests ====================

    #[test]
    fn test_severity_debug() {
        let debug = format!("{:?}", Severity::Info);
        assert_eq!(debug, "Info");
    }

    #[test]
    fn test_report_entry_debug() {
        let entry = ReportEntry::info("Test");
        let debug = format!("{:?}", entry);
        assert!(debug.contains("ReportEntry"));
    }

    #[test]
    fn test_report_status_debug() {
        let debug = format!("{:?}", ReportStatus::Pass);
        assert_eq!(debug, "Pass");
    }

    #[test]
    fn test_mismatch_detail_debug() {
        let detail = MismatchDetail {
            sequence: 1,
            timestamp: 1000,
            expected_hash: "aa".to_string(),
            actual_hash: "bb".to_string(),
        };
        let debug = format!("{:?}", detail);
        assert!(debug.contains("MismatchDetail"));
    }

    #[test]
    fn test_comparison_report_debug() {
        let report = ComparisonReport::new();
        let debug = format!("{:?}", report);
        assert!(debug.contains("ComparisonReport"));
    }

    // ==================== Clone Tests ====================

    #[test]
    fn test_severity_clone() {
        let severity = Severity::Error;
        let cloned = severity;
        assert_eq!(severity, cloned);
    }

    #[test]
    fn test_report_entry_clone() {
        let entry = ReportEntry::info("Test").with_sequence(1);
        let cloned = entry.clone();
        assert_eq!(cloned.message, "Test");
        assert_eq!(cloned.sequence, Some(1));
    }

    #[test]
    fn test_comparison_report_clone() {
        let report = ComparisonReport::pass(100, 5);
        let cloned = report.clone();
        assert_eq!(cloned.status, ReportStatus::Pass);
        assert_eq!(cloned.stats.events_processed, 100);
    }
}
