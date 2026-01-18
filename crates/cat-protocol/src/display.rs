//! Display and annotation support for protocol frames
//!
//! This module provides types and traits for annotating decoded protocol frames
//! with human-readable information. The UI can use these annotations to display
//! colored hex dumps with tooltips and decoded summaries.

use std::ops::Range;

use crate::command::OperatingMode;
use crate::flex::{FlexCodec, FlexCommand, FlexMode};
use crate::icom::{CivCodec, CivCommand, CivCommandType, PREAMBLE, TERMINATOR};
use crate::kenwood::{KenwoodCodec, KenwoodCommand};
use crate::yaesu::YaesuCommand;
use crate::yaesu_ascii::{YaesuAsciiCodec, YaesuAsciiCommand};
use crate::ProtocolCodec;

/// Type of segment for UI coloring
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentType {
    /// Frame preamble/sync bytes
    Preamble,
    /// Device address
    Address,
    /// Command code/opcode
    Command,
    /// Frequency data
    Frequency,
    /// Operating mode
    Mode,
    /// Status/state information
    Status,
    /// Generic data
    Data,
    /// Frame terminator
    Terminator,
}

/// A segment of a decoded frame with annotation
#[derive(Debug, Clone)]
pub struct FrameSegment {
    /// Byte range in the original data
    pub range: Range<usize>,
    /// Label for this segment (e.g., "preamble", "addr", "freq")
    pub label: &'static str,
    /// Decoded value as a string (e.g., "14.250 MHz")
    pub value: String,
    /// Type of segment (UI maps this to colors)
    pub segment_type: SegmentType,
}

/// A part of the summary with semantic type
#[derive(Debug, Clone)]
pub struct SummaryPart {
    /// Text content
    pub text: String,
    /// Type of this part (UI maps this to colors)
    pub part_type: SegmentType,
    /// Optional byte range this summary part corresponds to (for hover linking)
    pub range: Option<Range<usize>>,
}

impl SummaryPart {
    /// Create a plain text summary part (uses Data type for default color)
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            part_type: SegmentType::Data,
            range: None,
        }
    }

    /// Create a summary part with specific type
    pub fn typed(text: impl Into<String>, part_type: SegmentType) -> Self {
        Self {
            text: text.into(),
            part_type,
            range: None,
        }
    }

    /// Create a summary part with type and byte range for hover linking
    pub fn with_range(text: impl Into<String>, part_type: SegmentType, range: Range<usize>) -> Self {
        Self {
            text: text.into(),
            part_type,
            range: Some(range),
        }
    }
}

/// Annotated frame ready for display
#[derive(Debug, Clone)]
pub struct AnnotatedFrame {
    /// Protocol name (e.g., "CI-V", "Yaesu", "Kenwood")
    pub protocol: &'static str,
    /// Summary parts with semantic types
    pub summary: Vec<SummaryPart>,
    /// Annotated byte segments
    pub segments: Vec<FrameSegment>,
}

/// Trait for commands that can describe their display representation
pub trait FrameAnnotation {
    /// Create an annotated frame from this command and its raw bytes
    fn annotate(&self, raw_bytes: &[u8]) -> AnnotatedFrame;
}

// ============================================================================
// Format Helpers
// ============================================================================

/// Format frequency in MHz with appropriate precision
pub fn format_frequency(hz: u64) -> String {
    let mhz = hz as f64 / 1_000_000.0;
    if hz.is_multiple_of(1000) {
        // kHz resolution
        format!("{:.3} MHz", mhz)
    } else {
        // Hz resolution
        format!("{:.6} MHz", mhz)
    }
}

/// Format an operating mode as a human-readable string
pub fn format_mode(mode: OperatingMode) -> &'static str {
    match mode {
        OperatingMode::Lsb => "LSB",
        OperatingMode::Usb => "USB",
        OperatingMode::Cw => "CW",
        OperatingMode::CwR => "CW-R",
        OperatingMode::Am => "AM",
        OperatingMode::Fm => "FM",
        OperatingMode::FmN => "FM-N",
        OperatingMode::Dig => "DIG",
        OperatingMode::DigU => "DIG-U",
        OperatingMode::DigL => "DIG-L",
        OperatingMode::Pkt => "PKT",
        OperatingMode::Data => "DATA",
        OperatingMode::DataU => "DATA-U",
        OperatingMode::DataL => "DATA-L",
        OperatingMode::Rtty => "RTTY",
        OperatingMode::RttyR => "RTTY-R",
    }
}

/// Format CI-V mode byte as a human-readable string
fn format_civ_mode(mode: u8) -> &'static str {
    match mode {
        0x00 => "LSB",
        0x01 => "USB",
        0x02 => "AM",
        0x03 => "CW",
        0x04 => "RTTY",
        0x05 => "FM",
        0x06 => "CW-R",
        0x07 => "RTTY-R",
        0x08 => "DATA-L",
        0x09 => "DATA-U",
        _ => "?",
    }
}

/// Format Yaesu mode byte as a human-readable string
fn format_yaesu_mode(mode: u8) -> &'static str {
    match mode {
        0x00 => "LSB",
        0x01 => "USB",
        0x02 => "CW",
        0x03 => "CW-R",
        0x04 => "AM",
        0x06 | 0x08 => "FM",
        0x0A => "DIG",
        0x0C => "PKT",
        _ => "?",
    }
}

/// Format Kenwood mode digit as a human-readable string
fn format_kenwood_mode(mode: u8) -> &'static str {
    match mode {
        1 => "LSB",
        2 => "USB",
        3 => "CW",
        4 => "FM",
        5 => "AM",
        6 => "RTTY",
        7 => "CW-R",
        8 => "DATA-L",
        9 => "RTTY-R",
        10 => "DATA-U",
        _ => "?",
    }
}

/// Format Yaesu ASCII mode digit as a human-readable string
fn format_yaesu_ascii_mode(mode: u8) -> &'static str {
    match mode {
        1 => "LSB",
        2 => "USB",
        3 => "CW-U",
        4 => "FM",
        5 => "AM",
        6 => "RTTY-L",
        7 => "CW-L",
        8 => "DATA-L",
        9 => "RTTY-U",
        10 => "DATA-FM",
        11 => "FM-N",
        12 => "DATA-U",
        13 => "AM-N",
        14 => "C4FM",
        _ => "?",
    }
}

// ============================================================================
// FrameAnnotation for CivCommand
// ============================================================================

/// Format CI-V address as descriptive string
fn format_civ_address(addr: u8) -> String {
    match addr {
        0x00 => "Broadcast".to_string(),
        0xE0 => "Controller".to_string(),
        _ => format!("Radio {:02X}h", addr),
    }
}

impl FrameAnnotation for CivCommand {
    fn annotate(&self, raw_bytes: &[u8]) -> AnnotatedFrame {
        let data_len = raw_bytes.len();
        let mut segments = Vec::new();

        // Preamble (bytes 0-1)
        segments.push(FrameSegment {
            range: 0..2,
            label: "preamble",
            value: "FE FE".to_string(),
            segment_type: SegmentType::Preamble,
        });

        // To address (byte 2) - destination
        segments.push(FrameSegment {
            range: 2..3,
            label: "dest",
            value: format_civ_address(self.to_addr),
            segment_type: SegmentType::Address,
        });

        // From address (byte 3) - source
        segments.push(FrameSegment {
            range: 3..4,
            label: "src",
            value: format_civ_address(self.from_addr),
            segment_type: SegmentType::Address,
        });

        // Command (byte 4)
        let cmd_byte = raw_bytes.get(4).copied().unwrap_or(0);
        let cmd_range = 4..5;
        segments.push(FrameSegment {
            range: cmd_range.clone(),
            label: "cmd",
            value: format!("{:02X}", cmd_byte),
            segment_type: SegmentType::Command,
        });

        // Terminator (last byte)
        segments.push(FrameSegment {
            range: (data_len - 1)..data_len,
            label: "end",
            value: "FD".to_string(),
            segment_type: SegmentType::Terminator,
        });

        let summary = match &self.command {
            CivCommandType::SetFrequency { hz } => {
                let freq_range = if data_len > 6 {
                    segments.push(FrameSegment {
                        range: 5..(data_len - 1),
                        label: "freq",
                        value: format_frequency(*hz),
                        segment_type: SegmentType::Frequency,
                    });
                    Some(5..(data_len - 1))
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("Set Freq", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = freq_range {
                        SummaryPart::with_range(format_frequency(*hz), SegmentType::Frequency, r)
                    } else {
                        SummaryPart::typed(format_frequency(*hz), SegmentType::Frequency)
                    },
                ]
            }
            CivCommandType::GetFrequency => vec![SummaryPart::with_range("Get Freq", SegmentType::Command, cmd_range)],
            CivCommandType::FrequencyReport { hz } => {
                let freq_range = if data_len > 6 {
                    segments.push(FrameSegment {
                        range: 5..(data_len - 1),
                        label: "freq",
                        value: format_frequency(*hz),
                        segment_type: SegmentType::Frequency,
                    });
                    Some(5..(data_len - 1))
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("Freq", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = freq_range {
                        SummaryPart::with_range(format_frequency(*hz), SegmentType::Frequency, r)
                    } else {
                        SummaryPart::typed(format_frequency(*hz), SegmentType::Frequency)
                    },
                ]
            }
            CivCommandType::SetMode { mode, filter } => {
                let mode_range = if data_len > 6 {
                    segments.push(FrameSegment {
                        range: 5..6,
                        label: "mode",
                        value: format_civ_mode(*mode).to_string(),
                        segment_type: SegmentType::Mode,
                    });
                    Some(5..6)
                } else {
                    None
                };
                let filter_range = if data_len > 7 {
                    segments.push(FrameSegment {
                        range: 6..7,
                        label: "filter",
                        value: format!("{}", filter),
                        segment_type: SegmentType::Data,
                    });
                    Some(6..7)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("Set Mode", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = mode_range {
                        SummaryPart::with_range(format_civ_mode(*mode), SegmentType::Mode, r)
                    } else {
                        SummaryPart::typed(format_civ_mode(*mode), SegmentType::Mode)
                    },
                    SummaryPart::plain(" (filter "),
                    if let Some(r) = filter_range {
                        SummaryPart::with_range(format!("{}", filter), SegmentType::Data, r)
                    } else {
                        SummaryPart::typed(format!("{}", filter), SegmentType::Data)
                    },
                    SummaryPart::plain(")"),
                ]
            }
            CivCommandType::GetMode => vec![SummaryPart::with_range("Get Mode", SegmentType::Command, cmd_range)],
            CivCommandType::ModeReport { mode, filter } => {
                let mode_range = if data_len > 6 {
                    segments.push(FrameSegment {
                        range: 5..6,
                        label: "mode",
                        value: format_civ_mode(*mode).to_string(),
                        segment_type: SegmentType::Mode,
                    });
                    Some(5..6)
                } else {
                    None
                };
                let filter_range = if data_len > 7 {
                    segments.push(FrameSegment {
                        range: 6..7,
                        label: "filter",
                        value: format!("{}", filter),
                        segment_type: SegmentType::Data,
                    });
                    Some(6..7)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("Mode", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = mode_range {
                        SummaryPart::with_range(format_civ_mode(*mode), SegmentType::Mode, r)
                    } else {
                        SummaryPart::typed(format_civ_mode(*mode), SegmentType::Mode)
                    },
                    SummaryPart::plain(" (filter "),
                    if let Some(r) = filter_range {
                        SummaryPart::with_range(format!("{}", filter), SegmentType::Data, r)
                    } else {
                        SummaryPart::typed(format!("{}", filter), SegmentType::Data)
                    },
                    SummaryPart::plain(")"),
                ]
            }
            CivCommandType::VfoSelect { vfo } => {
                let vfo_name = match vfo {
                    0x00 => "A",
                    0x01 => "B",
                    _ => "?",
                };
                let vfo_range = if data_len > 6 {
                    segments.push(FrameSegment {
                        range: 5..6,
                        label: "vfo",
                        value: vfo_name.to_string(),
                        segment_type: SegmentType::Data,
                    });
                    Some(5..6)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("VFO", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = vfo_range {
                        SummaryPart::with_range(vfo_name, SegmentType::Data, r)
                    } else {
                        SummaryPart::typed(vfo_name, SegmentType::Data)
                    },
                ]
            }
            CivCommandType::SetPtt { on } => {
                let state = if *on { "ON" } else { "OFF" };
                let state_range = if data_len > 7 {
                    segments.push(FrameSegment {
                        range: 5..6,
                        label: "subcmd",
                        value: "PTT".to_string(),
                        segment_type: SegmentType::Command,
                    });
                    segments.push(FrameSegment {
                        range: 6..7,
                        label: "state",
                        value: state.to_string(),
                        segment_type: SegmentType::Status,
                    });
                    Some(6..7)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("PTT", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = state_range {
                        SummaryPart::with_range(state, SegmentType::Status, r)
                    } else {
                        SummaryPart::typed(state, SegmentType::Status)
                    },
                ]
            }
            CivCommandType::PttReport { on } => {
                let state = if *on { "ON" } else { "OFF" };
                vec![
                    SummaryPart::with_range("PTT", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    SummaryPart::typed(state, SegmentType::Status),
                ]
            }
            CivCommandType::Split { on } => {
                let state = if *on { "ON" } else { "OFF" };
                let split_range = if data_len > 6 {
                    segments.push(FrameSegment {
                        range: 5..6,
                        label: "split",
                        value: state.to_string(),
                        segment_type: SegmentType::Status,
                    });
                    Some(5..6)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("Split", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = split_range {
                        SummaryPart::with_range(state, SegmentType::Status, r)
                    } else {
                        SummaryPart::typed(state, SegmentType::Status)
                    },
                ]
            }
            CivCommandType::Transceive { enabled } => {
                let state = if *enabled { "ON" } else { "OFF" };
                let state_range = if data_len > 7 {
                    segments.push(FrameSegment {
                        range: 5..6,
                        label: "subcmd",
                        value: "Transceive".to_string(),
                        segment_type: SegmentType::Command,
                    });
                    segments.push(FrameSegment {
                        range: 6..7,
                        label: "state",
                        value: state.to_string(),
                        segment_type: SegmentType::Status,
                    });
                    Some(6..7)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("Transceive", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = state_range {
                        SummaryPart::with_range(state, SegmentType::Status, r)
                    } else {
                        SummaryPart::typed(state, SegmentType::Status)
                    },
                ]
            }
            CivCommandType::Ok => vec![SummaryPart::with_range("OK", SegmentType::Data, cmd_range)],
            CivCommandType::Ng => vec![SummaryPart::with_range("NG (Error)", SegmentType::Status, cmd_range)],
            CivCommandType::Unknown {
                cmd,
                subcmd,
                data: cmd_data,
            } => {
                if data_len > 6 {
                    segments.push(FrameSegment {
                        range: 5..(data_len - 1),
                        label: "data",
                        value: format!("{} bytes", cmd_data.len()),
                        segment_type: SegmentType::Data,
                    });
                }
                if let Some(sc) = subcmd {
                    vec![SummaryPart::with_range(format!("cmd={:02X} sub={:02X}", cmd, sc), SegmentType::Command, cmd_range)]
                } else {
                    vec![SummaryPart::with_range(format!("cmd={:02X}", cmd), SegmentType::Command, cmd_range)]
                }
            }
        };

        AnnotatedFrame {
            protocol: "CI-V",
            summary,
            segments,
        }
    }
}

// ============================================================================
// FrameAnnotation for YaesuCommand
// ============================================================================

impl FrameAnnotation for YaesuCommand {
    fn annotate(&self, raw_bytes: &[u8]) -> AnnotatedFrame {
        let mut segments = Vec::new();

        // Helper to add command segment at byte 4 (for commands, not responses)
        let add_cmd_segment = |segments: &mut Vec<FrameSegment>, opcode: u8| {
            segments.push(FrameSegment {
                range: 4..5,
                label: "cmd",
                value: format!("{:02X}", opcode),
                segment_type: SegmentType::Command,
            });
        };

        let summary = match self {
            YaesuCommand::SetFrequency { hz } => {
                let freq_range = 0..4;
                let cmd_range = 4..5;
                segments.push(FrameSegment {
                    range: freq_range.clone(),
                    label: "freq",
                    value: format_frequency(*hz),
                    segment_type: SegmentType::Frequency,
                });
                add_cmd_segment(&mut segments, raw_bytes.get(4).copied().unwrap_or(0));
                vec![
                    SummaryPart::with_range("Set Freq", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    SummaryPart::with_range(format_frequency(*hz), SegmentType::Frequency, freq_range),
                ]
            }
            YaesuCommand::GetFrequencyMode => {
                let cmd_range = 4..5;
                segments.push(FrameSegment {
                    range: 0..4,
                    label: "params",
                    value: "query".to_string(),
                    segment_type: SegmentType::Data,
                });
                add_cmd_segment(&mut segments, raw_bytes.get(4).copied().unwrap_or(0));
                vec![SummaryPart::with_range("Get Freq/Mode", SegmentType::Command, cmd_range)]
            }
            YaesuCommand::FrequencyModeReport { hz, mode } => {
                // Response: byte 4 is MODE, not opcode - no command segment
                let freq_range = 0..4;
                let mode_range = 4..5;
                segments.push(FrameSegment {
                    range: freq_range.clone(),
                    label: "freq",
                    value: format_frequency(*hz),
                    segment_type: SegmentType::Frequency,
                });
                segments.push(FrameSegment {
                    range: mode_range.clone(),
                    label: "mode",
                    value: format_yaesu_mode(*mode).to_string(),
                    segment_type: SegmentType::Mode,
                });
                vec![
                    SummaryPart::typed("Freq", SegmentType::Data),
                    SummaryPart::plain(" "),
                    SummaryPart::with_range(format_frequency(*hz), SegmentType::Frequency, freq_range),
                    SummaryPart::plain(" "),
                    SummaryPart::with_range(format_yaesu_mode(*mode), SegmentType::Mode, mode_range),
                ]
            }
            YaesuCommand::SetMode { mode } => {
                let mode_range = 0..1;
                let cmd_range = 4..5;
                segments.push(FrameSegment {
                    range: mode_range.clone(),
                    label: "mode",
                    value: format_yaesu_mode(*mode).to_string(),
                    segment_type: SegmentType::Mode,
                });
                segments.push(FrameSegment {
                    range: 1..4,
                    label: "padding",
                    value: String::new(),
                    segment_type: SegmentType::Preamble,
                });
                add_cmd_segment(&mut segments, raw_bytes.get(4).copied().unwrap_or(0));
                vec![
                    SummaryPart::with_range("Set Mode", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    SummaryPart::with_range(format_yaesu_mode(*mode), SegmentType::Mode, mode_range),
                ]
            }
            YaesuCommand::PttOn => {
                let cmd_range = 4..5;
                segments.push(FrameSegment {
                    range: 0..4,
                    label: "padding",
                    value: String::new(),
                    segment_type: SegmentType::Preamble,
                });
                add_cmd_segment(&mut segments, raw_bytes.get(4).copied().unwrap_or(0));
                vec![
                    SummaryPart::with_range("PTT", SegmentType::Command, cmd_range.clone()),
                    SummaryPart::plain(" "),
                    SummaryPart::with_range("ON", SegmentType::Status, cmd_range),
                ]
            }
            YaesuCommand::PttOff => {
                let cmd_range = 4..5;
                segments.push(FrameSegment {
                    range: 0..4,
                    label: "padding",
                    value: String::new(),
                    segment_type: SegmentType::Preamble,
                });
                add_cmd_segment(&mut segments, raw_bytes.get(4).copied().unwrap_or(0));
                vec![
                    SummaryPart::with_range("PTT", SegmentType::Command, cmd_range.clone()),
                    SummaryPart::plain(" "),
                    SummaryPart::with_range("OFF", SegmentType::Status, cmd_range),
                ]
            }
            YaesuCommand::ToggleVfo => {
                let cmd_range = 4..5;
                segments.push(FrameSegment {
                    range: 0..4,
                    label: "padding",
                    value: String::new(),
                    segment_type: SegmentType::Preamble,
                });
                add_cmd_segment(&mut segments, raw_bytes.get(4).copied().unwrap_or(0));
                vec![SummaryPart::with_range("Toggle VFO", SegmentType::Command, cmd_range)]
            }
            YaesuCommand::SplitOn => {
                let cmd_range = 4..5;
                segments.push(FrameSegment {
                    range: 0..4,
                    label: "padding",
                    value: String::new(),
                    segment_type: SegmentType::Preamble,
                });
                add_cmd_segment(&mut segments, raw_bytes.get(4).copied().unwrap_or(0));
                vec![
                    SummaryPart::with_range("Split", SegmentType::Command, cmd_range.clone()),
                    SummaryPart::plain(" "),
                    SummaryPart::with_range("ON", SegmentType::Status, cmd_range),
                ]
            }
            YaesuCommand::SplitOff => {
                let cmd_range = 4..5;
                segments.push(FrameSegment {
                    range: 0..4,
                    label: "padding",
                    value: String::new(),
                    segment_type: SegmentType::Preamble,
                });
                add_cmd_segment(&mut segments, raw_bytes.get(4).copied().unwrap_or(0));
                vec![
                    SummaryPart::with_range("Split", SegmentType::Command, cmd_range.clone()),
                    SummaryPart::plain(" "),
                    SummaryPart::with_range("OFF", SegmentType::Status, cmd_range),
                ]
            }
            YaesuCommand::ReadRxStatus => {
                let cmd_range = 4..5;
                segments.push(FrameSegment {
                    range: 0..4,
                    label: "padding",
                    value: String::new(),
                    segment_type: SegmentType::Preamble,
                });
                add_cmd_segment(&mut segments, raw_bytes.get(4).copied().unwrap_or(0));
                vec![SummaryPart::with_range("Read RX Status", SegmentType::Command, cmd_range)]
            }
            YaesuCommand::RxStatusReport { status } => {
                // Response: bytes 0-4 contain status data, byte 4 is part of status
                let data_range = 0..4;
                let status_range = 4..5;
                segments.push(FrameSegment {
                    range: data_range,
                    label: "data",
                    value: "status data".to_string(),
                    segment_type: SegmentType::Data,
                });
                segments.push(FrameSegment {
                    range: status_range.clone(),
                    label: "status",
                    value: format!("{:02X}", status),
                    segment_type: SegmentType::Status,
                });
                vec![
                    SummaryPart::typed("RX Status", SegmentType::Data),
                    SummaryPart::plain(" "),
                    SummaryPart::with_range(format!("{:02X}", status), SegmentType::Status, status_range),
                ]
            }
            YaesuCommand::ReadTxStatus => {
                let cmd_range = 4..5;
                segments.push(FrameSegment {
                    range: 0..4,
                    label: "padding",
                    value: String::new(),
                    segment_type: SegmentType::Preamble,
                });
                add_cmd_segment(&mut segments, raw_bytes.get(4).copied().unwrap_or(0));
                vec![SummaryPart::with_range("Read TX Status", SegmentType::Command, cmd_range)]
            }
            YaesuCommand::TxStatusReport { status } => {
                // Response: bytes 0-4 contain status data, byte 4 is part of status
                let data_range = 0..4;
                let status_range = 4..5;
                segments.push(FrameSegment {
                    range: data_range,
                    label: "data",
                    value: "status data".to_string(),
                    segment_type: SegmentType::Data,
                });
                segments.push(FrameSegment {
                    range: status_range.clone(),
                    label: "status",
                    value: format!("{:02X}", status),
                    segment_type: SegmentType::Status,
                });
                vec![
                    SummaryPart::typed("TX Status", SegmentType::Data),
                    SummaryPart::plain(" "),
                    SummaryPart::with_range(format!("{:02X}", status), SegmentType::Status, status_range),
                ]
            }
            YaesuCommand::PowerOn => {
                let cmd_range = 4..5;
                segments.push(FrameSegment {
                    range: 0..4,
                    label: "padding",
                    value: String::new(),
                    segment_type: SegmentType::Preamble,
                });
                add_cmd_segment(&mut segments, raw_bytes.get(4).copied().unwrap_or(0));
                vec![
                    SummaryPart::with_range("Power", SegmentType::Command, cmd_range.clone()),
                    SummaryPart::plain(" "),
                    SummaryPart::with_range("ON", SegmentType::Status, cmd_range),
                ]
            }
            YaesuCommand::PowerOff => {
                let cmd_range = 4..5;
                segments.push(FrameSegment {
                    range: 0..4,
                    label: "padding",
                    value: String::new(),
                    segment_type: SegmentType::Preamble,
                });
                add_cmd_segment(&mut segments, raw_bytes.get(4).copied().unwrap_or(0));
                vec![
                    SummaryPart::with_range("Power", SegmentType::Command, cmd_range.clone()),
                    SummaryPart::plain(" "),
                    SummaryPart::with_range("OFF", SegmentType::Status, cmd_range),
                ]
            }
            YaesuCommand::LockOn => {
                let cmd_range = 4..5;
                segments.push(FrameSegment {
                    range: 0..4,
                    label: "padding",
                    value: String::new(),
                    segment_type: SegmentType::Preamble,
                });
                add_cmd_segment(&mut segments, raw_bytes.get(4).copied().unwrap_or(0));
                vec![
                    SummaryPart::with_range("Lock", SegmentType::Command, cmd_range.clone()),
                    SummaryPart::plain(" "),
                    SummaryPart::with_range("ON", SegmentType::Status, cmd_range),
                ]
            }
            YaesuCommand::LockOff => {
                let cmd_range = 4..5;
                segments.push(FrameSegment {
                    range: 0..4,
                    label: "padding",
                    value: String::new(),
                    segment_type: SegmentType::Preamble,
                });
                add_cmd_segment(&mut segments, raw_bytes.get(4).copied().unwrap_or(0));
                vec![
                    SummaryPart::with_range("Lock", SegmentType::Command, cmd_range.clone()),
                    SummaryPart::plain(" "),
                    SummaryPart::with_range("OFF", SegmentType::Status, cmd_range),
                ]
            }
            YaesuCommand::Unknown { bytes } => {
                let cmd_range = 4..5;
                segments.push(FrameSegment {
                    range: 0..4,
                    label: "data",
                    value: "params".to_string(),
                    segment_type: SegmentType::Data,
                });
                add_cmd_segment(&mut segments, bytes.get(4).copied().unwrap_or(0));
                vec![SummaryPart::with_range(format!("cmd={:02X}", bytes[4]), SegmentType::Command, cmd_range)]
            }
        };

        AnnotatedFrame {
            protocol: "Yaesu",
            summary,
            segments,
        }
    }
}

// ============================================================================
// FrameAnnotation for KenwoodCommand
// ============================================================================

impl FrameAnnotation for KenwoodCommand {
    fn annotate(&self, raw_bytes: &[u8]) -> AnnotatedFrame {
        let data_len = raw_bytes.len();
        let has_terminator = raw_bytes.last() == Some(&b';');
        let mut segments = Vec::new();

        // Parse command prefix from raw bytes
        let cmd_str = std::str::from_utf8(raw_bytes).unwrap_or("");
        let prefix = if cmd_str.len() >= 2 {
            &cmd_str[..2]
        } else {
            ""
        };

        // Detect FlexRadio ZZ prefix
        let is_flex = prefix == "ZZ" && cmd_str.len() >= 4;

        // Command prefix (first 2 bytes)
        segments.push(FrameSegment {
            range: 0..2,
            label: "cmd",
            value: prefix.to_string(),
            segment_type: SegmentType::Command,
        });

        let (protocol, summary) = if is_flex {
            let zz_cmd = &cmd_str[2..4];
            segments.push(FrameSegment {
                range: 2..4,
                label: "subcmd",
                value: zz_cmd.to_string(),
                segment_type: SegmentType::Command,
            });

            let summary = self.create_kenwood_summary(raw_bytes, &mut segments, 4);
            ("Flex", summary)
        } else {
            let summary = self.create_kenwood_summary(raw_bytes, &mut segments, 2);
            ("Kenwood", summary)
        };

        // Terminator if present
        if has_terminator {
            segments.push(FrameSegment {
                range: (data_len - 1)..data_len,
                label: "end",
                value: ";".to_string(),
                segment_type: SegmentType::Terminator,
            });
        }

        AnnotatedFrame {
            protocol,
            summary,
            segments,
        }
    }
}

impl KenwoodCommand {
    fn create_kenwood_summary(
        &self,
        raw_bytes: &[u8],
        segments: &mut Vec<FrameSegment>,
        params_start: usize,
    ) -> Vec<SummaryPart> {
        let data_len = raw_bytes.len();
        let has_terminator = raw_bytes.last() == Some(&b';');
        let params_end = if has_terminator {
            data_len - 1
        } else {
            data_len
        };

        // Command range is 0..params_start (either "FA" at 0..2 or "ZZFA" at 0..4)
        let cmd_range = 0..params_start;

        match self {
            KenwoodCommand::FrequencyA(Some(hz)) | KenwoodCommand::FrequencyB(Some(hz)) => {
                let vfo = if matches!(self, KenwoodCommand::FrequencyA(_)) {
                    "A"
                } else {
                    "B"
                };
                let freq_range = if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "freq",
                        value: format_frequency(*hz),
                        segment_type: SegmentType::Frequency,
                    });
                    Some(params_start..params_end)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range(format!("VFO {}", vfo), SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = freq_range {
                        SummaryPart::with_range(format_frequency(*hz), SegmentType::Frequency, r)
                    } else {
                        SummaryPart::typed(format_frequency(*hz), SegmentType::Frequency)
                    },
                ]
            }
            KenwoodCommand::FrequencyA(None) | KenwoodCommand::FrequencyB(None) => {
                let vfo = if matches!(self, KenwoodCommand::FrequencyA(_)) {
                    "A"
                } else {
                    "B"
                };
                vec![SummaryPart::with_range(format!("Get Freq VFO {}", vfo), SegmentType::Command, cmd_range)]
            }
            KenwoodCommand::Mode(Some(m)) => {
                let mode_range = if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "mode",
                        value: format_kenwood_mode(*m).to_string(),
                        segment_type: SegmentType::Mode,
                    });
                    Some(params_start..params_end)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("Mode", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = mode_range {
                        SummaryPart::with_range(format_kenwood_mode(*m), SegmentType::Mode, r)
                    } else {
                        SummaryPart::typed(format_kenwood_mode(*m), SegmentType::Mode)
                    },
                ]
            }
            KenwoodCommand::Mode(None) => vec![SummaryPart::with_range("Get Mode", SegmentType::Command, cmd_range)],
            KenwoodCommand::Transmit(Some(true)) => vec![
                SummaryPart::with_range("PTT", SegmentType::Command, cmd_range),
                SummaryPart::plain(" "),
                SummaryPart::typed("ON", SegmentType::Status),
            ],
            KenwoodCommand::Transmit(Some(false)) | KenwoodCommand::Receive => vec![
                SummaryPart::with_range("PTT", SegmentType::Command, cmd_range),
                SummaryPart::plain(" "),
                SummaryPart::typed("OFF", SegmentType::Status),
            ],
            KenwoodCommand::Transmit(None) => vec![SummaryPart::with_range("Get PTT", SegmentType::Command, cmd_range)],
            KenwoodCommand::Id(Some(id)) => {
                let id_range = if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "id",
                        value: id.clone(),
                        segment_type: SegmentType::Data,
                    });
                    Some(params_start..params_end)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("ID", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = id_range {
                        SummaryPart::with_range(id, SegmentType::Data, r)
                    } else {
                        SummaryPart::typed(id, SegmentType::Data)
                    },
                ]
            }
            KenwoodCommand::Id(None) => vec![SummaryPart::with_range("Get ID", SegmentType::Command, cmd_range)],
            KenwoodCommand::Info(Some(info)) => {
                // Frequency at bytes 2-12 (11 digits)
                let freq_range = if params_start + 11 <= params_end {
                    segments.push(FrameSegment {
                        range: params_start..(params_start + 11),
                        label: "freq",
                        value: format_frequency(info.frequency_hz),
                        segment_type: SegmentType::Frequency,
                    });
                    Some(params_start..(params_start + 11))
                } else {
                    None
                };
                if params_start + 11 < params_end {
                    segments.push(FrameSegment {
                        range: (params_start + 11)..params_end,
                        label: "status",
                        value: "flags".to_string(),
                        segment_type: SegmentType::Status,
                    });
                }
                vec![
                    SummaryPart::with_range("Status", SegmentType::Command, cmd_range),
                    SummaryPart::plain(": "),
                    if let Some(r) = freq_range {
                        SummaryPart::with_range(format_frequency(info.frequency_hz), SegmentType::Frequency, r)
                    } else {
                        SummaryPart::typed(format_frequency(info.frequency_hz), SegmentType::Frequency)
                    },
                ]
            }
            KenwoodCommand::Info(None) => vec![SummaryPart::with_range("Get Status", SegmentType::Command, cmd_range)],
            KenwoodCommand::VfoSelect(Some(v)) => {
                let vfo = if *v == 0 { "A" } else { "B" };
                let vfo_range = if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "vfo",
                        value: vfo.to_string(),
                        segment_type: SegmentType::Data,
                    });
                    Some(params_start..params_end)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("VFO", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = vfo_range {
                        SummaryPart::with_range(vfo, SegmentType::Data, r)
                    } else {
                        SummaryPart::typed(vfo, SegmentType::Data)
                    },
                ]
            }
            KenwoodCommand::VfoSelect(None) => vec![SummaryPart::with_range("Get VFO", SegmentType::Command, cmd_range)],
            KenwoodCommand::Split(Some(s)) => {
                let state = if *s { "ON" } else { "OFF" };
                let split_range = if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "split",
                        value: state.to_string(),
                        segment_type: SegmentType::Status,
                    });
                    Some(params_start..params_end)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("Split", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = split_range {
                        SummaryPart::with_range(state, SegmentType::Status, r)
                    } else {
                        SummaryPart::typed(state, SegmentType::Status)
                    },
                ]
            }
            KenwoodCommand::Split(None) => vec![SummaryPart::with_range("Get Split", SegmentType::Command, cmd_range)],
            KenwoodCommand::Power(Some(on)) => {
                let state = if *on { "ON" } else { "OFF" };
                let power_range = if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "power",
                        value: state.to_string(),
                        segment_type: SegmentType::Status,
                    });
                    Some(params_start..params_end)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("Power", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = power_range {
                        SummaryPart::with_range(state, SegmentType::Status, r)
                    } else {
                        SummaryPart::typed(state, SegmentType::Status)
                    },
                ]
            }
            KenwoodCommand::Power(None) => vec![SummaryPart::with_range("Get Power", SegmentType::Command, cmd_range)],
            KenwoodCommand::AutoInfo(Some(enabled)) => {
                let state = if *enabled { "ON" } else { "OFF" };
                let ai_range = if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "state",
                        value: state.to_string(),
                        segment_type: SegmentType::Status,
                    });
                    Some(params_start..params_end)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("Auto Info", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = ai_range {
                        SummaryPart::with_range(state, SegmentType::Status, r)
                    } else {
                        SummaryPart::typed(state, SegmentType::Status)
                    },
                ]
            }
            KenwoodCommand::AutoInfo(None) => vec![SummaryPart::with_range("Get Auto Info", SegmentType::Command, cmd_range)],
            KenwoodCommand::Unknown(s) => {
                if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "params",
                        value: s[2..].to_string(),
                        segment_type: SegmentType::Data,
                    });
                }
                vec![SummaryPart::with_range(s, SegmentType::Command, cmd_range)]
            }
        }
    }
}

// ============================================================================
// FrameAnnotation for FlexCommand
// ============================================================================

impl FrameAnnotation for FlexCommand {
    fn annotate(&self, raw_bytes: &[u8]) -> AnnotatedFrame {
        let data_len = raw_bytes.len();
        let has_terminator = raw_bytes.last() == Some(&b';');
        let mut segments = Vec::new();

        // Parse command prefix from raw bytes
        let cmd_str = std::str::from_utf8(raw_bytes).unwrap_or("");

        // Determine if ZZ-prefixed (4-byte cmd) or standard (2-byte cmd)
        let is_zz = cmd_str.starts_with("ZZ");
        let cmd_len = if is_zz { 4 } else { 2 };
        let params_start = cmd_len;
        let params_end = if has_terminator {
            data_len - 1
        } else {
            data_len
        };

        // Command prefix
        let cmd_range = 0..cmd_len;
        segments.push(FrameSegment {
            range: cmd_range.clone(),
            label: "cmd",
            value: cmd_str.get(..cmd_len).unwrap_or("").to_string(),
            segment_type: SegmentType::Command,
        });

        let summary = match self {
            // Delegate Kenwood-wrapped commands
            FlexCommand::Kenwood(kw) => {
                match kw {
                    KenwoodCommand::FrequencyA(Some(hz)) | KenwoodCommand::FrequencyB(Some(hz)) => {
                        let vfo = if matches!(kw, KenwoodCommand::FrequencyA(_)) {
                            "A"
                        } else {
                            "B"
                        };
                        let freq_range = if params_start < params_end {
                            segments.push(FrameSegment {
                                range: params_start..params_end,
                                label: "freq",
                                value: format_frequency(*hz),
                                segment_type: SegmentType::Frequency,
                            });
                            Some(params_start..params_end)
                        } else {
                            None
                        };
                        vec![
                            SummaryPart::with_range(format!("VFO {}", vfo), SegmentType::Command, cmd_range),
                            SummaryPart::plain(" "),
                            if let Some(r) = freq_range {
                                SummaryPart::with_range(format_frequency(*hz), SegmentType::Frequency, r)
                            } else {
                                SummaryPart::typed(format_frequency(*hz), SegmentType::Frequency)
                            },
                        ]
                    }
                    KenwoodCommand::FrequencyA(None) | KenwoodCommand::FrequencyB(None) => {
                        let vfo = if matches!(kw, KenwoodCommand::FrequencyA(_)) {
                            "A"
                        } else {
                            "B"
                        };
                        vec![SummaryPart::with_range(format!("Get Freq VFO {}", vfo), SegmentType::Command, cmd_range)]
                    }
                    KenwoodCommand::Transmit(Some(true)) => vec![
                        SummaryPart::with_range("PTT", SegmentType::Command, cmd_range),
                        SummaryPart::plain(" "),
                        SummaryPart::typed("ON", SegmentType::Status),
                    ],
                    KenwoodCommand::Transmit(Some(false)) | KenwoodCommand::Receive => vec![
                        SummaryPart::with_range("PTT", SegmentType::Command, cmd_range),
                        SummaryPart::plain(" "),
                        SummaryPart::typed("OFF", SegmentType::Status),
                    ],
                    KenwoodCommand::Transmit(None) => vec![SummaryPart::with_range("Get PTT", SegmentType::Command, cmd_range)],
                    KenwoodCommand::Id(Some(id)) => {
                        let id_range = if params_start < params_end {
                            segments.push(FrameSegment {
                                range: params_start..params_end,
                                label: "id",
                                value: id.clone(),
                                segment_type: SegmentType::Data,
                            });
                            Some(params_start..params_end)
                        } else {
                            None
                        };
                        vec![
                            SummaryPart::with_range("ID", SegmentType::Command, cmd_range),
                            SummaryPart::plain(" "),
                            if let Some(r) = id_range {
                                SummaryPart::with_range(id, SegmentType::Data, r)
                            } else {
                                SummaryPart::typed(id, SegmentType::Data)
                            },
                        ]
                    }
                    KenwoodCommand::Id(None) => vec![SummaryPart::with_range("Get ID", SegmentType::Command, cmd_range)],
                    KenwoodCommand::VfoSelect(Some(v)) => {
                        let vfo = if *v == 0 { "A" } else { "B" };
                        let vfo_range = if params_start < params_end {
                            segments.push(FrameSegment {
                                range: params_start..params_end,
                                label: "vfo",
                                value: vfo.to_string(),
                                segment_type: SegmentType::Data,
                            });
                            Some(params_start..params_end)
                        } else {
                            None
                        };
                        vec![
                            SummaryPart::with_range("VFO", SegmentType::Command, cmd_range),
                            SummaryPart::plain(" "),
                            if let Some(r) = vfo_range {
                                SummaryPart::with_range(vfo, SegmentType::Data, r)
                            } else {
                                SummaryPart::typed(vfo, SegmentType::Data)
                            },
                        ]
                    }
                    KenwoodCommand::VfoSelect(None) => vec![SummaryPart::with_range("Get VFO", SegmentType::Command, cmd_range)],
                    KenwoodCommand::Split(Some(s)) => {
                        let state = if *s { "ON" } else { "OFF" };
                        let split_range = if params_start < params_end {
                            segments.push(FrameSegment {
                                range: params_start..params_end,
                                label: "split",
                                value: state.to_string(),
                                segment_type: SegmentType::Status,
                            });
                            Some(params_start..params_end)
                        } else {
                            None
                        };
                        vec![
                            SummaryPart::with_range("Split", SegmentType::Command, cmd_range),
                            SummaryPart::plain(" "),
                            if let Some(r) = split_range {
                                SummaryPart::with_range(state, SegmentType::Status, r)
                            } else {
                                SummaryPart::typed(state, SegmentType::Status)
                            },
                        ]
                    }
                    KenwoodCommand::Split(None) => vec![SummaryPart::with_range("Get Split", SegmentType::Command, cmd_range)],
                    KenwoodCommand::Power(Some(on)) => {
                        let state = if *on { "ON" } else { "OFF" };
                        let power_range = if params_start < params_end {
                            segments.push(FrameSegment {
                                range: params_start..params_end,
                                label: "power",
                                value: state.to_string(),
                                segment_type: SegmentType::Status,
                            });
                            Some(params_start..params_end)
                        } else {
                            None
                        };
                        vec![
                            SummaryPart::with_range("Power", SegmentType::Command, cmd_range),
                            SummaryPart::plain(" "),
                            if let Some(r) = power_range {
                                SummaryPart::with_range(state, SegmentType::Status, r)
                            } else {
                                SummaryPart::typed(state, SegmentType::Status)
                            },
                        ]
                    }
                    KenwoodCommand::Power(None) => vec![SummaryPart::with_range("Get Power", SegmentType::Command, cmd_range)],
                    // Other Kenwood commands that might come through
                    _ => vec![SummaryPart::with_range("Kenwood", SegmentType::Command, cmd_range)],
                }
            }
            FlexCommand::Mode(Some(m)) => {
                let mode_name = format_flex_mode(*m);
                let mode_range = if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "mode",
                        value: mode_name.to_string(),
                        segment_type: SegmentType::Mode,
                    });
                    Some(params_start..params_end)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("Mode", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = mode_range {
                        SummaryPart::with_range(mode_name, SegmentType::Mode, r)
                    } else {
                        SummaryPart::typed(mode_name, SegmentType::Mode)
                    },
                ]
            }
            FlexCommand::Mode(None) => vec![SummaryPart::with_range("Get Mode", SegmentType::Command, cmd_range)],
            FlexCommand::Info(Some(info)) => {
                // Frequency at params_start for 11 digits
                let freq_range = if params_start + 11 <= params_end {
                    segments.push(FrameSegment {
                        range: params_start..(params_start + 11),
                        label: "freq",
                        value: format_frequency(info.frequency_hz),
                        segment_type: SegmentType::Frequency,
                    });
                    Some(params_start..(params_start + 11))
                } else {
                    None
                };
                if params_start + 11 < params_end {
                    segments.push(FrameSegment {
                        range: (params_start + 11)..params_end,
                        label: "status",
                        value: "flags".to_string(),
                        segment_type: SegmentType::Status,
                    });
                }
                vec![
                    SummaryPart::with_range("Status", SegmentType::Command, cmd_range),
                    SummaryPart::plain(": "),
                    if let Some(r) = freq_range {
                        SummaryPart::with_range(format_frequency(info.frequency_hz), SegmentType::Frequency, r)
                    } else {
                        SummaryPart::typed(format_frequency(info.frequency_hz), SegmentType::Frequency)
                    },
                ]
            }
            FlexCommand::Info(None) => vec![SummaryPart::with_range("Get Status", SegmentType::Command, cmd_range)],
            FlexCommand::AudioGain(Some(g)) => vec![
                SummaryPart::with_range("Audio Gain", SegmentType::Command, cmd_range),
                SummaryPart::plain(" "),
                SummaryPart::typed(format!("{}", g), SegmentType::Data),
            ],
            FlexCommand::AudioGain(None) => vec![SummaryPart::with_range("Get Audio Gain", SegmentType::Command, cmd_range)],
            FlexCommand::RfPower(Some(p)) => vec![
                SummaryPart::with_range("RF Power", SegmentType::Command, cmd_range),
                SummaryPart::plain(" "),
                SummaryPart::typed(format!("{}", p), SegmentType::Data),
            ],
            FlexCommand::RfPower(None) => vec![SummaryPart::with_range("Get RF Power", SegmentType::Command, cmd_range)],
            FlexCommand::SMeter(Some(v)) => vec![
                SummaryPart::with_range("S-Meter", SegmentType::Command, cmd_range),
                SummaryPart::plain(" "),
                SummaryPart::typed(format!("{}", v), SegmentType::Data),
            ],
            FlexCommand::SMeter(None) => vec![SummaryPart::with_range("Read S-Meter", SegmentType::Command, cmd_range)],
            FlexCommand::AgcMode(Some(m)) => vec![
                SummaryPart::with_range("AGC", SegmentType::Command, cmd_range),
                SummaryPart::plain(" "),
                SummaryPart::typed(format!("{}", m), SegmentType::Data),
            ],
            FlexCommand::AgcMode(None) => vec![SummaryPart::with_range("Get AGC", SegmentType::Command, cmd_range)],
            FlexCommand::NoiseReduction(Some(on)) => {
                let state = if *on { "ON" } else { "OFF" };
                vec![
                    SummaryPart::with_range("NR", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    SummaryPart::typed(state, SegmentType::Status),
                ]
            }
            FlexCommand::NoiseReduction(None) => vec![SummaryPart::with_range("Get NR", SegmentType::Command, cmd_range.clone())],
            FlexCommand::AutoInfo(Some(enabled)) => {
                let state = if *enabled { "ON" } else { "OFF" };
                let ai_range = if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "state",
                        value: state.to_string(),
                        segment_type: SegmentType::Status,
                    });
                    Some(params_start..params_end)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("Auto Info", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = ai_range {
                        SummaryPart::with_range(state, SegmentType::Status, r)
                    } else {
                        SummaryPart::typed(state, SegmentType::Status)
                    },
                ]
            }
            FlexCommand::AutoInfo(None) => vec![SummaryPart::with_range("Get Auto Info", SegmentType::Command, cmd_range)],
            FlexCommand::Unknown(s) => {
                if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "params",
                        value: s.get(cmd_len..).unwrap_or("").to_string(),
                        segment_type: SegmentType::Data,
                    });
                }
                vec![SummaryPart::with_range(s, SegmentType::Command, cmd_range)]
            }
        };

        // Terminator if present
        if has_terminator {
            segments.push(FrameSegment {
                range: (data_len - 1)..data_len,
                label: "end",
                value: ";".to_string(),
                segment_type: SegmentType::Terminator,
            });
        }

        AnnotatedFrame {
            protocol: "Flex",
            summary,
            segments,
        }
    }
}

/// Format FlexMode as a human-readable string
fn format_flex_mode(mode: FlexMode) -> &'static str {
    match mode {
        FlexMode::Lsb => "LSB",
        FlexMode::Usb => "USB",
        FlexMode::Dsb => "DSB",
        FlexMode::CwL => "CW-L",
        FlexMode::CwU => "CW-U",
        FlexMode::Fm => "FM",
        FlexMode::Am => "AM",
        FlexMode::DigU => "DIG-U",
        FlexMode::Spec => "SPEC",
        FlexMode::DigL => "DIG-L",
        FlexMode::Sam => "SAM",
        FlexMode::Nfm => "NFM",
        FlexMode::Dfm => "DFM",
        FlexMode::Fdv => "FreeDV",
        FlexMode::Rtty => "RTTY",
        FlexMode::Dstar => "D-STAR",
    }
}

// ============================================================================
// FrameAnnotation for YaesuAsciiCommand
// ============================================================================

impl FrameAnnotation for YaesuAsciiCommand {
    fn annotate(&self, raw_bytes: &[u8]) -> AnnotatedFrame {
        let data_len = raw_bytes.len();
        let has_terminator = raw_bytes.last() == Some(&b';');
        let mut segments = Vec::new();

        // Parse command prefix from raw bytes
        let cmd_str = std::str::from_utf8(raw_bytes).unwrap_or("");
        let prefix = if cmd_str.len() >= 2 {
            &cmd_str[..2]
        } else {
            ""
        };

        // Command prefix (first 2 bytes)
        segments.push(FrameSegment {
            range: 0..2,
            label: "cmd",
            value: prefix.to_string(),
            segment_type: SegmentType::Command,
        });

        let params_start = 2;
        let params_end = if has_terminator {
            data_len - 1
        } else {
            data_len
        };
        let cmd_range = 0..2;

        let summary = match self {
            YaesuAsciiCommand::FrequencyA(Some(hz)) | YaesuAsciiCommand::FrequencyB(Some(hz)) => {
                let vfo = if matches!(self, YaesuAsciiCommand::FrequencyA(_)) {
                    "A"
                } else {
                    "B"
                };
                let freq_range = if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "freq",
                        value: format_frequency(*hz),
                        segment_type: SegmentType::Frequency,
                    });
                    Some(params_start..params_end)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range(format!("VFO {}", vfo), SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = freq_range {
                        SummaryPart::with_range(format_frequency(*hz), SegmentType::Frequency, r)
                    } else {
                        SummaryPart::typed(format_frequency(*hz), SegmentType::Frequency)
                    },
                ]
            }
            YaesuAsciiCommand::FrequencyA(None) | YaesuAsciiCommand::FrequencyB(None) => {
                let vfo = if matches!(self, YaesuAsciiCommand::FrequencyA(_)) {
                    "A"
                } else {
                    "B"
                };
                vec![SummaryPart::with_range(
                    format!("Get Freq VFO {}", vfo),
                    SegmentType::Command,
                    cmd_range,
                )]
            }
            YaesuAsciiCommand::Mode {
                receiver,
                mode: Some(m),
            } => {
                // Yaesu ASCII mode: MD + receiver(1) + mode(1)
                let mode_range = if params_start + 1 < params_end {
                    segments.push(FrameSegment {
                        range: params_start..(params_start + 1),
                        label: "rx",
                        value: format!("RX{}", receiver),
                        segment_type: SegmentType::Data,
                    });
                    segments.push(FrameSegment {
                        range: (params_start + 1)..params_end,
                        label: "mode",
                        value: format_yaesu_ascii_mode(*m).to_string(),
                        segment_type: SegmentType::Mode,
                    });
                    Some((params_start + 1)..params_end)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("Mode", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = mode_range {
                        SummaryPart::with_range(format_yaesu_ascii_mode(*m), SegmentType::Mode, r)
                    } else {
                        SummaryPart::typed(format_yaesu_ascii_mode(*m), SegmentType::Mode)
                    },
                ]
            }
            YaesuAsciiCommand::Mode { mode: None, .. } => {
                vec![SummaryPart::with_range(
                    "Get Mode",
                    SegmentType::Command,
                    cmd_range,
                )]
            }
            YaesuAsciiCommand::Transmit(Some(tx)) => {
                let state = match tx {
                    0 => "OFF",
                    1 => "ON",
                    2 => "TUNE",
                    _ => "?",
                };
                let tx_range = if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "state",
                        value: state.to_string(),
                        segment_type: SegmentType::Status,
                    });
                    Some(params_start..params_end)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("PTT", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = tx_range {
                        SummaryPart::with_range(state, SegmentType::Status, r)
                    } else {
                        SummaryPart::typed(state, SegmentType::Status)
                    },
                ]
            }
            YaesuAsciiCommand::Transmit(None) => {
                vec![SummaryPart::with_range(
                    "Get PTT",
                    SegmentType::Command,
                    cmd_range,
                )]
            }
            YaesuAsciiCommand::Id(Some(id)) => {
                let id_range = if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "id",
                        value: id.clone(),
                        segment_type: SegmentType::Data,
                    });
                    Some(params_start..params_end)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("ID", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = id_range {
                        SummaryPart::with_range(id, SegmentType::Data, r)
                    } else {
                        SummaryPart::typed(id, SegmentType::Data)
                    },
                ]
            }
            YaesuAsciiCommand::Id(None) => {
                vec![SummaryPart::with_range(
                    "Get ID",
                    SegmentType::Command,
                    cmd_range,
                )]
            }
            YaesuAsciiCommand::Info(Some(info)) => {
                // Frequency portion in IF response (9 digits after 3-digit memory channel)
                let freq_range = if params_start + 12 <= params_end {
                    segments.push(FrameSegment {
                        range: (params_start + 3)..(params_start + 12),
                        label: "freq",
                        value: format_frequency(info.frequency_hz),
                        segment_type: SegmentType::Frequency,
                    });
                    Some((params_start + 3)..(params_start + 12))
                } else {
                    None
                };
                if params_start + 12 < params_end {
                    segments.push(FrameSegment {
                        range: (params_start + 12)..params_end,
                        label: "status",
                        value: "flags".to_string(),
                        segment_type: SegmentType::Status,
                    });
                }
                vec![
                    SummaryPart::with_range("Status", SegmentType::Command, cmd_range),
                    SummaryPart::plain(": "),
                    if let Some(r) = freq_range {
                        SummaryPart::with_range(
                            format_frequency(info.frequency_hz),
                            SegmentType::Frequency,
                            r,
                        )
                    } else {
                        SummaryPart::typed(
                            format_frequency(info.frequency_hz),
                            SegmentType::Frequency,
                        )
                    },
                ]
            }
            YaesuAsciiCommand::Info(None) => {
                vec![SummaryPart::with_range(
                    "Get Status",
                    SegmentType::Command,
                    cmd_range,
                )]
            }
            YaesuAsciiCommand::VfoSelect(Some(v)) => {
                let vfo = if *v == 0 { "A" } else { "B" };
                let vfo_range = if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "vfo",
                        value: vfo.to_string(),
                        segment_type: SegmentType::Data,
                    });
                    Some(params_start..params_end)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("VFO", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = vfo_range {
                        SummaryPart::with_range(vfo, SegmentType::Data, r)
                    } else {
                        SummaryPart::typed(vfo, SegmentType::Data)
                    },
                ]
            }
            YaesuAsciiCommand::VfoSelect(None) => {
                vec![SummaryPart::with_range(
                    "Get VFO",
                    SegmentType::Command,
                    cmd_range,
                )]
            }
            YaesuAsciiCommand::Split(Some(s)) => {
                let state = if *s { "ON" } else { "OFF" };
                let split_range = if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "split",
                        value: state.to_string(),
                        segment_type: SegmentType::Status,
                    });
                    Some(params_start..params_end)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("Split", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = split_range {
                        SummaryPart::with_range(state, SegmentType::Status, r)
                    } else {
                        SummaryPart::typed(state, SegmentType::Status)
                    },
                ]
            }
            YaesuAsciiCommand::Split(None) => {
                vec![SummaryPart::with_range(
                    "Get Split",
                    SegmentType::Command,
                    cmd_range,
                )]
            }
            YaesuAsciiCommand::Power(Some(on)) => {
                let state = if *on { "ON" } else { "OFF" };
                let power_range = if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "power",
                        value: state.to_string(),
                        segment_type: SegmentType::Status,
                    });
                    Some(params_start..params_end)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("Power", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = power_range {
                        SummaryPart::with_range(state, SegmentType::Status, r)
                    } else {
                        SummaryPart::typed(state, SegmentType::Status)
                    },
                ]
            }
            YaesuAsciiCommand::Power(None) => {
                vec![SummaryPart::with_range(
                    "Get Power",
                    SegmentType::Command,
                    cmd_range,
                )]
            }
            YaesuAsciiCommand::AutoInfo(Some(enabled)) => {
                let state = if *enabled { "ON" } else { "OFF" };
                let ai_range = if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "state",
                        value: state.to_string(),
                        segment_type: SegmentType::Status,
                    });
                    Some(params_start..params_end)
                } else {
                    None
                };
                vec![
                    SummaryPart::with_range("Auto Info", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    if let Some(r) = ai_range {
                        SummaryPart::with_range(state, SegmentType::Status, r)
                    } else {
                        SummaryPart::typed(state, SegmentType::Status)
                    },
                ]
            }
            YaesuAsciiCommand::AutoInfo(None) => {
                vec![SummaryPart::with_range(
                    "Get Auto Info",
                    SegmentType::Command,
                    cmd_range,
                )]
            }
            YaesuAsciiCommand::SMeter(Some(v)) => {
                vec![
                    SummaryPart::with_range("S-Meter", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    SummaryPart::typed(format!("{}", v), SegmentType::Data),
                ]
            }
            YaesuAsciiCommand::SMeter(None) => {
                vec![SummaryPart::with_range(
                    "Read S-Meter",
                    SegmentType::Command,
                    cmd_range,
                )]
            }
            YaesuAsciiCommand::RfPower(Some(p)) => {
                vec![
                    SummaryPart::with_range("RF Power", SegmentType::Command, cmd_range),
                    SummaryPart::plain(" "),
                    SummaryPart::typed(format!("{}%", p), SegmentType::Data),
                ]
            }
            YaesuAsciiCommand::RfPower(None) => {
                vec![SummaryPart::with_range(
                    "Get RF Power",
                    SegmentType::Command,
                    cmd_range,
                )]
            }
            YaesuAsciiCommand::Unknown(s) => {
                if params_start < params_end {
                    segments.push(FrameSegment {
                        range: params_start..params_end,
                        label: "params",
                        value: s.get(2..).unwrap_or("").to_string(),
                        segment_type: SegmentType::Data,
                    });
                }
                vec![SummaryPart::with_range(s, SegmentType::Command, cmd_range)]
            }
        };

        // Terminator if present
        if has_terminator {
            segments.push(FrameSegment {
                range: (data_len - 1)..data_len,
                label: "end",
                value: ";".to_string(),
                segment_type: SegmentType::Terminator,
            });
        }

        AnnotatedFrame {
            protocol: "Yaesu ASCII",
            summary,
            segments,
        }
    }
}

// ============================================================================
// Top-level decode function
// ============================================================================

/// Try to decode raw data and return an annotated frame
///
/// This function auto-detects the protocol and returns an annotated frame
/// suitable for display. Returns `None` if the data cannot be decoded.
pub fn decode_and_annotate(data: &[u8]) -> Option<AnnotatedFrame> {
    decode_and_annotate_with_hint(data, None)
}

/// Try to decode raw data with an optional protocol hint
///
/// When a protocol hint is provided, it is used to select the correct decoder
/// instead of auto-detection. This is useful when the source radio's protocol
/// is known (e.g., from a configured virtual or COM radio).
pub fn decode_and_annotate_with_hint(
    data: &[u8],
    protocol_hint: Option<crate::Protocol>,
) -> Option<AnnotatedFrame> {
    use crate::Protocol;

    // If we have a protocol hint, use it directly
    if let Some(protocol) = protocol_hint {
        return match protocol {
            Protocol::IcomCIV => try_decode_civ(data),
            Protocol::Yaesu => try_decode_yaesu(data),
            Protocol::YaesuAscii => try_decode_yaesu_ascii(data),
            Protocol::Kenwood => try_decode_kenwood_only(data),
            Protocol::Elecraft => try_decode_elecraft(data),
            Protocol::FlexRadio => try_decode_flex(data),
        };
    }

    // Auto-detect: Try CI-V frame first (most specific detection)
    if data.len() >= 6 && data[0] == PREAMBLE && data[1] == PREAMBLE {
        return try_decode_civ(data);
    }

    // Try ASCII (Kenwood/Elecraft/FlexRadio)
    if let Some(frame) = try_decode_kenwood(data) {
        return Some(frame);
    }

    // Try Yaesu 5-byte command
    if data.len() == 5 {
        return try_decode_yaesu(data);
    }

    None
}

/// Try to decode CI-V frame
fn try_decode_civ(data: &[u8]) -> Option<AnnotatedFrame> {
    if data.len() < 6 || data[0] != PREAMBLE || data[1] != PREAMBLE {
        return None;
    }
    if data[data.len() - 1] != TERMINATOR {
        return None;
    }

    let mut codec = CivCodec::new();
    codec.push_bytes(data);

    if let Some(cmd) = codec.next_command() {
        Some(cmd.annotate(data))
    } else {
        // Fallback: create minimal annotation if codec fails
        let mut segments = vec![
            FrameSegment {
                range: 0..2,
                label: "preamble",
                value: "FE FE".to_string(),
                segment_type: SegmentType::Preamble,
            },
            FrameSegment {
                range: 2..3,
                label: "to",
                value: format!("{:02X}", data[2]),
                segment_type: SegmentType::Address,
            },
            FrameSegment {
                range: 3..4,
                label: "from",
                value: format!("{:02X}", data[3]),
                segment_type: SegmentType::Address,
            },
            FrameSegment {
                range: 4..5,
                label: "cmd",
                value: format!("{:02X}", data[4]),
                segment_type: SegmentType::Command,
            },
            FrameSegment {
                range: (data.len() - 1)..data.len(),
                label: "end",
                value: "FD".to_string(),
                segment_type: SegmentType::Terminator,
            },
        ];

        if data.len() > 6 {
            segments.insert(
                4,
                FrameSegment {
                    range: 5..(data.len() - 1),
                    label: "data",
                    value: format!("{} bytes", data.len() - 6),
                    segment_type: SegmentType::Data,
                },
            );
        }

        Some(AnnotatedFrame {
            protocol: "CI-V",
            summary: vec![SummaryPart::plain(format!("cmd={:02X}", data[4]))],
            segments,
        })
    }
}

/// Try to decode Yaesu frame
fn try_decode_yaesu(data: &[u8]) -> Option<AnnotatedFrame> {
    if data.len() != 5 {
        return None;
    }

    // Parse using codec to get proper YaesuCommand
    let mut codec = crate::yaesu::YaesuCodec::new();
    codec.push_bytes(data);

    codec.next_command().map(|cmd| cmd.annotate(data))
}

/// Try to decode Kenwood/FlexRadio ASCII frame (auto-detects ZZ prefix for Flex)
fn try_decode_kenwood(data: &[u8]) -> Option<AnnotatedFrame> {
    let s = std::str::from_utf8(data).ok()?;
    if !s.chars().all(|c| c.is_ascii_graphic() || c == ';') {
        return None;
    }

    let cmd_str = s.trim_end_matches(';');
    if cmd_str.len() < 2 {
        return None;
    }

    // Use FlexCodec for ZZ-prefixed commands (it handles both ZZ and standard commands)
    if cmd_str.starts_with("ZZ") {
        let mut codec = FlexCodec::new();
        codec.push_bytes(data);
        if let Some(cmd) = codec.next_command() {
            return Some(cmd.annotate(data));
        }
    }

    // Parse using Kenwood codec for standard commands
    let mut codec = KenwoodCodec::new();
    codec.push_bytes(data);

    codec.next_command().map(|cmd| cmd.annotate(data))
}

/// Try to decode Kenwood ASCII frame only (no Flex detection)
fn try_decode_kenwood_only(data: &[u8]) -> Option<AnnotatedFrame> {
    let s = std::str::from_utf8(data).ok()?;
    if !s.chars().all(|c| c.is_ascii_graphic() || c == ';') {
        return None;
    }

    let cmd_str = s.trim_end_matches(';');
    if cmd_str.len() < 2 {
        return None;
    }

    let mut codec = KenwoodCodec::new();
    codec.push_bytes(data);

    codec.next_command().map(|cmd| cmd.annotate(data))
}

/// Try to decode Elecraft ASCII frame (Kenwood-compatible)
fn try_decode_elecraft(data: &[u8]) -> Option<AnnotatedFrame> {
    // Elecraft uses Kenwood protocol, annotate as Elecraft
    let s = std::str::from_utf8(data).ok()?;
    if !s.chars().all(|c| c.is_ascii_graphic() || c == ';') {
        return None;
    }

    let cmd_str = s.trim_end_matches(';');
    if cmd_str.len() < 2 {
        return None;
    }

    let mut codec = KenwoodCodec::new();
    codec.push_bytes(data);

    codec.next_command().map(|cmd| {
        let mut frame = cmd.annotate(data);
        frame.protocol = "Elecraft";
        frame
    })
}

/// Try to decode FlexRadio ASCII frame
fn try_decode_flex(data: &[u8]) -> Option<AnnotatedFrame> {
    let s = std::str::from_utf8(data).ok()?;
    if !s.chars().all(|c| c.is_ascii_graphic() || c == ';') {
        return None;
    }

    let cmd_str = s.trim_end_matches(';');
    if cmd_str.len() < 2 {
        return None;
    }

    let mut codec = FlexCodec::new();
    codec.push_bytes(data);

    codec.next_command().map(|cmd| cmd.annotate(data))
}

/// Try to decode Yaesu ASCII frame
fn try_decode_yaesu_ascii(data: &[u8]) -> Option<AnnotatedFrame> {
    let s = std::str::from_utf8(data).ok()?;
    if !s.chars().all(|c| c.is_ascii_graphic() || c == ';') {
        return None;
    }

    let cmd_str = s.trim_end_matches(';');
    if cmd_str.len() < 2 {
        return None;
    }

    let mut codec = YaesuAsciiCodec::new();
    codec.push_bytes(data);

    codec.next_command().map(|cmd| cmd.annotate(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_frequency() {
        assert_eq!(format_frequency(14_250_000), "14.250 MHz");
        assert_eq!(format_frequency(7_074_000), "7.074 MHz");
        assert_eq!(format_frequency(14_074_500), "14.074500 MHz");
    }

    #[test]
    fn test_decode_civ_frequency() {
        // CI-V frequency report: FE FE E0 94 03 00 00 25 14 00 FD
        let data = [
            0xFE, 0xFE, 0xE0, 0x94, 0x03, 0x00, 0x00, 0x25, 0x14, 0x00, 0xFD,
        ];
        let frame = decode_and_annotate(&data).unwrap();
        assert_eq!(frame.protocol, "CI-V");
        assert!(frame.summary.iter().any(|p| p.text.contains("14.250")));
        // Verify frequency segment exists for highlighting
        let freq_seg = frame
            .segments
            .iter()
            .find(|s| s.segment_type == SegmentType::Frequency);
        assert!(freq_seg.is_some());
        let freq_seg = freq_seg.unwrap();
        assert_eq!(freq_seg.range, 5..10); // After preamble, addr, from, cmd
        assert!(freq_seg.value.contains("14.250"));
    }

    #[test]
    fn test_decode_kenwood_frequency() {
        let data = b"FA00014250000;";
        let frame = decode_and_annotate(data).unwrap();
        assert_eq!(frame.protocol, "Kenwood");
        assert!(frame.summary.iter().any(|p| p.text.contains("14.250")));
    }

    #[test]
    fn test_decode_yaesu() {
        // Yaesu PTT ON command
        let data = [0x00, 0x00, 0x00, 0x00, 0x08];
        let frame = decode_and_annotate(&data).unwrap();
        assert_eq!(frame.protocol, "Yaesu");
        assert!(frame.summary.iter().any(|p| p.text.contains("PTT")));
    }

    #[test]
    fn test_flex_detection() {
        let data = b"ZZFA00014250000;";
        let frame = decode_and_annotate(data).unwrap();
        assert_eq!(frame.protocol, "Flex");
        assert!(frame.summary.iter().any(|p| p.text.contains("14.250")));
        // Verify segments are created for highlighting
        assert!(frame.segments.len() >= 2); // At least cmd and freq
        assert!(frame
            .segments
            .iter()
            .any(|s| s.segment_type == SegmentType::Frequency));
    }

    #[test]
    fn test_kenwood_frequency_segments() {
        let data = b"FA00014250000;";
        let frame = decode_and_annotate(data).unwrap();
        assert_eq!(frame.protocol, "Kenwood");
        // Verify frequency segment exists for highlighting
        let freq_seg = frame
            .segments
            .iter()
            .find(|s| s.segment_type == SegmentType::Frequency);
        assert!(freq_seg.is_some());
        let freq_seg = freq_seg.unwrap();
        assert_eq!(freq_seg.range, 2..13); // FA + 11 digits
        assert!(freq_seg.value.contains("14.250"));
    }

    #[test]
    fn test_kenwood_vfo_segments() {
        let data = b"FR0;";
        let frame = decode_and_annotate(data).unwrap();
        assert_eq!(frame.protocol, "Kenwood");
        // VFO select should have a data segment
        let vfo_seg = frame.segments.iter().find(|s| s.label == "vfo");
        assert!(vfo_seg.is_some());
    }

    #[test]
    fn test_protocol_hint_elecraft() {
        use crate::Protocol;

        // Without hint, this decodes as Kenwood
        let data = b"FA00014250000;";
        let frame = decode_and_annotate(data).unwrap();
        assert_eq!(frame.protocol, "Kenwood");

        // With Elecraft hint, should decode as Elecraft
        let frame = decode_and_annotate_with_hint(data, Some(Protocol::Elecraft)).unwrap();
        assert_eq!(frame.protocol, "Elecraft");
        assert!(frame.summary.iter().any(|p| p.text.contains("14.250")));
    }

    #[test]
    fn test_protocol_hint_flex() {
        use crate::Protocol;

        // Standard Kenwood command decoded with FlexRadio hint
        let data = b"FA00014250000;";
        let frame = decode_and_annotate_with_hint(data, Some(Protocol::FlexRadio)).unwrap();
        assert_eq!(frame.protocol, "Flex");
    }

    #[test]
    fn test_protocol_hint_yaesu_ascii() {
        use crate::Protocol;

        // ASCII command with YaesuAscii hint should show Yaesu ASCII
        let data = b"FA00014250000;";
        let frame = decode_and_annotate_with_hint(data, Some(Protocol::YaesuAscii)).unwrap();
        assert_eq!(frame.protocol, "Yaesu ASCII");
    }

    #[test]
    fn test_protocol_hint_civ() {
        use crate::Protocol;

        // CI-V frequency report
        let data = [
            0xFE, 0xFE, 0xE0, 0x94, 0x03, 0x00, 0x00, 0x25, 0x14, 0x00, 0xFD,
        ];
        let frame = decode_and_annotate_with_hint(&data, Some(Protocol::IcomCIV)).unwrap();
        assert_eq!(frame.protocol, "CI-V");
    }
}
