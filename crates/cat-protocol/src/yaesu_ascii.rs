//! Yaesu ASCII CAT Protocol Implementation
//!
//! Modern Yaesu radios (FT-991, FT-991A, FTDX-101D, FTDX-10, FT-710) use an
//! ASCII-based protocol similar to Kenwood, with semicolon-terminated commands.
//!
//! # Differences from Kenwood Protocol
//! - Frequency uses 9 digits (1 Hz resolution): `FA014250000;`
//! - Mode command includes receiver selector: `MD02;` (0=main, 2=USB)
//! - Different mode codes (includes C4FM, DATA-FM, etc.)
//! - ID responses: FT-991=0570, FTDX-101D=0681, FTDX-10=0761
//!
//! # References
//! - [FT-991A CAT Manual](https://yaesu.com/Files/4CB893D7-1018-01AF-FA97E9E9AD48B50C/FT-991A_CAT_OM_ENG_1711-D.pdf)
//! - [FTDX-10 CAT Manual](https://www.yaesu.com/Files/4CB893D7-1018-01AF-FA97E9E9AD48B50C/FTDX10_CAT_OM_ENG_2308-F.pdf)

use crate::command::{OperatingMode, RadioCommand, Vfo};
use crate::error::ParseError;
use crate::{EncodeCommand, FromRadioCommand, ProtocolCodec, RadioCodec, ToRadioCommand};

/// Maximum command length (reasonable limit to prevent buffer overflow)
const MAX_COMMAND_LEN: usize = 64;

/// Yaesu ASCII frequency digit count (9 digits = 1 Hz resolution up to 999 MHz)
const FREQ_DIGITS: usize = 9;

/// Yaesu ASCII protocol command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YaesuAsciiCommand {
    /// Set/get VFO A frequency: FA014250000;
    FrequencyA(Option<u64>),
    /// Set/get VFO B frequency: FB007074000;
    FrequencyB(Option<u64>),
    /// Set/get mode: MD02; (P1=receiver 0=main, P2=mode)
    Mode {
        /// Receiver selector (0=main, 1=sub if available)
        receiver: u8,
        /// Mode value (None = query)
        mode: Option<u8>,
    },
    /// Transmit: TX0; (0=off) or TX1; (1=on) or TX2; (tune)
    Transmit(Option<u8>),
    /// Radio identification query: ID;
    Id(Option<String>),
    /// Information/status query: IF...;
    Info(Option<YaesuAsciiInfo>),
    /// VFO select: VS0; (0=VFO A, 1=VFO B)
    VfoSelect(Option<u8>),
    /// Split mode: ST0; or ST1;
    Split(Option<bool>),
    /// Power on/off: PS0; or PS1;
    Power(Option<bool>),
    /// Auto-information mode: AI0; (off) or AI1; (on) or AI; (query)
    AutoInfo(Option<bool>),
    /// S-meter/power meter read: SM0; (returns SM0xxx;)
    SMeter(Option<u16>),
    /// RF power output setting: PC000-100;
    RfPower(Option<u8>),
    /// Unknown/unrecognized command
    Unknown(String),
}

/// Parsed IF (information) response data for Yaesu ASCII
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YaesuAsciiInfo {
    /// Memory channel number
    pub memory_channel: u16,
    /// Current frequency in Hz
    pub frequency_hz: u64,
    /// Clarifier direction (+/-)
    pub clar_direction: i8,
    /// RIT/XIT offset in Hz
    pub clar_offset: i32,
    /// Clarifier on/off
    pub clar_on: bool,
    /// Operating mode
    pub mode: u8,
    /// VFO/Memory mode (0=VFO, 1=Memory)
    pub vfo_memory: u8,
    /// CTCSS/DCS status
    pub ctcss_dcs: u8,
    /// TX status (0=RX, 1=TX)
    pub tx: bool,
    /// Operating status
    pub operation: u8,
}

/// Streaming Yaesu ASCII protocol codec
pub struct YaesuAsciiCodec {
    buffer: Vec<u8>,
}

impl YaesuAsciiCodec {
    /// Create a new Yaesu ASCII codec
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(64),
        }
    }

    /// Parse a complete command string (without terminator)
    fn parse_command(cmd: &str) -> Result<YaesuAsciiCommand, ParseError> {
        if cmd.len() < 2 {
            return Err(ParseError::InvalidFrame("command too short".into()));
        }

        let prefix = &cmd[..2];
        let params = &cmd[2..];

        match prefix {
            "FA" => {
                if params.is_empty() {
                    Ok(YaesuAsciiCommand::FrequencyA(None))
                } else {
                    let freq = params
                        .parse::<u64>()
                        .map_err(|_| ParseError::InvalidFrequency(params.into()))?;
                    Ok(YaesuAsciiCommand::FrequencyA(Some(freq)))
                }
            }
            "FB" => {
                if params.is_empty() {
                    Ok(YaesuAsciiCommand::FrequencyB(None))
                } else {
                    let freq = params
                        .parse::<u64>()
                        .map_err(|_| ParseError::InvalidFrequency(params.into()))?;
                    Ok(YaesuAsciiCommand::FrequencyB(Some(freq)))
                }
            }
            "MD" => {
                // Yaesu MD format: MD + receiver(1) + mode(1)
                // e.g., MD02; = main receiver, USB
                if params.is_empty() {
                    Ok(YaesuAsciiCommand::Mode {
                        receiver: 0,
                        mode: None,
                    })
                } else if params.len() == 1 {
                    // Query for specific receiver: MD0;
                    let receiver = params
                        .parse::<u8>()
                        .map_err(|_| ParseError::InvalidFrame("invalid receiver".into()))?;
                    Ok(YaesuAsciiCommand::Mode {
                        receiver,
                        mode: None,
                    })
                } else {
                    let receiver = params[0..1]
                        .parse::<u8>()
                        .map_err(|_| ParseError::InvalidFrame("invalid receiver".into()))?;
                    // Mode can be hex digit (0-9, A-E)
                    let mode_char = params.chars().nth(1).unwrap_or('0');
                    let mode = parse_yaesu_mode_char(mode_char)?;
                    Ok(YaesuAsciiCommand::Mode {
                        receiver,
                        mode: Some(mode),
                    })
                }
            }
            "TX" => {
                if params.is_empty() {
                    Ok(YaesuAsciiCommand::Transmit(Some(1))) // TX; means transmit
                } else {
                    let tx = params
                        .parse::<u8>()
                        .map_err(|_| ParseError::InvalidFrame("invalid TX value".into()))?;
                    Ok(YaesuAsciiCommand::Transmit(Some(tx)))
                }
            }
            "ID" => {
                if params.is_empty() {
                    Ok(YaesuAsciiCommand::Id(None))
                } else {
                    Ok(YaesuAsciiCommand::Id(Some(params.to_string())))
                }
            }
            "IF" => {
                if params.is_empty() {
                    Ok(YaesuAsciiCommand::Info(None))
                } else {
                    let info = Self::parse_info(params)?;
                    Ok(YaesuAsciiCommand::Info(Some(info)))
                }
            }
            "VS" => {
                if params.is_empty() {
                    Ok(YaesuAsciiCommand::VfoSelect(None))
                } else {
                    let vfo = params
                        .parse::<u8>()
                        .map_err(|_| ParseError::InvalidFrame("invalid VFO".into()))?;
                    Ok(YaesuAsciiCommand::VfoSelect(Some(vfo)))
                }
            }
            "ST" => {
                if params.is_empty() {
                    Ok(YaesuAsciiCommand::Split(None))
                } else {
                    let split = params != "0";
                    Ok(YaesuAsciiCommand::Split(Some(split)))
                }
            }
            "PS" => {
                if params.is_empty() {
                    Ok(YaesuAsciiCommand::Power(None))
                } else {
                    let on = params != "0";
                    Ok(YaesuAsciiCommand::Power(Some(on)))
                }
            }
            "AI" => {
                if params.is_empty() {
                    Ok(YaesuAsciiCommand::AutoInfo(None))
                } else {
                    let enabled = params != "0";
                    Ok(YaesuAsciiCommand::AutoInfo(Some(enabled)))
                }
            }
            "SM" => {
                if params.is_empty() || params.len() == 1 {
                    Ok(YaesuAsciiCommand::SMeter(None))
                } else {
                    // SM0xxx; format - skip receiver digit, parse value
                    let value = params[1..]
                        .parse::<u16>()
                        .map_err(|_| ParseError::InvalidFrame("invalid S-meter".into()))?;
                    Ok(YaesuAsciiCommand::SMeter(Some(value)))
                }
            }
            "PC" => {
                if params.is_empty() {
                    Ok(YaesuAsciiCommand::RfPower(None))
                } else {
                    let power = params
                        .parse::<u8>()
                        .map_err(|_| ParseError::InvalidFrame("invalid power".into()))?;
                    Ok(YaesuAsciiCommand::RfPower(Some(power)))
                }
            }
            _ => Ok(YaesuAsciiCommand::Unknown(cmd.to_string())),
        }
    }

    /// Parse IF response parameters for Yaesu ASCII
    /// Format: IFmmmfffffffffff+rrrrr0teleeeee;
    fn parse_info(params: &str) -> Result<YaesuAsciiInfo, ParseError> {
        // Yaesu IF response format (27+ chars):
        // mmm: 3-digit memory channel
        // fffffffffff: 9-digit frequency (newer models may vary)
        // +/-: clarifier direction
        // rrrrr: 4-digit clarifier offset
        // 0: always 0
        // t: RX/TX status
        // e: mode
        // l: VFO/Memory
        // eeeee: CTCSS/DCS, scan, simplex/split, tone, shift

        if params.len() < 20 {
            return Err(ParseError::InvalidFrame(format!(
                "IF response too short: {} chars",
                params.len()
            )));
        }

        let memory_channel = params[0..3].parse::<u16>().unwrap_or(0);

        // Frequency can be 9-11 digits depending on model
        let freq_end = 3 + FREQ_DIGITS;
        let frequency_hz = if freq_end <= params.len() {
            params[3..freq_end].parse::<u64>().unwrap_or(0)
        } else {
            params[3..].parse::<u64>().unwrap_or(0)
        };

        // Parse remaining fields if available
        let clar_direction = if params.len() > freq_end {
            match params.chars().nth(freq_end) {
                Some('+') => 1,
                Some('-') => -1,
                _ => 0,
            }
        } else {
            0
        };

        let clar_offset_start = freq_end + 1;
        let clar_offset = if params.len() > clar_offset_start + 4 {
            params[clar_offset_start..clar_offset_start + 4]
                .parse::<i32>()
                .unwrap_or(0)
                * clar_direction as i32
        } else {
            0
        };

        let clar_on_pos = freq_end + 5;
        let clar_on = params.chars().nth(clar_on_pos) == Some('1');

        let tx_pos = clar_on_pos + 2;
        let tx = params.chars().nth(tx_pos) == Some('1');

        let mode_pos = tx_pos + 1;
        let mode = params
            .chars()
            .nth(mode_pos)
            .and_then(|c| parse_yaesu_mode_char(c).ok())
            .unwrap_or(2);

        let vfo_memory_pos = mode_pos + 1;
        let vfo_memory = params
            .chars()
            .nth(vfo_memory_pos)
            .and_then(|c| c.to_digit(10))
            .unwrap_or(0) as u8;

        Ok(YaesuAsciiInfo {
            memory_channel,
            frequency_hz,
            clar_direction,
            clar_offset,
            clar_on,
            mode,
            vfo_memory,
            ctcss_dcs: 0,
            tx,
            operation: 0,
        })
    }
}

impl Default for YaesuAsciiCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl ProtocolCodec for YaesuAsciiCodec {
    type Command = YaesuAsciiCommand;

    fn push_bytes(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);

        // Prevent buffer overflow
        if self.buffer.len() > MAX_COMMAND_LEN * 4 {
            let start = self.buffer.len() - MAX_COMMAND_LEN;
            self.buffer = self.buffer[start..].to_vec();
        }
    }

    fn next_command(&mut self) -> Option<Self::Command> {
        // Find terminator
        let term_pos = self.buffer.iter().position(|&b| b == b';')?;

        // Extract command bytes
        let cmd_bytes: Vec<u8> = self.buffer.drain(..=term_pos).collect();

        // Parse as ASCII (strip terminator)
        let cmd_str = String::from_utf8_lossy(&cmd_bytes[..cmd_bytes.len() - 1]);

        match Self::parse_command(&cmd_str) {
            Ok(cmd) => Some(cmd),
            Err(e) => {
                tracing::warn!("Failed to parse Yaesu ASCII command: {}", e);
                Some(YaesuAsciiCommand::Unknown(cmd_str.into_owned()))
            }
        }
    }

    fn clear(&mut self) {
        self.buffer.clear();
    }
}

impl ToRadioCommand for YaesuAsciiCommand {
    fn to_radio_command(&self) -> RadioCommand {
        match self {
            YaesuAsciiCommand::FrequencyA(Some(hz)) => RadioCommand::SetFrequency { hz: *hz },
            YaesuAsciiCommand::FrequencyA(None) => RadioCommand::GetFrequency,
            YaesuAsciiCommand::FrequencyB(Some(hz)) => RadioCommand::SetFrequency { hz: *hz },
            YaesuAsciiCommand::FrequencyB(None) => RadioCommand::GetFrequency,
            YaesuAsciiCommand::Mode {
                mode: Some(m),
                receiver: _,
            } => RadioCommand::SetMode {
                mode: yaesu_mode_to_operating_mode(*m),
            },
            YaesuAsciiCommand::Mode {
                mode: None,
                receiver: _,
            } => RadioCommand::GetMode,
            YaesuAsciiCommand::Transmit(Some(tx)) => RadioCommand::SetPtt { active: *tx != 0 },
            YaesuAsciiCommand::Transmit(None) => RadioCommand::GetPtt,
            YaesuAsciiCommand::Id(Some(id)) => RadioCommand::IdReport { id: id.clone() },
            YaesuAsciiCommand::Id(None) => RadioCommand::GetId,
            YaesuAsciiCommand::Info(Some(info)) => RadioCommand::StatusReport {
                frequency_hz: Some(info.frequency_hz),
                mode: Some(yaesu_mode_to_operating_mode(info.mode)),
                ptt: Some(info.tx),
                vfo: Some(if info.vfo_memory == 0 { Vfo::A } else { Vfo::B }),
            },
            YaesuAsciiCommand::Info(None) => RadioCommand::GetStatus,
            YaesuAsciiCommand::VfoSelect(Some(v)) => RadioCommand::SetVfo {
                vfo: if *v == 0 { Vfo::A } else { Vfo::B },
            },
            YaesuAsciiCommand::VfoSelect(None) => RadioCommand::GetVfo,
            YaesuAsciiCommand::Split(Some(s)) => RadioCommand::SetVfo {
                vfo: if *s { Vfo::Split } else { Vfo::A },
            },
            YaesuAsciiCommand::Split(None) => RadioCommand::GetVfo,
            YaesuAsciiCommand::Power(Some(on)) => RadioCommand::SetPower { on: *on },
            YaesuAsciiCommand::Power(None) => RadioCommand::Unknown { data: vec![] },
            YaesuAsciiCommand::AutoInfo(Some(enabled)) => {
                RadioCommand::EnableAutoInfo { enabled: *enabled }
            }
            YaesuAsciiCommand::AutoInfo(None) => RadioCommand::GetAutoInfo,
            YaesuAsciiCommand::SMeter(_) | YaesuAsciiCommand::RfPower(_) => {
                RadioCommand::Unknown { data: vec![] }
            }
            YaesuAsciiCommand::Unknown(s) => RadioCommand::Unknown {
                data: s.as_bytes().to_vec(),
            },
        }
    }
}

impl FromRadioCommand for YaesuAsciiCommand {
    fn from_radio_command(cmd: &RadioCommand) -> Option<Self> {
        match cmd {
            RadioCommand::SetFrequency { hz } => Some(YaesuAsciiCommand::FrequencyA(Some(*hz))),
            RadioCommand::GetFrequency => Some(YaesuAsciiCommand::FrequencyA(None)),
            RadioCommand::FrequencyReport { hz } => Some(YaesuAsciiCommand::FrequencyA(Some(*hz))),
            RadioCommand::SetMode { mode } => Some(YaesuAsciiCommand::Mode {
                receiver: 0,
                mode: Some(operating_mode_to_yaesu(*mode)),
            }),
            RadioCommand::GetMode => Some(YaesuAsciiCommand::Mode {
                receiver: 0,
                mode: None,
            }),
            RadioCommand::ModeReport { mode } => Some(YaesuAsciiCommand::Mode {
                receiver: 0,
                mode: Some(operating_mode_to_yaesu(*mode)),
            }),
            RadioCommand::SetPtt { active: true } => Some(YaesuAsciiCommand::Transmit(Some(1))),
            RadioCommand::SetPtt { active: false } => Some(YaesuAsciiCommand::Transmit(Some(0))),
            RadioCommand::GetPtt => Some(YaesuAsciiCommand::Transmit(None)),
            RadioCommand::PttReport { active } => {
                Some(YaesuAsciiCommand::Transmit(Some(if *active {
                    1
                } else {
                    0
                })))
            }
            RadioCommand::SetVfo { vfo } => match vfo {
                Vfo::A => Some(YaesuAsciiCommand::VfoSelect(Some(0))),
                Vfo::B => Some(YaesuAsciiCommand::VfoSelect(Some(1))),
                Vfo::Split => Some(YaesuAsciiCommand::Split(Some(true))),
                Vfo::Memory => Some(YaesuAsciiCommand::VfoSelect(Some(0))),
            },
            RadioCommand::GetVfo => Some(YaesuAsciiCommand::VfoSelect(None)),
            RadioCommand::GetId => Some(YaesuAsciiCommand::Id(None)),
            RadioCommand::IdReport { id } => Some(YaesuAsciiCommand::Id(Some(id.clone()))),
            RadioCommand::GetStatus => Some(YaesuAsciiCommand::Info(None)),
            RadioCommand::SetPower { on } => Some(YaesuAsciiCommand::Power(Some(*on))),
            RadioCommand::EnableAutoInfo { enabled } => {
                Some(YaesuAsciiCommand::AutoInfo(Some(*enabled)))
            }
            RadioCommand::GetAutoInfo => Some(YaesuAsciiCommand::AutoInfo(None)),
            RadioCommand::AutoInfoReport { enabled } => {
                Some(YaesuAsciiCommand::AutoInfo(Some(*enabled)))
            }
            _ => None,
        }
    }
}

impl RadioCodec for YaesuAsciiCodec {
    fn push_bytes(&mut self, data: &[u8]) {
        ProtocolCodec::push_bytes(self, data);
    }

    fn next_command(&mut self) -> Option<RadioCommand> {
        ProtocolCodec::next_command(self).map(|cmd| cmd.to_radio_command())
    }

    fn clear(&mut self) {
        ProtocolCodec::clear(self);
    }
}

impl EncodeCommand for YaesuAsciiCommand {
    fn encode(&self) -> Vec<u8> {
        let cmd = match self {
            // 9-digit frequency format for Yaesu ASCII
            YaesuAsciiCommand::FrequencyA(Some(hz)) => format!("FA{:09}", hz),
            YaesuAsciiCommand::FrequencyA(None) => "FA".to_string(),
            YaesuAsciiCommand::FrequencyB(Some(hz)) => format!("FB{:09}", hz),
            YaesuAsciiCommand::FrequencyB(None) => "FB".to_string(),
            YaesuAsciiCommand::Mode {
                receiver,
                mode: Some(m),
            } => {
                let mode_char = yaesu_mode_to_char(*m);
                format!("MD{}{}", receiver, mode_char)
            }
            YaesuAsciiCommand::Mode {
                receiver,
                mode: None,
            } => format!("MD{}", receiver),
            YaesuAsciiCommand::Transmit(Some(tx)) => format!("TX{}", tx),
            YaesuAsciiCommand::Transmit(None) => "TX".to_string(),
            YaesuAsciiCommand::Id(Some(id)) => format!("ID{}", id),
            YaesuAsciiCommand::Id(None) => "ID".to_string(),
            YaesuAsciiCommand::Info(_) => "IF".to_string(),
            YaesuAsciiCommand::VfoSelect(Some(v)) => format!("VS{}", v),
            YaesuAsciiCommand::VfoSelect(None) => "VS".to_string(),
            YaesuAsciiCommand::Split(Some(s)) => format!("ST{}", if *s { 1 } else { 0 }),
            YaesuAsciiCommand::Split(None) => "ST".to_string(),
            YaesuAsciiCommand::Power(Some(on)) => format!("PS{}", if *on { 1 } else { 0 }),
            YaesuAsciiCommand::Power(None) => "PS".to_string(),
            YaesuAsciiCommand::AutoInfo(Some(enabled)) => {
                format!("AI{}", if *enabled { 1 } else { 0 })
            }
            YaesuAsciiCommand::AutoInfo(None) => "AI".to_string(),
            YaesuAsciiCommand::SMeter(Some(v)) => format!("SM0{:03}", v),
            YaesuAsciiCommand::SMeter(None) => "SM0".to_string(),
            YaesuAsciiCommand::RfPower(Some(p)) => format!("PC{:03}", p),
            YaesuAsciiCommand::RfPower(None) => "PC".to_string(),
            YaesuAsciiCommand::Unknown(s) => s.clone(),
        };
        format!("{};", cmd).into_bytes()
    }
}

/// Parse Yaesu mode character to numeric value
fn parse_yaesu_mode_char(c: char) -> Result<u8, ParseError> {
    match c {
        '1' => Ok(1),        // LSB
        '2' => Ok(2),        // USB
        '3' => Ok(3),        // CW-U
        '4' => Ok(4),        // FM
        '5' => Ok(5),        // AM
        '6' => Ok(6),        // RTTY-LSB
        '7' => Ok(7),        // CW-L
        '8' => Ok(8),        // DATA-LSB
        '9' => Ok(9),        // RTTY-USB
        'A' | 'a' => Ok(10), // DATA-FM
        'B' | 'b' => Ok(11), // FM-N
        'C' | 'c' => Ok(12), // DATA-USB
        'D' | 'd' => Ok(13), // AM-N
        'E' | 'e' => Ok(14), // C4FM
        '0' => Ok(0),        // Sometimes used
        _ => Err(ParseError::InvalidMode(c.to_string())),
    }
}

/// Convert Yaesu mode to character for encoding
fn yaesu_mode_to_char(mode: u8) -> char {
    match mode {
        1 => '1',
        2 => '2',
        3 => '3',
        4 => '4',
        5 => '5',
        6 => '6',
        7 => '7',
        8 => '8',
        9 => '9',
        10 => 'A',
        11 => 'B',
        12 => 'C',
        13 => 'D',
        14 => 'E',
        _ => '2', // Default to USB
    }
}

/// Convert Yaesu ASCII mode number to OperatingMode
fn yaesu_mode_to_operating_mode(mode: u8) -> OperatingMode {
    match mode {
        1 => OperatingMode::Lsb,
        2 => OperatingMode::Usb,
        3 => OperatingMode::Cw, // CW-U
        4 => OperatingMode::Fm,
        5 => OperatingMode::Am,
        6 => OperatingMode::Rtty,   // RTTY-LSB
        7 => OperatingMode::CwR,    // CW-L
        8 => OperatingMode::DataL,  // DATA-LSB
        9 => OperatingMode::RttyR,  // RTTY-USB
        10 => OperatingMode::Data,  // DATA-FM
        11 => OperatingMode::FmN,   // FM-N
        12 => OperatingMode::DataU, // DATA-USB
        13 => OperatingMode::Am,    // AM-N (narrow)
        14 => OperatingMode::Fm,    // C4FM (digital FM)
        _ => OperatingMode::Usb,
    }
}

/// Convert OperatingMode to Yaesu ASCII mode number
fn operating_mode_to_yaesu(mode: OperatingMode) -> u8 {
    match mode {
        OperatingMode::Lsb => 1,
        OperatingMode::Usb => 2,
        OperatingMode::Cw => 3,
        OperatingMode::Fm => 4,
        OperatingMode::Am => 5,
        OperatingMode::Rtty => 6,
        OperatingMode::CwR => 7,
        OperatingMode::DataL | OperatingMode::DigL => 8,
        OperatingMode::RttyR => 9,
        OperatingMode::Data | OperatingMode::Dig | OperatingMode::Pkt => 10,
        OperatingMode::FmN => 11,
        OperatingMode::DataU | OperatingMode::DigU => 12,
    }
}

/// Known Yaesu ASCII radio ID responses
pub mod radio_ids {
    /// FT-991 ID
    pub const FT_991: &str = "0570";
    /// FT-991A ID
    pub const FT_991A: &str = "0670";
    /// FTDX-101D ID
    pub const FTDX_101D: &str = "0681";
    /// FTDX-101MP ID
    pub const FTDX_101MP: &str = "0682";
    /// FTDX-10 ID
    pub const FTDX_10: &str = "0761";
    /// FT-710 ID
    pub const FT_710: &str = "0800";
}

/// Generate a probe command to detect Yaesu ASCII radios
pub fn probe_command() -> Vec<u8> {
    b"ID;".to_vec()
}

/// Check if a response looks like a valid Yaesu ASCII ID response
pub fn is_valid_id_response(data: &[u8]) -> bool {
    // Valid responses: ID0570; ID0670; ID0681; etc.
    if data.len() >= 6 && data.starts_with(b"ID") && data.ends_with(b";") {
        let id_part = &data[2..data.len() - 1];
        // Yaesu IDs are 4-digit numbers
        id_part.len() == 4 && id_part.iter().all(|b| b.is_ascii_digit())
    } else {
        false
    }
}

/// Check if an ID string matches a known Yaesu ASCII radio
pub fn is_known_yaesu_ascii_id(id: &str) -> bool {
    matches!(
        id,
        radio_ids::FT_991
            | radio_ids::FT_991A
            | radio_ids::FTDX_101D
            | radio_ids::FTDX_101MP
            | radio_ids::FTDX_10
            | radio_ids::FT_710
    )
}

#[cfg(test)]
mod tests {
    use super::{
        is_known_yaesu_ascii_id, is_valid_id_response, YaesuAsciiCodec, YaesuAsciiCommand,
    };
    use crate::{EncodeCommand, FromRadioCommand, ProtocolCodec, RadioCommand, ToRadioCommand};

    #[test]
    fn test_parse_frequency() {
        let mut codec = YaesuAsciiCodec::new();
        codec.push_bytes(b"FA014250000;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, YaesuAsciiCommand::FrequencyA(Some(14_250_000)));
    }

    #[test]
    fn test_parse_frequency_vhf() {
        let mut codec = YaesuAsciiCodec::new();
        codec.push_bytes(b"FA146520000;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, YaesuAsciiCommand::FrequencyA(Some(146_520_000)));
    }

    #[test]
    fn test_parse_mode() {
        let mut codec = YaesuAsciiCodec::new();
        codec.push_bytes(b"MD02;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(
            cmd,
            YaesuAsciiCommand::Mode {
                receiver: 0,
                mode: Some(2)
            }
        );
    }

    #[test]
    fn test_parse_mode_data() {
        let mut codec = YaesuAsciiCodec::new();
        codec.push_bytes(b"MD0C;"); // DATA-USB

        let cmd = codec.next_command().unwrap();
        assert_eq!(
            cmd,
            YaesuAsciiCommand::Mode {
                receiver: 0,
                mode: Some(12)
            }
        );
    }

    #[test]
    fn test_parse_id_query() {
        let mut codec = YaesuAsciiCodec::new();
        codec.push_bytes(b"ID;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, YaesuAsciiCommand::Id(None));
    }

    #[test]
    fn test_parse_id_response() {
        let mut codec = YaesuAsciiCodec::new();
        codec.push_bytes(b"ID0570;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, YaesuAsciiCommand::Id(Some("0570".to_string())));
    }

    #[test]
    fn test_encode_frequency() {
        let cmd = YaesuAsciiCommand::FrequencyA(Some(14_250_000));
        let encoded = cmd.encode();
        assert_eq!(encoded, b"FA014250000;");
    }

    #[test]
    fn test_encode_frequency_9_digits() {
        let cmd = YaesuAsciiCommand::FrequencyA(Some(7_074_000));
        let encoded = cmd.encode();
        // Should be 9 digits with leading zeros
        assert_eq!(encoded, b"FA007074000;");
    }

    #[test]
    fn test_encode_mode() {
        let cmd = YaesuAsciiCommand::Mode {
            receiver: 0,
            mode: Some(2),
        };
        let encoded = cmd.encode();
        assert_eq!(encoded, b"MD02;");
    }

    #[test]
    fn test_encode_mode_hex() {
        let cmd = YaesuAsciiCommand::Mode {
            receiver: 0,
            mode: Some(12), // DATA-USB = 'C'
        };
        let encoded = cmd.encode();
        assert_eq!(encoded, b"MD0C;");
    }

    #[test]
    fn test_streaming_parse() {
        let mut codec = YaesuAsciiCodec::new();

        // Push partial data
        codec.push_bytes(b"FA014");
        assert!(codec.next_command().is_none());

        // Push rest
        codec.push_bytes(b"250000;");
        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, YaesuAsciiCommand::FrequencyA(Some(14_250_000)));
    }

    #[test]
    fn test_to_radio_command() {
        let cmd = YaesuAsciiCommand::FrequencyA(Some(7_074_000));
        let radio_cmd = cmd.to_radio_command();
        assert_eq!(radio_cmd, RadioCommand::SetFrequency { hz: 7_074_000 });
    }

    #[test]
    fn test_from_radio_command() {
        let radio_cmd = RadioCommand::SetFrequency { hz: 14_250_000 };
        let cmd = YaesuAsciiCommand::from_radio_command(&radio_cmd).unwrap();
        assert_eq!(cmd, YaesuAsciiCommand::FrequencyA(Some(14_250_000)));
    }

    #[test]
    fn test_auto_info() {
        let mut codec = YaesuAsciiCodec::new();
        codec.push_bytes(b"AI1;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, YaesuAsciiCommand::AutoInfo(Some(true)));
        assert_eq!(
            cmd.to_radio_command(),
            RadioCommand::EnableAutoInfo { enabled: true }
        );
    }

    #[test]
    fn test_is_valid_id_response() {
        assert!(is_valid_id_response(b"ID0570;"));
        assert!(is_valid_id_response(b"ID0681;"));
        assert!(!is_valid_id_response(b"ID019;")); // Too short (Kenwood format)
        assert!(!is_valid_id_response(b"FA014250000;"));
    }

    #[test]
    fn test_is_known_yaesu_ascii_id() {
        assert!(is_known_yaesu_ascii_id("0570")); // FT-991
        assert!(is_known_yaesu_ascii_id("0681")); // FTDX-101D
        assert!(!is_known_yaesu_ascii_id("019")); // Kenwood ID
    }
}
