//! Traffic log export functionality

use std::path::PathBuf;
use std::time::SystemTime;

use super::models::{DiagnosticSeverity, TrafficDirection, TrafficEntry, TrafficSource};
use super::TrafficMonitor;

impl TrafficMonitor {
    /// Check if an entry passes the current filters
    pub(super) fn entry_passes_filter(&self, entry: &TrafficEntry) -> bool {
        // Diagnostic filtering - events are pre-filtered by tracing layer,
        // but we still need to check the master toggle
        if let TrafficEntry::Diagnostic { .. } = entry {
            // If diagnostic_level is None (off), hide all diagnostics
            // Otherwise, all diagnostics that arrive have already passed the tracing filter
            return self.diagnostic_level.is_some();
        }

        // Direction filter for data entries
        let direction_match = entry
            .direction()
            .map(|dir| self.filter_direction.is_none_or(|filter| dir == filter))
            .unwrap_or(true);

        // Simulated filter
        let sim_match = self.show_simulated || !entry.is_simulated();

        direction_match && sim_match
    }

    /// Format a timestamp for export
    fn format_timestamp(timestamp: &SystemTime) -> String {
        timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| {
                let secs = d.as_secs() % 86400;
                let hours = secs / 3600;
                let mins = (secs % 3600) / 60;
                let secs = secs % 60;
                let millis = d.subsec_millis();
                format!("{:02}:{:02}:{:02}.{:03}", hours, mins, secs, millis)
            })
            .unwrap_or_else(|_| "??:??:??.???".to_string())
    }

    /// Format an entry as a text line for export
    fn format_entry_for_export(entry: &TrafficEntry) -> String {
        match entry {
            TrafficEntry::Data {
                timestamp,
                direction,
                source,
                data,
                decoded,
            } => {
                let time = Self::format_timestamp(timestamp);
                let dir = match direction {
                    TrafficDirection::Incoming => "IN ",
                    TrafficDirection::Outgoing => "OUT",
                };
                let src = match source {
                    TrafficSource::RealRadio { port, .. } => format!("Radio({})", port),
                    TrafficSource::ToRealRadio { port, .. } => format!("->Radio({})", port),
                    TrafficSource::SimulatedRadio { id } => format!("Sim({})", id),
                    TrafficSource::ToSimulatedRadio { id } => format!("->Sim({})", id),
                    TrafficSource::RealAmplifier { port } => format!("->Amp({})", port),
                    TrafficSource::FromRealAmplifier { port } => format!("Amp({})", port),
                    TrafficSource::SimulatedAmplifier => "->SimAmp".to_string(),
                    TrafficSource::FromSimulatedAmplifier => "SimAmp".to_string(),
                };
                let hex: String = data
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect::<Vec<_>>()
                    .join(" ");
                let decoded_str = decoded
                    .as_ref()
                    .map(|d| {
                        let summary: String = d.summary.iter().map(|p| p.text.as_str()).collect();
                        format!(" [{}] {}", d.protocol, summary)
                    })
                    .unwrap_or_default();
                format!("{} {} {:12} {}{}", time, dir, src, hex, decoded_str)
            }
            TrafficEntry::Diagnostic {
                timestamp,
                source,
                severity,
                message,
            } => {
                let time = Self::format_timestamp(timestamp);
                let sev = match severity {
                    DiagnosticSeverity::Debug => "DEBUG",
                    DiagnosticSeverity::Info => "INFO ",
                    DiagnosticSeverity::Warning => "WARN ",
                    DiagnosticSeverity::Error => "ERROR",
                };
                format!("{} {} [{}] {}", time, sev, source, message)
            }
        }
    }

    /// Format the filtered log as a string
    pub fn format_filtered_log(&self) -> String {
        let filtered: Vec<_> = self
            .entries
            .iter()
            .filter(|e| self.entry_passes_filter(e))
            .collect();

        let mut output = String::new();
        output.push_str("# Catapult Traffic Log Export\n");
        output.push_str(&format!("# Entries: {}\n\n", filtered.len()));

        for entry in filtered {
            output.push_str(&Self::format_entry_for_export(entry));
            output.push('\n');
        }

        output
    }

    /// Save the filtered log to a user-selected file
    /// Returns Ok(Some(path)) on success, Ok(None) if cancelled, Err on failure
    pub fn save_filtered_log_with_dialog(&self) -> Result<Option<PathBuf>, String> {
        let default_name = format!(
            "traffic-log-{}.txt",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        );

        let path = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("Text files", &["txt"])
            .add_filter("All files", &["*"])
            .save_file();

        let Some(path) = path else {
            return Ok(None); // User cancelled
        };

        let content = self.format_filtered_log();
        std::fs::write(&path, content).map_err(|e| format!("Failed to write file: {}", e))?;

        Ok(Some(path))
    }
}
