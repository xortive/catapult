//! Traffic monitor data models and types

use std::path::PathBuf;
use std::time::SystemTime;

use cat_mux::RadioHandle;
use cat_protocol::display::{AnnotatedFrame, SegmentType};
use egui::Color32;

/// Map SegmentType to UI color
pub(crate) fn segment_color(segment_type: SegmentType) -> Color32 {
    match segment_type {
        SegmentType::Preamble => Color32::from_rgb(128, 128, 128), // Gray
        SegmentType::Address => Color32::from_rgb(100, 180, 255),  // Light blue
        SegmentType::Command => Color32::from_rgb(255, 180, 100),  // Orange
        SegmentType::Frequency => Color32::from_rgb(255, 255, 100), // Yellow
        SegmentType::Mode => Color32::from_rgb(200, 150, 255),     // Light purple
        SegmentType::Status => Color32::from_rgb(255, 150, 200),   // Pink
        SegmentType::Data => Color32::from_rgb(100, 255, 180),     // Light green
        SegmentType::Terminator => Color32::from_rgb(128, 128, 128), // Gray
    }
}

/// Source of traffic data
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrafficSource {
    /// Real radio on a serial port (incoming)
    RealRadio { handle: RadioHandle, port: String },
    /// Command sent to real radio (outgoing to radio)
    ToRealRadio { handle: RadioHandle, port: String },
    /// Simulated radio (incoming)
    SimulatedRadio { id: String },
    /// Command sent to simulated radio (outgoing to radio)
    ToSimulatedRadio { id: String },
    /// Real amplifier on a serial port (outgoing to amp)
    RealAmplifier { port: String },
    /// Real amplifier on a serial port (incoming from amp)
    FromRealAmplifier { port: String },
    /// Virtual amplifier (outgoing to amp)
    SimulatedAmplifier,
    /// Virtual amplifier (incoming from amp)
    FromSimulatedAmplifier,
}

/// Severity level for diagnostic entries
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    /// Debug message
    Debug,
    /// Informational message
    Info,
    /// Warning
    Warning,
    /// Error
    Error,
}

/// A single traffic entry
#[derive(Debug, Clone)]
pub enum TrafficEntry {
    /// Data entry (normal traffic)
    Data {
        /// Timestamp
        timestamp: SystemTime,
        /// Direction
        direction: TrafficDirection,
        /// Traffic source
        source: TrafficSource,
        /// Raw data
        data: Vec<u8>,
        /// Decoded representation (from cache or computed on add)
        decoded: Option<AnnotatedFrame>,
    },
    /// Diagnostic entry (error or warning)
    Diagnostic {
        /// Timestamp
        timestamp: SystemTime,
        /// Source of the diagnostic
        source: String,
        /// Severity level
        severity: DiagnosticSeverity,
        /// Message
        message: String,
    },
}

impl TrafficEntry {
    /// Get the direction (None for diagnostics)
    pub fn direction(&self) -> Option<TrafficDirection> {
        match self {
            TrafficEntry::Data { direction, .. } => Some(*direction),
            TrafficEntry::Diagnostic { .. } => None,
        }
    }
}

/// Traffic direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrafficDirection {
    /// Incoming from radio
    Incoming,
    /// Outgoing to amplifier
    Outgoing,
}

/// Result of an export action from the traffic monitor
pub enum ExportAction {
    /// Copy log content to clipboard
    CopyToClipboard(String),
    /// Log was saved to a file
    SavedToFile(PathBuf),
    /// User cancelled the save dialog
    Cancelled,
    /// An error occurred
    Error(String),
}
