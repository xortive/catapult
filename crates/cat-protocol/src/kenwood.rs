//! Kenwood CAT Protocol Implementation
//!
//! The Kenwood protocol uses ASCII semicolon-terminated commands.
//! Commands are human-readable with 2-letter command prefixes.
//!
//! # Format
//! - Commands: `XXppppp;` where XX is command code, ppppp is parameters
//! - Responses: Same format as commands
//! - Terminator: `;` (0x3B)
//!
//! # Common Commands
//! - `FA` - VFO A frequency
//! - `FB` - VFO B frequency
//! - `MD` - Mode
//! - `TX` - Transmit
//! - `RX` - Receive
//! - `ID` - Radio identification
//! - `IF` - Information (status)

use crate::command::{OperatingMode, RadioCommand, Vfo};
use crate::error::ParseError;
use crate::{EncodeCommand, FromRadioCommand, ProtocolCodec, ToRadioCommand};

/// Maximum command length (reasonable limit to prevent buffer overflow)
const MAX_COMMAND_LEN: usize = 64;

/// Kenwood protocol command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KenwoodCommand {
    /// Set/get VFO A frequency: FA00014250000;
    FrequencyA(Option<u64>),
    /// Set/get VFO B frequency: FB00007074000;
    FrequencyB(Option<u64>),
    /// Set/get mode: MD1; (1=LSB, 2=USB, 3=CW, etc.)
    Mode(Option<u8>),
    /// Transmit: TX0; or TX1;
    Transmit(Option<bool>),
    /// Receive: RX;
    Receive,
    /// Radio identification query: ID;
    Id(Option<String>),
    /// Information/status query: IF...;
    Info(Option<KenwoodInfo>),
    /// VFO select: FR0; (0=VFO A, 1=VFO B)
    VfoSelect(Option<u8>),
    /// Split mode: FT0; or FT1;
    Split(Option<bool>),
    /// Power on/off: PS0; or PS1;
    Power(Option<bool>),
    /// Auto-information mode: AI0; (off) or AI2; (on) or AI; (query)
    AutoInfo(Option<bool>),
    /// Control band (which VFO has front panel control): CB; (query), CB0; or CB1;
    ControlBand(Option<u8>),
    /// Transmit band (which VFO is selected for TX): TB; (query), TB0; or TB1;
    TransmitBand(Option<u8>),
    /// Unknown/unrecognized command
    Unknown(String),
}

/// Parsed IF (information) response data
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KenwoodInfo {
    /// Current frequency in Hz
    pub frequency_hz: u64,
    /// RIT/XIT offset
    pub rit_offset: i16,
    /// RIT enabled
    pub rit_on: bool,
    /// XIT enabled
    pub xit_on: bool,
    /// Memory channel
    pub memory_channel: u8,
    /// TX enabled (PTT)
    pub tx: bool,
    /// Operating mode
    pub mode: u8,
    /// VFO (0=A, 1=B)
    pub vfo: u8,
    /// Scan status
    pub scan: bool,
    /// Split operation
    pub split: bool,
    /// CTCSS tone
    pub tone: u8,
}

/// Streaming Kenwood protocol codec
pub struct KenwoodCodec {
    buffer: Vec<u8>,
}

impl KenwoodCodec {
    /// Create a new Kenwood codec
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(64),
        }
    }

    /// Parse a complete command string (without terminator)
    fn parse_command(cmd: &str) -> Result<KenwoodCommand, ParseError> {
        if cmd.len() < 2 {
            return Err(ParseError::InvalidFrame("command too short".into()));
        }

        let prefix = &cmd[..2];
        let params = &cmd[2..];

        match prefix {
            "FA" => {
                if params.is_empty() {
                    Ok(KenwoodCommand::FrequencyA(None))
                } else {
                    let freq = params
                        .parse::<u64>()
                        .map_err(|_| ParseError::InvalidFrequency(params.into()))?;
                    Ok(KenwoodCommand::FrequencyA(Some(freq)))
                }
            }
            "FB" => {
                if params.is_empty() {
                    Ok(KenwoodCommand::FrequencyB(None))
                } else {
                    let freq = params
                        .parse::<u64>()
                        .map_err(|_| ParseError::InvalidFrequency(params.into()))?;
                    Ok(KenwoodCommand::FrequencyB(Some(freq)))
                }
            }
            "MD" => {
                if params.is_empty() {
                    Ok(KenwoodCommand::Mode(None))
                } else {
                    let mode = params
                        .parse::<u8>()
                        .map_err(|_| ParseError::InvalidMode(params.into()))?;
                    Ok(KenwoodCommand::Mode(Some(mode)))
                }
            }
            "TX" => {
                if params.is_empty() {
                    Ok(KenwoodCommand::Transmit(Some(true)))
                } else {
                    let tx = params != "0";
                    Ok(KenwoodCommand::Transmit(Some(tx)))
                }
            }
            "RX" => Ok(KenwoodCommand::Receive),
            "ID" => {
                if params.is_empty() {
                    Ok(KenwoodCommand::Id(None))
                } else {
                    Ok(KenwoodCommand::Id(Some(params.to_string())))
                }
            }
            "IF" => {
                if params.is_empty() {
                    Ok(KenwoodCommand::Info(None))
                } else {
                    let info = Self::parse_info(params)?;
                    Ok(KenwoodCommand::Info(Some(info)))
                }
            }
            "FR" => {
                if params.is_empty() {
                    Ok(KenwoodCommand::VfoSelect(None))
                } else {
                    let vfo = params
                        .parse::<u8>()
                        .map_err(|_| ParseError::InvalidFrame("invalid VFO".into()))?;
                    Ok(KenwoodCommand::VfoSelect(Some(vfo)))
                }
            }
            "FT" => {
                if params.is_empty() {
                    Ok(KenwoodCommand::Split(None))
                } else {
                    let split = params != "0";
                    Ok(KenwoodCommand::Split(Some(split)))
                }
            }
            "PS" => {
                if params.is_empty() {
                    Ok(KenwoodCommand::Power(None))
                } else {
                    let on = params != "0";
                    Ok(KenwoodCommand::Power(Some(on)))
                }
            }
            "AI" => {
                if params.is_empty() {
                    Ok(KenwoodCommand::AutoInfo(None))
                } else {
                    let enabled = params != "0";
                    Ok(KenwoodCommand::AutoInfo(Some(enabled)))
                }
            }
            "CB" => {
                if params.is_empty() {
                    Ok(KenwoodCommand::ControlBand(None))
                } else {
                    let band = params
                        .parse::<u8>()
                        .map_err(|_| ParseError::InvalidFrame("invalid control band".into()))?;
                    Ok(KenwoodCommand::ControlBand(Some(band)))
                }
            }
            "TB" => {
                if params.is_empty() {
                    Ok(KenwoodCommand::TransmitBand(None))
                } else {
                    let band = params
                        .parse::<u8>()
                        .map_err(|_| ParseError::InvalidFrame("invalid transmit band".into()))?;
                    Ok(KenwoodCommand::TransmitBand(Some(band)))
                }
            }
            _ => Ok(KenwoodCommand::Unknown(cmd.to_string())),
        }
    }

    /// Parse IF response parameters
    fn parse_info(params: &str) -> Result<KenwoodInfo, ParseError> {
        // IF response format (TS-2000 style, 37 chars):
        // IFaaaaaaaaaaaabbbbbrrrrrtttttvvmmfsct
        // Where:
        // - aaaaaaaaaaa: 11-digit frequency
        // - bbbbb: 5-digit step size (we skip)
        // - rrrrr: 5-digit RIT/XIT offset
        // - t: RIT on/off
        // - t: XIT on/off
        // - v: Memory channel (2 digits)
        // - mm: TX status
        // - f: Mode
        // - s: VFO
        // - c: Scan status
        // - t: Split/CTCSS

        if params.len() < 33 {
            return Err(ParseError::InvalidFrame(format!(
                "IF response too short: {} chars",
                params.len()
            )));
        }

        let frequency_hz = params[0..11]
            .parse::<u64>()
            .map_err(|_| ParseError::InvalidFrequency(params[0..11].into()))?;

        let rit_offset = params[16..21].parse::<i16>().unwrap_or(0);

        let rit_on = params.chars().nth(21) == Some('1');
        let xit_on = params.chars().nth(22) == Some('1');

        let memory_channel = params[23..25].parse::<u8>().unwrap_or(0);
        let tx = params.chars().nth(27) != Some('0');
        let mode = params[28..29].parse::<u8>().unwrap_or(0);
        let vfo = params[29..30].parse::<u8>().unwrap_or(0);
        let scan = params.chars().nth(30) == Some('1');
        let split = params.chars().nth(31) == Some('1');
        let tone = params[32..].parse::<u8>().unwrap_or(0);

        Ok(KenwoodInfo {
            frequency_hz,
            rit_offset,
            rit_on,
            xit_on,
            memory_channel,
            tx,
            mode,
            vfo,
            scan,
            split,
            tone,
        })
    }
}

impl Default for KenwoodCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl ProtocolCodec for KenwoodCodec {
    type Command = KenwoodCommand;

    fn push_bytes(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);

        // Prevent buffer overflow
        if self.buffer.len() > MAX_COMMAND_LEN * 4 {
            // Keep only the last portion
            let start = self.buffer.len() - MAX_COMMAND_LEN;
            self.buffer = self.buffer[start..].to_vec();
        }
    }

    fn next_command(&mut self) -> Option<Self::Command> {
        self.next_command_with_bytes().map(|(cmd, _)| cmd)
    }

    fn next_command_with_bytes(&mut self) -> Option<(Self::Command, Vec<u8>)> {
        // Find terminator
        let term_pos = self.buffer.iter().position(|&b| b == b';')?;

        // Extract command bytes
        let cmd_bytes: Vec<u8> = self.buffer.drain(..=term_pos).collect();

        // Parse as ASCII (strip terminator)
        let cmd_str = String::from_utf8_lossy(&cmd_bytes[..cmd_bytes.len() - 1]);

        let cmd = match Self::parse_command(&cmd_str) {
            Ok(cmd) => cmd,
            Err(e) => {
                tracing::warn!("Failed to parse Kenwood command: {}", e);
                KenwoodCommand::Unknown(cmd_str.into_owned())
            }
        };

        Some((cmd, cmd_bytes))
    }

    fn clear(&mut self) {
        self.buffer.clear();
    }
}

impl ToRadioCommand for KenwoodCommand {
    fn to_radio_command(&self) -> RadioCommand {
        match self {
            KenwoodCommand::FrequencyA(Some(hz)) => RadioCommand::SetFrequency { hz: *hz },
            KenwoodCommand::FrequencyA(None) => RadioCommand::GetFrequency,
            KenwoodCommand::FrequencyB(Some(hz)) => RadioCommand::SetFrequency { hz: *hz },
            KenwoodCommand::FrequencyB(None) => RadioCommand::GetFrequency,
            KenwoodCommand::Mode(Some(m)) => RadioCommand::SetMode {
                mode: kenwood_mode_to_operating_mode(*m),
            },
            KenwoodCommand::Mode(None) => RadioCommand::GetMode,
            KenwoodCommand::Transmit(Some(tx)) => RadioCommand::SetPtt { active: *tx },
            KenwoodCommand::Transmit(None) => RadioCommand::GetPtt,
            KenwoodCommand::Receive => RadioCommand::SetPtt { active: false },
            KenwoodCommand::Id(Some(id)) => RadioCommand::IdReport { id: id.clone() },
            KenwoodCommand::Id(None) => RadioCommand::GetId,
            KenwoodCommand::Info(Some(info)) => RadioCommand::StatusReport {
                frequency_hz: Some(info.frequency_hz),
                mode: Some(kenwood_mode_to_operating_mode(info.mode)),
                ptt: Some(info.tx),
                vfo: Some(if info.vfo == 0 { Vfo::A } else { Vfo::B }),
            },
            KenwoodCommand::Info(None) => RadioCommand::GetStatus,
            KenwoodCommand::VfoSelect(Some(v)) => RadioCommand::SetVfo {
                vfo: if *v == 0 { Vfo::A } else { Vfo::B },
            },
            KenwoodCommand::VfoSelect(None) => RadioCommand::GetVfo,
            KenwoodCommand::Split(Some(s)) => RadioCommand::SetVfo {
                vfo: if *s { Vfo::Split } else { Vfo::A },
            },
            KenwoodCommand::Split(None) => RadioCommand::GetVfo,
            KenwoodCommand::Power(Some(on)) => RadioCommand::SetPower { on: *on },
            KenwoodCommand::Power(None) => RadioCommand::Unknown { data: vec![] },
            KenwoodCommand::AutoInfo(Some(enabled)) => {
                RadioCommand::EnableAutoInfo { enabled: *enabled }
            }
            KenwoodCommand::AutoInfo(None) => RadioCommand::GetAutoInfo,
            KenwoodCommand::ControlBand(Some(band)) => {
                RadioCommand::ControlBandReport { band: *band }
            }
            KenwoodCommand::ControlBand(None) => RadioCommand::GetControlBand,
            KenwoodCommand::TransmitBand(Some(band)) => {
                RadioCommand::TransmitBandReport { band: *band }
            }
            KenwoodCommand::TransmitBand(None) => RadioCommand::GetTransmitBand,
            KenwoodCommand::Unknown(s) => RadioCommand::Unknown {
                data: s.as_bytes().to_vec(),
            },
        }
    }
}

impl FromRadioCommand for KenwoodCommand {
    fn from_radio_command(cmd: &RadioCommand) -> Option<Self> {
        match cmd {
            RadioCommand::SetFrequency { hz } => Some(KenwoodCommand::FrequencyA(Some(*hz))),
            RadioCommand::GetFrequency => Some(KenwoodCommand::FrequencyA(None)),
            RadioCommand::FrequencyReport { hz } => Some(KenwoodCommand::FrequencyA(Some(*hz))),
            RadioCommand::SetMode { mode } => {
                Some(KenwoodCommand::Mode(Some(operating_mode_to_kenwood(*mode))))
            }
            RadioCommand::GetMode => Some(KenwoodCommand::Mode(None)),
            RadioCommand::ModeReport { mode } => {
                Some(KenwoodCommand::Mode(Some(operating_mode_to_kenwood(*mode))))
            }
            RadioCommand::SetPtt { active: true } => Some(KenwoodCommand::Transmit(Some(true))),
            RadioCommand::SetPtt { active: false } => Some(KenwoodCommand::Receive),
            RadioCommand::GetPtt => Some(KenwoodCommand::Transmit(None)),
            RadioCommand::PttReport { active } => Some(KenwoodCommand::Transmit(Some(*active))),
            RadioCommand::SetVfo { vfo } => match vfo {
                Vfo::A => Some(KenwoodCommand::VfoSelect(Some(0))),
                Vfo::B => Some(KenwoodCommand::VfoSelect(Some(1))),
                Vfo::Split => Some(KenwoodCommand::Split(Some(true))),
                Vfo::Memory => Some(KenwoodCommand::VfoSelect(Some(2))),
            },
            RadioCommand::GetVfo => Some(KenwoodCommand::VfoSelect(None)),
            RadioCommand::GetId => Some(KenwoodCommand::Id(None)),
            RadioCommand::IdReport { id } => Some(KenwoodCommand::Id(Some(id.clone()))),
            RadioCommand::GetStatus => Some(KenwoodCommand::Info(None)),
            RadioCommand::SetPower { on } => Some(KenwoodCommand::Power(Some(*on))),
            RadioCommand::EnableAutoInfo { enabled } => {
                Some(KenwoodCommand::AutoInfo(Some(*enabled)))
            }
            RadioCommand::GetAutoInfo => Some(KenwoodCommand::AutoInfo(None)),
            RadioCommand::AutoInfoReport { enabled } => {
                Some(KenwoodCommand::AutoInfo(Some(*enabled)))
            }
            RadioCommand::GetControlBand => Some(KenwoodCommand::ControlBand(None)),
            RadioCommand::ControlBandReport { band } => {
                Some(KenwoodCommand::ControlBand(Some(*band)))
            }
            RadioCommand::GetTransmitBand => Some(KenwoodCommand::TransmitBand(None)),
            RadioCommand::TransmitBandReport { band } => {
                Some(KenwoodCommand::TransmitBand(Some(*band)))
            }
            _ => None,
        }
    }
}

impl EncodeCommand for KenwoodCommand {
    fn encode(&self) -> Vec<u8> {
        let cmd = match self {
            KenwoodCommand::FrequencyA(Some(hz)) => format!("FA{:011}", hz),
            KenwoodCommand::FrequencyA(None) => "FA".to_string(),
            KenwoodCommand::FrequencyB(Some(hz)) => format!("FB{:011}", hz),
            KenwoodCommand::FrequencyB(None) => "FB".to_string(),
            KenwoodCommand::Mode(Some(m)) => format!("MD{}", m),
            KenwoodCommand::Mode(None) => "MD".to_string(),
            KenwoodCommand::Transmit(Some(true)) => "TX1".to_string(),
            KenwoodCommand::Transmit(Some(false)) => "TX0".to_string(),
            KenwoodCommand::Transmit(None) => "TX".to_string(),
            KenwoodCommand::Receive => "RX".to_string(),
            KenwoodCommand::Id(Some(id)) => format!("ID{}", id),
            KenwoodCommand::Id(None) => "ID".to_string(),
            KenwoodCommand::Info(_) => "IF".to_string(),
            KenwoodCommand::VfoSelect(Some(v)) => format!("FR{}", v),
            KenwoodCommand::VfoSelect(None) => "FR".to_string(),
            KenwoodCommand::Split(Some(s)) => format!("FT{}", if *s { 1 } else { 0 }),
            KenwoodCommand::Split(None) => "FT".to_string(),
            KenwoodCommand::Power(Some(on)) => format!("PS{}", if *on { 1 } else { 0 }),
            KenwoodCommand::Power(None) => "PS".to_string(),
            KenwoodCommand::AutoInfo(Some(enabled)) => {
                format!("AI{}", if *enabled { 2 } else { 0 })
            }
            KenwoodCommand::AutoInfo(None) => "AI".to_string(),
            KenwoodCommand::ControlBand(Some(band)) => format!("CB{}", band),
            KenwoodCommand::ControlBand(None) => "CB".to_string(),
            KenwoodCommand::TransmitBand(Some(band)) => format!("TB{}", band),
            KenwoodCommand::TransmitBand(None) => "TB".to_string(),
            KenwoodCommand::Unknown(s) => s.clone(),
        };
        format!("{};", cmd).into_bytes()
    }
}

/// Convert Kenwood mode number to OperatingMode
fn kenwood_mode_to_operating_mode(mode: u8) -> OperatingMode {
    match mode {
        1 => OperatingMode::Lsb,
        2 => OperatingMode::Usb,
        3 => OperatingMode::Cw,
        4 => OperatingMode::Fm,
        5 => OperatingMode::Am,
        6 => OperatingMode::Rtty,
        7 => OperatingMode::CwR,
        8 => OperatingMode::DataL,
        9 => OperatingMode::RttyR,
        10 => OperatingMode::DataU,
        _ => OperatingMode::Usb,
    }
}

/// Convert OperatingMode to Kenwood mode number
fn operating_mode_to_kenwood(mode: OperatingMode) -> u8 {
    match mode {
        OperatingMode::Lsb => 1,
        OperatingMode::Usb => 2,
        OperatingMode::Cw => 3,
        OperatingMode::Fm => 4,
        OperatingMode::FmN => 4,
        OperatingMode::Am => 5,
        OperatingMode::Rtty => 6,
        OperatingMode::CwR => 7,
        OperatingMode::DataL | OperatingMode::DigL | OperatingMode::Dig => 8,
        OperatingMode::RttyR => 9,
        OperatingMode::DataU | OperatingMode::DigU | OperatingMode::Data | OperatingMode::Pkt => 10,
    }
}

/// Generate a probe command to detect Kenwood radios
pub fn probe_command() -> Vec<u8> {
    b"ID;".to_vec()
}

/// Check if a response looks like a valid Kenwood ID response
pub fn is_valid_id_response(data: &[u8]) -> bool {
    // Valid responses: ID019; ID021; etc.
    if data.len() >= 5 && data.starts_with(b"ID") && data.ends_with(b";") {
        let id_part = &data[2..data.len() - 1];
        id_part.iter().all(|b| b.is_ascii_digit())
    } else {
        false
    }
}

crate::impl_radio_codec!(KenwoodCodec);

#[cfg(test)]
mod tests {
    use super::{KenwoodCodec, KenwoodCommand};
    use crate::{EncodeCommand, FromRadioCommand, ProtocolCodec, RadioCommand, ToRadioCommand};

    #[test]
    fn test_parse_frequency() {
        let mut codec = KenwoodCodec::new();
        codec.push_bytes(b"FA00014250000;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, KenwoodCommand::FrequencyA(Some(14_250_000)));
    }

    #[test]
    fn test_parse_mode() {
        let mut codec = KenwoodCodec::new();
        codec.push_bytes(b"MD2;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, KenwoodCommand::Mode(Some(2)));
    }

    #[test]
    fn test_parse_id_query() {
        let mut codec = KenwoodCodec::new();
        codec.push_bytes(b"ID;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, KenwoodCommand::Id(None));
    }

    #[test]
    fn test_encode_frequency() {
        let cmd = KenwoodCommand::FrequencyA(Some(14_250_000));
        let encoded = cmd.encode();
        assert_eq!(encoded, b"FA00014250000;");
    }

    #[test]
    fn test_streaming_parse() {
        let mut codec = KenwoodCodec::new();

        // Push partial data
        codec.push_bytes(b"FA000142");
        assert!(codec.next_command().is_none());

        // Push rest
        codec.push_bytes(b"50000;");
        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, KenwoodCommand::FrequencyA(Some(14_250_000)));
    }

    #[test]
    fn test_multiple_commands() {
        let mut codec = KenwoodCodec::new();
        codec.push_bytes(b"FA00014250000;MD2;TX1;");

        assert_eq!(
            codec.next_command(),
            Some(KenwoodCommand::FrequencyA(Some(14_250_000)))
        );
        assert_eq!(codec.next_command(), Some(KenwoodCommand::Mode(Some(2))));
        assert_eq!(
            codec.next_command(),
            Some(KenwoodCommand::Transmit(Some(true)))
        );
        assert!(codec.next_command().is_none());
    }

    #[test]
    fn test_to_radio_command() {
        let cmd = KenwoodCommand::FrequencyA(Some(7_074_000));
        let radio_cmd = cmd.to_radio_command();
        assert_eq!(radio_cmd, RadioCommand::SetFrequency { hz: 7_074_000 });
    }

    #[test]
    fn test_from_radio_command() {
        let radio_cmd = RadioCommand::SetFrequency { hz: 14_250_000 };
        let cmd = KenwoodCommand::from_radio_command(&radio_cmd).unwrap();
        assert_eq!(cmd, KenwoodCommand::FrequencyA(Some(14_250_000)));
    }

    #[test]
    fn test_parse_auto_info_query() {
        let mut codec = KenwoodCodec::new();
        codec.push_bytes(b"AI;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, KenwoodCommand::AutoInfo(None));
        assert_eq!(cmd.to_radio_command(), RadioCommand::GetAutoInfo);
    }

    #[test]
    fn test_parse_auto_info_enable() {
        let mut codec = KenwoodCodec::new();
        codec.push_bytes(b"AI1;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, KenwoodCommand::AutoInfo(Some(true)));
        assert_eq!(
            cmd.to_radio_command(),
            RadioCommand::EnableAutoInfo { enabled: true }
        );
    }

    #[test]
    fn test_parse_auto_info_disable() {
        let mut codec = KenwoodCodec::new();
        codec.push_bytes(b"AI0;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, KenwoodCommand::AutoInfo(Some(false)));
        assert_eq!(
            cmd.to_radio_command(),
            RadioCommand::EnableAutoInfo { enabled: false }
        );
    }

    #[test]
    fn test_encode_auto_info() {
        assert_eq!(KenwoodCommand::AutoInfo(None).encode(), b"AI;");
        assert_eq!(KenwoodCommand::AutoInfo(Some(true)).encode(), b"AI2;");
        assert_eq!(KenwoodCommand::AutoInfo(Some(false)).encode(), b"AI0;");
    }

    #[test]
    fn test_from_radio_command_auto_info() {
        let cmd =
            KenwoodCommand::from_radio_command(&RadioCommand::EnableAutoInfo { enabled: true })
                .unwrap();
        assert_eq!(cmd, KenwoodCommand::AutoInfo(Some(true)));

        let cmd = KenwoodCommand::from_radio_command(&RadioCommand::GetAutoInfo).unwrap();
        assert_eq!(cmd, KenwoodCommand::AutoInfo(None));
    }

    #[test]
    fn test_parse_control_band_query() {
        let mut codec = KenwoodCodec::new();
        codec.push_bytes(b"CB;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, KenwoodCommand::ControlBand(None));
        assert_eq!(cmd.to_radio_command(), RadioCommand::GetControlBand);
    }

    #[test]
    fn test_parse_control_band_set() {
        let mut codec = KenwoodCodec::new();
        codec.push_bytes(b"CB0;");
        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, KenwoodCommand::ControlBand(Some(0)));
        assert_eq!(
            cmd.to_radio_command(),
            RadioCommand::ControlBandReport { band: 0 }
        );

        let mut codec = KenwoodCodec::new();
        codec.push_bytes(b"CB1;");
        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, KenwoodCommand::ControlBand(Some(1)));
        assert_eq!(
            cmd.to_radio_command(),
            RadioCommand::ControlBandReport { band: 1 }
        );
    }

    #[test]
    fn test_encode_control_band() {
        assert_eq!(KenwoodCommand::ControlBand(None).encode(), b"CB;");
        assert_eq!(KenwoodCommand::ControlBand(Some(0)).encode(), b"CB0;");
        assert_eq!(KenwoodCommand::ControlBand(Some(1)).encode(), b"CB1;");
    }

    #[test]
    fn test_parse_transmit_band_query() {
        let mut codec = KenwoodCodec::new();
        codec.push_bytes(b"TB;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, KenwoodCommand::TransmitBand(None));
        assert_eq!(cmd.to_radio_command(), RadioCommand::GetTransmitBand);
    }

    #[test]
    fn test_parse_transmit_band_set() {
        let mut codec = KenwoodCodec::new();
        codec.push_bytes(b"TB0;");
        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, KenwoodCommand::TransmitBand(Some(0)));
        assert_eq!(
            cmd.to_radio_command(),
            RadioCommand::TransmitBandReport { band: 0 }
        );

        let mut codec = KenwoodCodec::new();
        codec.push_bytes(b"TB1;");
        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, KenwoodCommand::TransmitBand(Some(1)));
        assert_eq!(
            cmd.to_radio_command(),
            RadioCommand::TransmitBandReport { band: 1 }
        );
    }

    #[test]
    fn test_encode_transmit_band() {
        assert_eq!(KenwoodCommand::TransmitBand(None).encode(), b"TB;");
        assert_eq!(KenwoodCommand::TransmitBand(Some(0)).encode(), b"TB0;");
        assert_eq!(KenwoodCommand::TransmitBand(Some(1)).encode(), b"TB1;");
    }

    #[test]
    fn test_from_radio_command_control_band() {
        let cmd = KenwoodCommand::from_radio_command(&RadioCommand::GetControlBand).unwrap();
        assert_eq!(cmd, KenwoodCommand::ControlBand(None));

        let cmd = KenwoodCommand::from_radio_command(&RadioCommand::ControlBandReport { band: 1 })
            .unwrap();
        assert_eq!(cmd, KenwoodCommand::ControlBand(Some(1)));
    }

    #[test]
    fn test_from_radio_command_transmit_band() {
        let cmd = KenwoodCommand::from_radio_command(&RadioCommand::GetTransmitBand).unwrap();
        assert_eq!(cmd, KenwoodCommand::TransmitBand(None));

        let cmd = KenwoodCommand::from_radio_command(&RadioCommand::TransmitBandReport { band: 1 })
            .unwrap();
        assert_eq!(cmd, KenwoodCommand::TransmitBand(Some(1)));
    }
}
