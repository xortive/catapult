//! Icom CI-V Protocol Implementation
//!
//! The CI-V (Communication Interface V) protocol is used by Icom transceivers.
//! It uses framed variable-length binary messages with address-based routing.
//!
//! # Frame Format
//! ```text
//! FE FE [to] [from] [cmd] [subcmd] [data...] FD
//! ```
//!
//! - `FE FE`: Preamble (two bytes)
//! - `to`: Destination address (radio address or 0xE0 for controller)
//! - `from`: Source address (controller address, typically 0xE0)
//! - `cmd`: Command code
//! - `subcmd`: Sub-command code (optional, depends on command)
//! - `data`: Variable length data (BCD encoded for frequencies)
//! - `FD`: Terminator
//!
//! # Frequency Encoding
//! Frequencies are encoded in BCD (Binary Coded Decimal), little-endian.
//! Example: 14.250.000 Hz = 00 00 25 41 00 (reversed: 00 14 25 00 00)

use crate::command::{OperatingMode, RadioRequest, RadioResponse, Vfo};
use crate::error::ParseError;
use crate::{
    EncodeCommand, FromRadioRequest, FromRadioResponse, ProtocolCodec, ToRadioRequest,
    ToRadioResponse,
};

/// CI-V frame preamble byte
pub const PREAMBLE: u8 = 0xFE;
/// CI-V frame terminator byte
pub const TERMINATOR: u8 = 0xFD;
/// Default controller address
pub const CONTROLLER_ADDR: u8 = 0xE0;
/// Broadcast address
pub const BROADCAST_ADDR: u8 = 0x00;

/// Maximum frame length (reasonable limit)
const MAX_FRAME_LEN: usize = 64;

/// CI-V command codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CivCommandCode {
    /// Set/read frequency (0x00/0x03)
    Frequency = 0x00,
    /// Set/read mode (0x01/0x04)
    Mode = 0x01,
    /// Read frequency (from radio)
    ReadFrequency = 0x03,
    /// Read mode (from radio)
    ReadMode = 0x04,
    /// Set frequency (to radio)
    SetFrequency = 0x05,
    /// Set mode (to radio)
    SetMode = 0x06,
    /// VFO select
    VfoSelect = 0x07,
    /// Scan control
    Scan = 0x0E,
    /// Split control
    Split = 0x0F,
    /// Tuning step
    TuningStep = 0x10,
    /// Attenuator
    Attenuator = 0x11,
    /// Antenna select
    Antenna = 0x12,
    /// Announce mode
    Announce = 0x13,
    /// Various level settings
    Level = 0x14,
    /// Read meter
    ReadMeter = 0x15,
    /// Preamp
    Preamp = 0x16,
    /// AGC setting
    Agc = 0x17,
    /// NB setting
    NoiseBlanker = 0x18,
    /// PTT control
    Ptt = 0x1C,
    /// Transceive mode
    Transceive = 0x1A,
    /// OK response from radio
    Ok = 0xFB,
    /// NG (error) response from radio
    Ng = 0xFA,
}

impl TryFrom<u8> for CivCommandCode {
    type Error = ParseError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::Frequency),
            0x01 => Ok(Self::Mode),
            0x03 => Ok(Self::ReadFrequency),
            0x04 => Ok(Self::ReadMode),
            0x05 => Ok(Self::SetFrequency),
            0x06 => Ok(Self::SetMode),
            0x07 => Ok(Self::VfoSelect),
            0x0E => Ok(Self::Scan),
            0x0F => Ok(Self::Split),
            0x10 => Ok(Self::TuningStep),
            0x11 => Ok(Self::Attenuator),
            0x12 => Ok(Self::Antenna),
            0x13 => Ok(Self::Announce),
            0x14 => Ok(Self::Level),
            0x15 => Ok(Self::ReadMeter),
            0x16 => Ok(Self::Preamp),
            0x17 => Ok(Self::Agc),
            0x18 => Ok(Self::NoiseBlanker),
            0x1C => Ok(Self::Ptt),
            0x1A => Ok(Self::Transceive),
            0xFB => Ok(Self::Ok),
            0xFA => Ok(Self::Ng),
            _ => Err(ParseError::UnknownCommand(format!(
                "CI-V cmd 0x{:02X}",
                value
            ))),
        }
    }
}

/// Parsed CI-V command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CivCommand {
    /// Destination address
    pub to_addr: u8,
    /// Source address
    pub from_addr: u8,
    /// Command type
    pub command: CivCommandType,
}

/// CI-V command types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CivCommandType {
    /// Set frequency (BCD encoded Hz)
    SetFrequency { hz: u64 },
    /// Get/report frequency
    GetFrequency,
    /// Frequency response
    FrequencyReport { hz: u64 },
    /// Set mode
    SetMode { mode: u8, filter: u8 },
    /// Get mode
    GetMode,
    /// Mode response
    ModeReport { mode: u8, filter: u8 },
    /// Select VFO
    VfoSelect { vfo: u8 },
    /// Set PTT
    SetPtt { on: bool },
    /// PTT status
    PttReport { on: bool },
    /// Split on/off
    Split { on: bool },
    /// Transceive mode (auto-information): 0x1A 0x05
    /// When enabled, radio sends unsolicited updates
    Transceive { enabled: bool },
    /// OK acknowledgment
    Ok,
    /// Error/NG response
    Ng,
    /// Unknown command
    Unknown {
        cmd: u8,
        subcmd: Option<u8>,
        data: Vec<u8>,
    },
}

/// Streaming CI-V protocol codec
pub struct CivCodec {
    buffer: Vec<u8>,
}

impl CivCodec {
    /// Create a new CI-V codec
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(64),
        }
    }

    /// Find the start of a valid frame (FE FE sequence)
    fn find_preamble(&self) -> Option<usize> {
        self.buffer
            .windows(2)
            .position(|w| w[0] == PREAMBLE && w[1] == PREAMBLE)
    }

    /// Parse a complete frame
    fn parse_frame(frame: &[u8]) -> Result<CivCommand, ParseError> {
        // Minimum frame: FE FE to from cmd FD = 6 bytes
        if frame.len() < 6 {
            return Err(ParseError::Incomplete {
                needed: 6 - frame.len(),
            });
        }

        // Verify preamble
        if frame[0] != PREAMBLE || frame[1] != PREAMBLE {
            return Err(ParseError::InvalidFrame("missing preamble".into()));
        }

        // Verify terminator
        if frame[frame.len() - 1] != TERMINATOR {
            return Err(ParseError::InvalidFrame("missing terminator".into()));
        }

        let to_addr = frame[2];
        let from_addr = frame[3];
        let cmd = frame[4];
        let data = &frame[5..frame.len() - 1];

        let command = Self::parse_command(cmd, data)?;

        Ok(CivCommand {
            to_addr,
            from_addr,
            command,
        })
    }

    /// Parse command and data into CivCommandType
    fn parse_command(cmd: u8, data: &[u8]) -> Result<CivCommandType, ParseError> {
        match cmd {
            0x00 | 0x05 => {
                // Set frequency
                if data.is_empty() {
                    Ok(CivCommandType::GetFrequency)
                } else {
                    let hz = bcd_to_frequency(data)?;
                    Ok(CivCommandType::SetFrequency { hz })
                }
            }
            0x03 => {
                // Frequency query (no data) or response (with BCD data)
                if data.is_empty() {
                    Ok(CivCommandType::GetFrequency)
                } else {
                    let hz = bcd_to_frequency(data)?;
                    Ok(CivCommandType::FrequencyReport { hz })
                }
            }
            0x01 | 0x06 => {
                // Set mode
                if data.is_empty() {
                    Ok(CivCommandType::GetMode)
                } else {
                    let mode = data.first().copied().unwrap_or(0);
                    let filter = data.get(1).copied().unwrap_or(0);
                    Ok(CivCommandType::SetMode { mode, filter })
                }
            }
            0x04 => {
                // Mode query (no data) or response (with mode/filter data)
                if data.is_empty() {
                    Ok(CivCommandType::GetMode)
                } else {
                    let mode = data.first().copied().unwrap_or(0);
                    let filter = data.get(1).copied().unwrap_or(0);
                    Ok(CivCommandType::ModeReport { mode, filter })
                }
            }
            0x07 => {
                // VFO select
                let vfo = data.first().copied().unwrap_or(0);
                Ok(CivCommandType::VfoSelect { vfo })
            }
            0x1C => {
                // PTT control
                if data.is_empty() {
                    Ok(CivCommandType::SetPtt { on: false })
                } else {
                    // Subcmd 0x00 = PTT, data[1] = on/off
                    let on = data.get(1).map(|&v| v != 0).unwrap_or(false);
                    Ok(CivCommandType::SetPtt { on })
                }
            }
            0x0F => {
                // Split
                let on = data.first().map(|&v| v != 0).unwrap_or(false);
                Ok(CivCommandType::Split { on })
            }
            0x1A => {
                // Transceive mode and other settings
                // Subcmd 0x05 = Transceive on/off
                let subcmd = data.first().copied().unwrap_or(0);
                if subcmd == 0x05 {
                    let enabled = data.get(1).map(|&v| v != 0).unwrap_or(false);
                    Ok(CivCommandType::Transceive { enabled })
                } else {
                    // Other 0x1A commands
                    let rest = if data.len() > 1 {
                        data[1..].to_vec()
                    } else {
                        vec![]
                    };
                    Ok(CivCommandType::Unknown {
                        cmd,
                        subcmd: Some(subcmd),
                        data: rest,
                    })
                }
            }
            0xFB => Ok(CivCommandType::Ok),
            0xFA => Ok(CivCommandType::Ng),
            _ => {
                let subcmd = data.first().copied();
                let rest = if data.len() > 1 {
                    data[1..].to_vec()
                } else {
                    vec![]
                };
                Ok(CivCommandType::Unknown {
                    cmd,
                    subcmd,
                    data: rest,
                })
            }
        }
    }
}

impl Default for CivCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl ProtocolCodec for CivCodec {
    type Command = CivCommand;

    fn push_bytes(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);

        // Prevent buffer overflow
        if self.buffer.len() > MAX_FRAME_LEN * 4 {
            let start = self.buffer.len() - MAX_FRAME_LEN;
            self.buffer = self.buffer[start..].to_vec();
        }
    }

    fn next_command(&mut self) -> Option<Self::Command> {
        self.next_command_with_bytes().map(|(cmd, _)| cmd)
    }

    fn next_command_with_bytes(&mut self) -> Option<(Self::Command, Vec<u8>)> {
        // Find preamble
        let preamble_pos = self.find_preamble()?;

        // Discard bytes before preamble
        if preamble_pos > 0 {
            self.buffer.drain(..preamble_pos);
        }

        // Find terminator
        let term_pos = self.buffer.iter().position(|&b| b == TERMINATOR)?;

        // Extract complete frame
        let frame: Vec<u8> = self.buffer.drain(..=term_pos).collect();

        match Self::parse_frame(&frame) {
            Ok(cmd) => Some((cmd, frame)),
            Err(e) => {
                tracing::warn!("Failed to parse CI-V frame: {}", e);
                None
            }
        }
    }

    fn clear(&mut self) {
        self.buffer.clear();
    }
}

impl ToRadioResponse for CivCommand {
    fn to_radio_response(&self) -> RadioResponse {
        match &self.command {
            CivCommandType::SetFrequency { hz } => RadioResponse::Frequency { hz: *hz },
            CivCommandType::GetFrequency => RadioResponse::Unknown { data: vec![] },
            CivCommandType::FrequencyReport { hz } => RadioResponse::Frequency { hz: *hz },
            CivCommandType::SetMode { mode, .. } => RadioResponse::Mode {
                mode: civ_mode_to_operating_mode(*mode),
            },
            CivCommandType::GetMode => RadioResponse::Unknown { data: vec![] },
            CivCommandType::ModeReport { mode, .. } => RadioResponse::Mode {
                mode: civ_mode_to_operating_mode(*mode),
            },
            CivCommandType::VfoSelect { vfo } => RadioResponse::Vfo {
                vfo: match vfo {
                    0x00 => Vfo::A,
                    0x01 => Vfo::B,
                    _ => Vfo::A,
                },
            },
            CivCommandType::SetPtt { on } => RadioResponse::Ptt { active: *on },
            CivCommandType::PttReport { on } => RadioResponse::Ptt { active: *on },
            CivCommandType::Split { on } => RadioResponse::Vfo {
                vfo: if *on { Vfo::Split } else { Vfo::A },
            },
            CivCommandType::Transceive { enabled } => RadioResponse::AutoInfo { enabled: *enabled },
            CivCommandType::Ok | CivCommandType::Ng => RadioResponse::Unknown { data: vec![] },
            CivCommandType::Unknown { cmd, data, .. } => RadioResponse::Unknown {
                data: std::iter::once(*cmd).chain(data.iter().copied()).collect(),
            },
        }
    }
}

impl ToRadioRequest for CivCommand {
    fn to_radio_request(&self) -> RadioRequest {
        match &self.command {
            CivCommandType::SetFrequency { hz } => RadioRequest::SetFrequency { hz: *hz },
            CivCommandType::GetFrequency => RadioRequest::GetFrequency,
            CivCommandType::FrequencyReport { .. } => RadioRequest::Unknown { data: vec![] },
            CivCommandType::SetMode { mode, .. } => RadioRequest::SetMode {
                mode: civ_mode_to_operating_mode(*mode),
            },
            CivCommandType::GetMode => RadioRequest::GetMode,
            CivCommandType::ModeReport { .. } => RadioRequest::Unknown { data: vec![] },
            CivCommandType::VfoSelect { vfo } => RadioRequest::SetVfo {
                vfo: match vfo {
                    0x00 => Vfo::A,
                    0x01 => Vfo::B,
                    _ => Vfo::A,
                },
            },
            CivCommandType::SetPtt { on } => RadioRequest::SetPtt { active: *on },
            CivCommandType::PttReport { .. } => RadioRequest::Unknown { data: vec![] },
            CivCommandType::Split { on } => RadioRequest::SetVfo {
                vfo: if *on { Vfo::Split } else { Vfo::A },
            },
            CivCommandType::Transceive { enabled } => {
                RadioRequest::SetAutoInfo { enabled: *enabled }
            }
            CivCommandType::Ok | CivCommandType::Ng => RadioRequest::Unknown { data: vec![] },
            CivCommandType::Unknown { cmd, data, .. } => RadioRequest::Unknown {
                data: std::iter::once(*cmd).chain(data.iter().copied()).collect(),
            },
        }
    }
}

impl CivCommand {
    /// Create a new CI-V command
    pub fn new(to_addr: u8, from_addr: u8, command: CivCommandType) -> Self {
        Self {
            to_addr,
            from_addr,
            command,
        }
    }

    /// Create a command to send to a specific radio
    pub fn to_radio(radio_addr: u8, command: CivCommandType) -> Self {
        Self::new(radio_addr, CONTROLLER_ADDR, command)
    }

    /// Create a command from a radio
    pub fn from_radio(radio_addr: u8, command: CivCommandType) -> Self {
        Self::new(CONTROLLER_ADDR, radio_addr, command)
    }
}

impl FromRadioRequest for CivCommand {
    fn from_radio_request(req: &RadioRequest) -> Option<Self> {
        let civ_cmd = match req {
            RadioRequest::SetFrequency { hz } => CivCommandType::SetFrequency { hz: *hz },
            RadioRequest::GetFrequency => CivCommandType::GetFrequency,
            RadioRequest::SetMode { mode } => CivCommandType::SetMode {
                mode: operating_mode_to_civ(*mode),
                filter: 1,
            },
            RadioRequest::GetMode => CivCommandType::GetMode,
            RadioRequest::SetPtt { active } => CivCommandType::SetPtt { on: *active },
            RadioRequest::GetPtt => return None, // No direct query in CI-V
            RadioRequest::SetVfo { vfo } => match vfo {
                Vfo::Split => CivCommandType::Split { on: true },
                Vfo::A => CivCommandType::VfoSelect { vfo: 0x00 },
                Vfo::B => CivCommandType::VfoSelect { vfo: 0x01 },
                Vfo::Memory => CivCommandType::VfoSelect { vfo: 0x02 },
            },
            RadioRequest::GetVfo => return None, // No direct query in CI-V
            RadioRequest::GetId => return None,
            RadioRequest::GetStatus => return None,
            RadioRequest::SetPower { .. } => return None,
            RadioRequest::SetAutoInfo { enabled } => {
                CivCommandType::Transceive { enabled: *enabled }
            }
            RadioRequest::GetAutoInfo => return None,
            RadioRequest::GetControlBand | RadioRequest::GetTransmitBand => return None,
            RadioRequest::Unknown { .. } => return None,
        };

        Some(CivCommand::to_radio(BROADCAST_ADDR, civ_cmd))
    }
}

impl FromRadioResponse for CivCommand {
    fn from_radio_response(resp: &RadioResponse) -> Option<Self> {
        let civ_cmd = match resp {
            RadioResponse::Frequency { hz } => CivCommandType::FrequencyReport { hz: *hz },
            RadioResponse::Mode { mode } => CivCommandType::ModeReport {
                mode: operating_mode_to_civ(*mode),
                filter: 1,
            },
            RadioResponse::Ptt { active } => CivCommandType::PttReport { on: *active },
            RadioResponse::Vfo { vfo } => match vfo {
                Vfo::Split => CivCommandType::Split { on: true },
                Vfo::A => CivCommandType::VfoSelect { vfo: 0x00 },
                Vfo::B => CivCommandType::VfoSelect { vfo: 0x01 },
                Vfo::Memory => CivCommandType::VfoSelect { vfo: 0x02 },
            },
            RadioResponse::Id { .. } => return None,
            RadioResponse::Status { frequency_hz, .. } => {
                frequency_hz.map(|hz| CivCommandType::FrequencyReport { hz })?
            }
            RadioResponse::AutoInfo { enabled } => CivCommandType::Transceive { enabled: *enabled },
            RadioResponse::ControlBand { .. } | RadioResponse::TransmitBand { .. } => return None,
            RadioResponse::Unknown { .. } => return None,
        };

        Some(CivCommand::to_radio(BROADCAST_ADDR, civ_cmd))
    }
}

impl EncodeCommand for CivCommand {
    fn encode(&self) -> Vec<u8> {
        let mut frame = vec![PREAMBLE, PREAMBLE, self.to_addr, self.from_addr];

        match &self.command {
            CivCommandType::SetFrequency { hz } => {
                frame.push(0x05);
                frame.extend(frequency_to_bcd(*hz));
            }
            CivCommandType::GetFrequency => {
                frame.push(0x03);
            }
            CivCommandType::FrequencyReport { hz } => {
                frame.push(0x03);
                frame.extend(frequency_to_bcd(*hz));
            }
            CivCommandType::SetMode { mode, filter } => {
                frame.push(0x06);
                frame.push(*mode);
                frame.push(*filter);
            }
            CivCommandType::GetMode => {
                frame.push(0x04);
            }
            CivCommandType::ModeReport { mode, filter } => {
                frame.push(0x04);
                frame.push(*mode);
                frame.push(*filter);
            }
            CivCommandType::VfoSelect { vfo } => {
                frame.push(0x07);
                frame.push(*vfo);
            }
            CivCommandType::SetPtt { on } => {
                frame.push(0x1C);
                frame.push(0x00);
                frame.push(if *on { 0x01 } else { 0x00 });
            }
            CivCommandType::PttReport { on } => {
                frame.push(0x1C);
                frame.push(0x00);
                frame.push(if *on { 0x01 } else { 0x00 });
            }
            CivCommandType::Split { on } => {
                frame.push(0x0F);
                frame.push(if *on { 0x01 } else { 0x00 });
            }
            CivCommandType::Transceive { enabled } => {
                frame.push(0x1A);
                frame.push(0x05); // Subcmd for transceive
                frame.push(if *enabled { 0x01 } else { 0x00 });
            }
            CivCommandType::Ok => {
                frame.push(0xFB);
            }
            CivCommandType::Ng => {
                frame.push(0xFA);
            }
            CivCommandType::Unknown { cmd, subcmd, data } => {
                frame.push(*cmd);
                if let Some(sc) = subcmd {
                    frame.push(*sc);
                }
                frame.extend(data);
            }
        }

        frame.push(TERMINATOR);
        frame
    }
}

/// Convert BCD-encoded bytes to frequency in Hz
/// CI-V uses little-endian BCD (least significant digit first)
fn bcd_to_frequency(data: &[u8]) -> Result<u64, ParseError> {
    let mut freq: u64 = 0;
    let mut multiplier: u64 = 1;

    for &byte in data {
        let low = (byte & 0x0F) as u64;
        let high = ((byte >> 4) & 0x0F) as u64;

        if low > 9 || high > 9 {
            return Err(ParseError::InvalidBcd(byte));
        }

        freq += low * multiplier;
        multiplier *= 10;
        freq += high * multiplier;
        multiplier *= 10;
    }

    Ok(freq)
}

/// Convert frequency in Hz to BCD-encoded bytes
/// Returns 5 bytes (10 BCD digits), little-endian
fn frequency_to_bcd(hz: u64) -> Vec<u8> {
    let mut result = Vec::with_capacity(5);
    let mut remaining = hz;

    for _ in 0..5 {
        let low = (remaining % 10) as u8;
        remaining /= 10;
        let high = (remaining % 10) as u8;
        remaining /= 10;
        result.push((high << 4) | low);
    }

    result
}

/// Convert CI-V mode number to OperatingMode
fn civ_mode_to_operating_mode(mode: u8) -> OperatingMode {
    match mode {
        0x00 => OperatingMode::Lsb,
        0x01 => OperatingMode::Usb,
        0x02 => OperatingMode::Am,
        0x03 => OperatingMode::Cw,
        0x04 => OperatingMode::Rtty,
        0x05 => OperatingMode::Fm,
        0x06 => OperatingMode::CwR,
        0x07 => OperatingMode::RttyR,
        0x08 => OperatingMode::DataL,
        0x09 => OperatingMode::DataU, // Icom calls this DATA-FM sometimes
        0x11 => OperatingMode::DataL, // Some Icoms use different codes
        0x12 => OperatingMode::DataU,
        _ => OperatingMode::Usb,
    }
}

/// Convert OperatingMode to CI-V mode number
fn operating_mode_to_civ(mode: OperatingMode) -> u8 {
    match mode {
        OperatingMode::Lsb => 0x00,
        OperatingMode::Usb => 0x01,
        OperatingMode::Am => 0x02,
        OperatingMode::Cw => 0x03,
        OperatingMode::Rtty => 0x04,
        OperatingMode::Fm | OperatingMode::FmN => 0x05,
        OperatingMode::CwR => 0x06,
        OperatingMode::RttyR => 0x07,
        OperatingMode::DataL | OperatingMode::DigL | OperatingMode::Dig => 0x08,
        OperatingMode::DataU | OperatingMode::DigU | OperatingMode::Data | OperatingMode::Pkt => {
            0x09
        }
    }
}

/// Generate a probe command to detect CI-V radios
/// This reads the frequency, which should work on any Icom radio
pub fn probe_command(radio_addr: u8) -> Vec<u8> {
    CivCommand::to_radio(radio_addr, CivCommandType::GetFrequency).encode()
}

/// Check if a response looks like a valid CI-V frame
pub fn is_valid_frame(data: &[u8]) -> bool {
    data.len() >= 6
        && data[0] == PREAMBLE
        && data[1] == PREAMBLE
        && data[data.len() - 1] == TERMINATOR
}

/// Extract the source address from a CI-V frame
pub fn extract_source_address(data: &[u8]) -> Option<u8> {
    if is_valid_frame(data) {
        Some(data[3])
    } else {
        None
    }
}

crate::impl_radio_codec!(CivCodec);

#[cfg(test)]
mod tests {
    use super::{bcd_to_frequency, frequency_to_bcd, CivCodec, CivCommand, CivCommandType};
    use crate::{
        EncodeCommand, FromRadioRequest, ProtocolCodec, RadioRequest, RadioResponse,
        ToRadioResponse,
    };

    #[test]
    fn test_bcd_to_frequency() {
        // 14.250.000 Hz in BCD little-endian
        // 14250000 -> digits from LSB: 0,0,0,0,5,2,4,1,0,0
        // Byte 0: low=0 (10^0), high=0 (10^1) → 0x00
        // Byte 1: low=0 (10^2), high=0 (10^3) → 0x00
        // Byte 2: low=5 (10^4), high=2 (10^5) → 0x25
        // Byte 3: low=4 (10^6), high=1 (10^7) → 0x14
        // Byte 4: low=0 (10^8), high=0 (10^9) → 0x00
        let bcd = [0x00, 0x00, 0x25, 0x14, 0x00];
        let freq = bcd_to_frequency(&bcd).unwrap();
        assert_eq!(freq, 14_250_000);
    }

    #[test]
    fn test_frequency_to_bcd() {
        let bcd = frequency_to_bcd(14_250_000);
        // 14250000 in little-endian BCD:
        // 00 (ones+tens), 00 (hundreds+thousands), 25 (ten-thousands+hundred-thousands),
        // 14 (millions+ten-millions), 00 (hundred-millions+billions)
        assert_eq!(bcd, vec![0x00, 0x00, 0x25, 0x14, 0x00]);
    }

    #[test]
    fn test_bcd_roundtrip() {
        let freqs = [7_074_000, 14_250_000, 28_500_000, 144_200_000];
        for freq in freqs {
            let bcd = frequency_to_bcd(freq);
            let back = bcd_to_frequency(&bcd).unwrap();
            assert_eq!(back, freq, "Roundtrip failed for {}", freq);
        }
    }

    #[test]
    fn test_parse_frequency_response() {
        let mut codec = CivCodec::new();
        // Response: freq 14.250.000 from radio 0x94 to controller
        // BCD: [0x00, 0x00, 0x25, 0x14, 0x00]
        let frame = [
            0xFE, 0xFE, 0xE0, 0x94, 0x03, 0x00, 0x00, 0x25, 0x14, 0x00, 0xFD,
        ];
        codec.push_bytes(&frame);

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd.from_addr, 0x94);
        assert!(matches!(
            cmd.command,
            CivCommandType::FrequencyReport { hz: 14_250_000 }
        ));
    }

    #[test]
    fn test_encode_set_frequency() {
        let cmd = CivCommand::to_radio(0x94, CivCommandType::SetFrequency { hz: 14_250_000 });
        let encoded = cmd.encode();
        assert_eq!(
            encoded,
            vec![0xFE, 0xFE, 0x94, 0xE0, 0x05, 0x00, 0x00, 0x25, 0x14, 0x00, 0xFD]
        );
    }

    #[test]
    fn test_streaming_parse() {
        let mut codec = CivCodec::new();

        // Push partial frame
        codec.push_bytes(&[0xFE, 0xFE, 0xE0, 0x94]);
        assert!(codec.next_command().is_none());

        // Push rest
        codec.push_bytes(&[0xFB, 0xFD]);
        let cmd = codec.next_command().unwrap();
        assert!(matches!(cmd.command, CivCommandType::Ok));
    }

    #[test]
    fn test_to_radio_response() {
        let civ_cmd =
            CivCommand::from_radio(0x94, CivCommandType::FrequencyReport { hz: 7_074_000 });
        let response = civ_cmd.to_radio_response();
        assert_eq!(response, RadioResponse::Frequency { hz: 7_074_000 });
    }

    #[test]
    fn test_parse_transceive_enable() {
        let mut codec = CivCodec::new();
        // Frame: FE FE E0 94 1A 05 01 FD (transceive on from radio 0x94)
        let frame = [0xFE, 0xFE, 0xE0, 0x94, 0x1A, 0x05, 0x01, 0xFD];
        codec.push_bytes(&frame);

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd.from_addr, 0x94);
        assert!(matches!(
            cmd.command,
            CivCommandType::Transceive { enabled: true }
        ));
        assert_eq!(
            cmd.to_radio_response(),
            RadioResponse::AutoInfo { enabled: true }
        );
    }

    #[test]
    fn test_parse_transceive_disable() {
        let mut codec = CivCodec::new();
        // Frame: FE FE E0 94 1A 05 00 FD (transceive off)
        let frame = [0xFE, 0xFE, 0xE0, 0x94, 0x1A, 0x05, 0x00, 0xFD];
        codec.push_bytes(&frame);

        let cmd = codec.next_command().unwrap();
        assert!(matches!(
            cmd.command,
            CivCommandType::Transceive { enabled: false }
        ));
    }

    #[test]
    fn test_encode_transceive() {
        let cmd = CivCommand::to_radio(0x94, CivCommandType::Transceive { enabled: true });
        let encoded = cmd.encode();
        assert_eq!(
            encoded,
            vec![0xFE, 0xFE, 0x94, 0xE0, 0x1A, 0x05, 0x01, 0xFD]
        );

        let cmd = CivCommand::to_radio(0x94, CivCommandType::Transceive { enabled: false });
        let encoded = cmd.encode();
        assert_eq!(
            encoded,
            vec![0xFE, 0xFE, 0x94, 0xE0, 0x1A, 0x05, 0x00, 0xFD]
        );
    }

    #[test]
    fn test_from_radio_request_transceive() {
        let civ_cmd =
            CivCommand::from_radio_request(&RadioRequest::SetAutoInfo { enabled: true }).unwrap();
        assert!(matches!(
            civ_cmd.command,
            CivCommandType::Transceive { enabled: true }
        ));
    }
}
