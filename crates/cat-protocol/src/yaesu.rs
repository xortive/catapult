//! Yaesu CAT Protocol Implementation
//!
//! The Yaesu CAT protocol uses 5-byte binary commands. The format varies
//! slightly between radio models, but the core structure is consistent.
//!
//! # Command Format (FT-817/857/897 style)
//! ```text
//! [P1] [P2] [P3] [P4] [CMD]
//! ```
//!
//! - Bytes 0-3: Parameters (meaning depends on command)
//! - Byte 4: Command opcode
//!
//! # Frequency Encoding
//! Frequencies are BCD encoded in bytes 0-3 (big-endian).
//! Example: 14.250.00 MHz = 0x14 0x25 0x00 0x00
//!
//! Note: Different Yaesu models have different resolutions:
//! - FT-817/857/897: 10 Hz resolution (4 BCD bytes = 8 digits)
//! - FT-991/FTDX: 1 Hz resolution (extended commands)

use crate::command::{OperatingMode, RadioCommand, Vfo};
use crate::error::ParseError;
use crate::{EncodeCommand, FromRadioCommand, ProtocolCodec, ToRadioCommand};

/// Standard Yaesu command length
pub const COMMAND_LEN: usize = 5;

/// Yaesu command opcodes (FT-817/857/897 compatible)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YaesuOpcode {
    /// Lock on
    LockOn = 0x00,
    /// Lock off
    LockOff = 0x80,
    /// Set frequency (P1-P4 = BCD freq)
    SetFrequency = 0x01,
    /// Set mode (P1 = mode)
    SetMode = 0x07,
    /// Toggle VFO
    ToggleVfo = 0x81,
    /// Split on
    SplitOn = 0x02,
    /// Split off
    SplitOff = 0x82,
    /// Clarifier on
    ClarOn = 0x05,
    /// Clarifier off
    ClarOff = 0x85,
    /// Clarifier frequency
    ClarFreq = 0xF5,
    /// PTT on (TX)
    PttOn = 0x08,
    /// PTT off (RX)
    PttOff = 0x88,
    /// Read RX status
    ReadRxStatus = 0xE7,
    /// Read TX status
    ReadTxStatus = 0xF7,
    /// Read frequency and mode
    ReadFreqMode = 0x03,
    /// Power on
    PowerOn = 0x0F,
    /// Power off
    PowerOff = 0x8F,
}

impl TryFrom<u8> for YaesuOpcode {
    type Error = ParseError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::LockOn),
            0x80 => Ok(Self::LockOff),
            0x01 => Ok(Self::SetFrequency),
            0x07 => Ok(Self::SetMode),
            0x81 => Ok(Self::ToggleVfo),
            0x02 => Ok(Self::SplitOn),
            0x82 => Ok(Self::SplitOff),
            0x05 => Ok(Self::ClarOn),
            0x85 => Ok(Self::ClarOff),
            0xF5 => Ok(Self::ClarFreq),
            0x08 => Ok(Self::PttOn),
            0x88 => Ok(Self::PttOff),
            0xE7 => Ok(Self::ReadRxStatus),
            0xF7 => Ok(Self::ReadTxStatus),
            0x03 => Ok(Self::ReadFreqMode),
            0x0F => Ok(Self::PowerOn),
            0x8F => Ok(Self::PowerOff),
            _ => Err(ParseError::UnknownCommand(format!(
                "Yaesu opcode 0x{:02X}",
                value
            ))),
        }
    }
}

/// Yaesu protocol command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YaesuCommand {
    /// Set frequency (in 10 Hz units for classic Yaesu)
    SetFrequency { hz: u64 },
    /// Read frequency/mode response
    FrequencyModeReport { hz: u64, mode: u8 },
    /// Read frequency/mode query
    GetFrequencyMode,
    /// Set operating mode
    SetMode { mode: u8 },
    /// PTT on
    PttOn,
    /// PTT off
    PttOff,
    /// Toggle VFO A/B
    ToggleVfo,
    /// Split on
    SplitOn,
    /// Split off
    SplitOff,
    /// Read RX status
    ReadRxStatus,
    /// RX status response
    RxStatusReport { status: u8 },
    /// Read TX status
    ReadTxStatus,
    /// TX status response
    TxStatusReport { status: u8 },
    /// Power on
    PowerOn,
    /// Power off
    PowerOff,
    /// Lock on
    LockOn,
    /// Lock off
    LockOff,
    /// Unknown command
    Unknown { bytes: [u8; 5] },
}

/// Yaesu RX status byte flags
pub mod rx_status {
    /// Squelch open (signal present)
    pub const SQUELCH_OPEN: u8 = 0x80;
    /// CTCSS/DCS match
    pub const TONE_MATCH: u8 = 0x40;
    /// Discriminator centered
    pub const DISC_CENTER: u8 = 0x20;
    /// S-meter reading (bits 0-3)
    pub const S_METER_MASK: u8 = 0x0F;
}

/// Yaesu TX status byte flags
pub mod tx_status {
    /// Power output / SWR meter (bits 0-3)
    pub const METER_MASK: u8 = 0x0F;
    /// High SWR
    pub const HIGH_SWR: u8 = 0x40;
    /// Split active
    pub const SPLIT: u8 = 0x20;
}

/// Streaming Yaesu protocol codec
pub struct YaesuCodec {
    buffer: Vec<u8>,
    /// Expected response length (for handling variable responses)
    expected_response_len: Option<usize>,
}

impl YaesuCodec {
    /// Create a new Yaesu codec
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(16),
            expected_response_len: None,
        }
    }

    /// Set expected response length for next read
    pub fn expect_response(&mut self, len: usize) {
        self.expected_response_len = Some(len);
    }

    /// Parse a 5-byte command
    fn parse_command(bytes: &[u8; 5]) -> YaesuCommand {
        let opcode = bytes[4];

        match opcode {
            0x01 => {
                // Set frequency
                let hz = bcd_to_frequency_be(bytes);
                YaesuCommand::SetFrequency { hz }
            }
            0x03 => {
                // This is the query command
                YaesuCommand::GetFrequencyMode
            }
            0x07 => {
                // Set mode
                YaesuCommand::SetMode { mode: bytes[0] }
            }
            0x08 => YaesuCommand::PttOn,
            0x88 => YaesuCommand::PttOff,
            0x81 => YaesuCommand::ToggleVfo,
            0x02 => YaesuCommand::SplitOn,
            0x82 => YaesuCommand::SplitOff,
            0xE7 => YaesuCommand::ReadRxStatus,
            0xF7 => YaesuCommand::ReadTxStatus,
            0x0F => YaesuCommand::PowerOn,
            0x8F => YaesuCommand::PowerOff,
            0x00 => YaesuCommand::LockOn,
            0x80 => YaesuCommand::LockOff,
            _ => YaesuCommand::Unknown { bytes: *bytes },
        }
    }

    /// Parse a frequency/mode response (5 bytes)
    fn parse_freq_mode_response(bytes: &[u8]) -> YaesuCommand {
        if bytes.len() >= 5 {
            let hz = bcd_to_frequency_be(&[bytes[0], bytes[1], bytes[2], bytes[3], 0]);
            let mode = bytes[4];
            YaesuCommand::FrequencyModeReport { hz, mode }
        } else if bytes.len() == 1 {
            // Single byte status response
            YaesuCommand::RxStatusReport { status: bytes[0] }
        } else {
            YaesuCommand::Unknown {
                bytes: [
                    bytes.first().copied().unwrap_or(0),
                    bytes.get(1).copied().unwrap_or(0),
                    bytes.get(2).copied().unwrap_or(0),
                    bytes.get(3).copied().unwrap_or(0),
                    bytes.get(4).copied().unwrap_or(0),
                ],
            }
        }
    }
}

impl Default for YaesuCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl ProtocolCodec for YaesuCodec {
    type Command = YaesuCommand;

    fn push_bytes(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    fn next_command(&mut self) -> Option<Self::Command> {
        let len = self.expected_response_len.unwrap_or(COMMAND_LEN);

        if self.buffer.len() < len {
            return None;
        }

        // Extract bytes
        let bytes: Vec<u8> = self.buffer.drain(..len).collect();
        self.expected_response_len = None;

        if len == 5 {
            let arr: [u8; 5] = bytes.try_into().ok()?;
            Some(Self::parse_command(&arr))
        } else {
            Some(Self::parse_freq_mode_response(&bytes))
        }
    }

    fn clear(&mut self) {
        self.buffer.clear();
        self.expected_response_len = None;
    }
}

impl ToRadioCommand for YaesuCommand {
    fn to_radio_command(&self) -> RadioCommand {
        match self {
            YaesuCommand::SetFrequency { hz } => RadioCommand::SetFrequency { hz: *hz },
            YaesuCommand::GetFrequencyMode => RadioCommand::GetFrequency,
            YaesuCommand::FrequencyModeReport { hz, mode } => RadioCommand::StatusReport {
                frequency_hz: Some(*hz),
                mode: Some(yaesu_mode_to_operating_mode(*mode)),
                ptt: None,
                vfo: None,
            },
            YaesuCommand::SetMode { mode } => RadioCommand::SetMode {
                mode: yaesu_mode_to_operating_mode(*mode),
            },
            YaesuCommand::PttOn => RadioCommand::SetPtt { active: true },
            YaesuCommand::PttOff => RadioCommand::SetPtt { active: false },
            YaesuCommand::ToggleVfo => RadioCommand::SetVfo { vfo: Vfo::B }, // Approximate
            YaesuCommand::SplitOn => RadioCommand::SetVfo { vfo: Vfo::Split },
            YaesuCommand::SplitOff => RadioCommand::SetVfo { vfo: Vfo::A },
            YaesuCommand::ReadRxStatus => RadioCommand::GetStatus,
            YaesuCommand::RxStatusReport { status } => {
                let ptt = (*status & rx_status::SQUELCH_OPEN) != 0;
                RadioCommand::StatusReport {
                    frequency_hz: None,
                    mode: None,
                    ptt: Some(ptt),
                    vfo: None,
                }
            }
            YaesuCommand::ReadTxStatus => RadioCommand::GetPtt,
            YaesuCommand::TxStatusReport { status } => {
                // If we're transmitting, the meter will show power
                let ptt = (*status & tx_status::METER_MASK) > 0;
                RadioCommand::PttReport { active: ptt }
            }
            YaesuCommand::PowerOn => RadioCommand::SetPower { on: true },
            YaesuCommand::PowerOff => RadioCommand::SetPower { on: false },
            YaesuCommand::LockOn | YaesuCommand::LockOff => RadioCommand::Unknown { data: vec![] },
            YaesuCommand::Unknown { bytes } => RadioCommand::Unknown {
                data: bytes.to_vec(),
            },
        }
    }
}

impl FromRadioCommand for YaesuCommand {
    fn from_radio_command(cmd: &RadioCommand) -> Option<Self> {
        match cmd {
            RadioCommand::SetFrequency { hz } => Some(YaesuCommand::SetFrequency { hz: *hz }),
            RadioCommand::GetFrequency => Some(YaesuCommand::GetFrequencyMode),
            RadioCommand::FrequencyReport { hz } => {
                Some(YaesuCommand::FrequencyModeReport { hz: *hz, mode: 0 })
            }
            RadioCommand::SetMode { mode } => Some(YaesuCommand::SetMode {
                mode: operating_mode_to_yaesu(*mode),
            }),
            RadioCommand::ModeReport { mode } => Some(YaesuCommand::SetMode {
                mode: operating_mode_to_yaesu(*mode),
            }),
            RadioCommand::GetMode => Some(YaesuCommand::GetFrequencyMode),
            RadioCommand::SetPtt { active: true } => Some(YaesuCommand::PttOn),
            RadioCommand::SetPtt { active: false } => Some(YaesuCommand::PttOff),
            RadioCommand::PttReport { active: true } => Some(YaesuCommand::PttOn),
            RadioCommand::PttReport { active: false } => Some(YaesuCommand::PttOff),
            RadioCommand::GetPtt => Some(YaesuCommand::ReadTxStatus),
            RadioCommand::GetStatus => Some(YaesuCommand::ReadRxStatus),
            RadioCommand::SetVfo { vfo: Vfo::Split } => Some(YaesuCommand::SplitOn),
            RadioCommand::SetVfo { .. } => Some(YaesuCommand::ToggleVfo),
            RadioCommand::SetPower { on: true } => Some(YaesuCommand::PowerOn),
            RadioCommand::SetPower { on: false } => Some(YaesuCommand::PowerOff),
            _ => None,
        }
    }
}

impl EncodeCommand for YaesuCommand {
    fn encode(&self) -> Vec<u8> {
        match self {
            YaesuCommand::SetFrequency { hz } => {
                let mut bytes = frequency_to_bcd_be(*hz);
                bytes.push(0x01);
                bytes
            }
            YaesuCommand::GetFrequencyMode => vec![0x00, 0x00, 0x00, 0x00, 0x03],
            YaesuCommand::FrequencyModeReport { hz, mode } => {
                let mut bytes = frequency_to_bcd_be(*hz);
                bytes.push(*mode);
                bytes
            }
            YaesuCommand::SetMode { mode } => vec![*mode, 0x00, 0x00, 0x00, 0x07],
            YaesuCommand::PttOn => vec![0x00, 0x00, 0x00, 0x00, 0x08],
            YaesuCommand::PttOff => vec![0x00, 0x00, 0x00, 0x00, 0x88],
            YaesuCommand::ToggleVfo => vec![0x00, 0x00, 0x00, 0x00, 0x81],
            YaesuCommand::SplitOn => vec![0x00, 0x00, 0x00, 0x00, 0x02],
            YaesuCommand::SplitOff => vec![0x00, 0x00, 0x00, 0x00, 0x82],
            YaesuCommand::ReadRxStatus => vec![0x00, 0x00, 0x00, 0x00, 0xE7],
            YaesuCommand::RxStatusReport { status } => vec![*status],
            YaesuCommand::ReadTxStatus => vec![0x00, 0x00, 0x00, 0x00, 0xF7],
            YaesuCommand::TxStatusReport { status } => vec![*status],
            YaesuCommand::PowerOn => vec![0x00, 0x00, 0x00, 0x00, 0x0F],
            YaesuCommand::PowerOff => vec![0x00, 0x00, 0x00, 0x00, 0x8F],
            YaesuCommand::LockOn => vec![0x00, 0x00, 0x00, 0x00, 0x00],
            YaesuCommand::LockOff => vec![0x00, 0x00, 0x00, 0x00, 0x80],
            YaesuCommand::Unknown { bytes } => bytes.to_vec(),
        }
    }
}

/// Convert big-endian BCD bytes to frequency in Hz
/// Classic Yaesu format: 4 bytes = 8 BCD digits, 10 Hz resolution
/// Example: 14.250.00 MHz = 0x14 0x25 0x00 0x00
fn bcd_to_frequency_be(bytes: &[u8; 5]) -> u64 {
    let mut freq: u64 = 0;

    for &byte in bytes.iter().take(4) {
        let high = ((byte >> 4) & 0x0F) as u64;
        let low = (byte & 0x0F) as u64;
        freq = freq * 100 + high * 10 + low;
    }

    // Classic Yaesu has 10 Hz resolution, multiply by 10
    freq * 10
}

/// Convert frequency in Hz to big-endian BCD bytes (4 bytes)
fn frequency_to_bcd_be(hz: u64) -> Vec<u8> {
    // Divide by 10 for 10 Hz resolution
    let mut remaining = hz / 10;
    let mut result = vec![0u8; 4];

    for i in (0..4).rev() {
        let low = (remaining % 10) as u8;
        remaining /= 10;
        let high = (remaining % 10) as u8;
        remaining /= 10;
        result[i] = (high << 4) | low;
    }

    result
}

/// Convert Yaesu mode byte to OperatingMode
fn yaesu_mode_to_operating_mode(mode: u8) -> OperatingMode {
    match mode {
        0x00 => OperatingMode::Lsb,
        0x01 => OperatingMode::Usb,
        0x02 => OperatingMode::Cw,
        0x03 => OperatingMode::CwR,
        0x04 => OperatingMode::Am,
        0x06 => OperatingMode::Fm, // Wide FM
        0x08 => OperatingMode::Fm, // FM
        0x0A => OperatingMode::Dig,
        0x0C => OperatingMode::Pkt,
        _ => OperatingMode::Usb,
    }
}

/// Convert OperatingMode to Yaesu mode byte
fn operating_mode_to_yaesu(mode: OperatingMode) -> u8 {
    match mode {
        OperatingMode::Lsb => 0x00,
        OperatingMode::Usb => 0x01,
        OperatingMode::Cw => 0x02,
        OperatingMode::CwR => 0x03,
        OperatingMode::Am => 0x04,
        OperatingMode::Fm | OperatingMode::FmN => 0x08,
        OperatingMode::Dig | OperatingMode::DigU | OperatingMode::DigL => 0x0A,
        OperatingMode::Data | OperatingMode::DataU | OperatingMode::DataL => 0x0A,
        OperatingMode::Pkt => 0x0C,
        OperatingMode::Rtty | OperatingMode::RttyR => 0x0A,
    }
}

/// Generate a probe command to detect Yaesu radios
/// Uses the read frequency/mode command
pub fn probe_command() -> Vec<u8> {
    vec![0x00, 0x00, 0x00, 0x00, 0x03]
}

/// Expected response length for the probe command
pub fn probe_response_len() -> usize {
    5 // Frequency (4 bytes) + Mode (1 byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bcd_frequency_roundtrip() {
        let freqs = [14_250_000u64, 7_074_000, 28_500_000, 144_200_000, 3_573_000];

        for freq in freqs {
            let bcd = frequency_to_bcd_be(freq);
            assert_eq!(bcd.len(), 4);
            // Parse back (need to add a 5th byte for the decoder)
            let arr: [u8; 5] = [bcd[0], bcd[1], bcd[2], bcd[3], 0];
            let back = bcd_to_frequency_be(&arr);
            // Should match to 10 Hz resolution
            assert_eq!(
                back / 10 * 10,
                freq / 10 * 10,
                "Roundtrip failed for {}",
                freq
            );
        }
    }

    #[test]
    fn test_encode_set_frequency() {
        let cmd = YaesuCommand::SetFrequency { hz: 14_250_000 };
        let encoded = cmd.encode();
        assert_eq!(encoded.len(), 5);
        assert_eq!(encoded[4], 0x01); // SetFrequency opcode
    }

    #[test]
    fn test_parse_ptt_on() {
        let mut codec = YaesuCodec::new();
        codec.push_bytes(&[0x00, 0x00, 0x00, 0x00, 0x08]);

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, YaesuCommand::PttOn);
    }

    #[test]
    fn test_to_radio_command() {
        let cmd = YaesuCommand::SetFrequency { hz: 7_074_000 };
        let radio_cmd = cmd.to_radio_command();
        assert_eq!(radio_cmd, RadioCommand::SetFrequency { hz: 7_074_000 });
    }

    #[test]
    fn test_from_radio_command() {
        let radio_cmd = RadioCommand::SetPtt { active: true };
        let cmd = YaesuCommand::from_radio_command(&radio_cmd).unwrap();
        assert_eq!(cmd, YaesuCommand::PttOn);
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let commands = vec![
            YaesuCommand::PttOn,
            YaesuCommand::PttOff,
            YaesuCommand::ToggleVfo,
            YaesuCommand::SplitOn,
            YaesuCommand::ReadRxStatus,
        ];

        for cmd in commands {
            let encoded = cmd.encode();
            let mut codec = YaesuCodec::new();
            codec.push_bytes(&encoded);
            let decoded = codec.next_command().unwrap();
            assert_eq!(decoded, cmd);
        }
    }
}
