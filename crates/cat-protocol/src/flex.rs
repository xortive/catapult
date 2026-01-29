//! FlexRadio SmartSDR CAT Protocol Implementation
//!
//! FlexRadio uses a Kenwood-compatible ASCII protocol with extensions.
//! Commands are semicolon-terminated and support both:
//! - Standard Kenwood 2-letter commands (FA, FB, MD, TX, RX, ID, IF)
//! - FlexRadio extended 4-letter ZZ commands (ZZFA, ZZFB, ZZMD, ZZTX, ZZIF)
//!
//! # Architecture
//! This implementation uses composition with `KenwoodCodec` to avoid duplicating
//! parsing logic. Standard Kenwood commands are delegated to the inner codec,
//! while FlexRadio-specific ZZ commands are handled here.
//!
//! # Protocol Differences by Generation
//!
//! All FlexRadio generations use the same CAT protocol via SmartSDR CAT.
//! The protocol is accessed through virtual serial ports created by SmartSDR.
//!
//! # Format
//! - Commands: `XXppppp;` (Kenwood) or `ZZXXppppp;` (FlexRadio extended)
//! - Responses: Same format as commands
//! - Terminator: `;` (0x3B)
//! - Default: 9600 baud, 8N1
//!
//! # Extended Commands
//! - `ZZFA` - VFO A frequency (11-digit Hz)
//! - `ZZFB` - VFO B frequency (11-digit Hz)
//! - `ZZMD` - Mode (2-digit code)
//! - `ZZTX` - Transmit control
//! - `ZZIF` - Status information
//!
//! # Model Identification
//! FlexRadio responds to ID; with model-specific codes:
//! - 904 = FLEX-6700
//! - 905 = FLEX-6500
//! - 906 = FLEX-6700R
//! - 907 = FLEX-6300
//! - 908 = FLEX-6400
//! - 909 = FLEX-6600
//! - 910 = FLEX-6400M
//! - 911 = FLEX-6600M
//! - 912 = FLEX-8400
//! - 913 = FLEX-8600

use crate::command::{OperatingMode, RadioCommand, Vfo};
use crate::kenwood::{KenwoodCodec, KenwoodCommand};
use crate::{EncodeCommand, FromRadioCommand, ProtocolCodec, ToRadioCommand};

/// FlexRadio protocol command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlexCommand {
    /// Base Kenwood command (for compatible commands like FA, FB, TX, RX, ID, FR, FT, PS)
    Kenwood(KenwoodCommand),
    /// Mode with FlexRadio's extended mode set: ZZMD01; or MD1;
    Mode(Option<FlexMode>),
    /// FlexRadio extended status: ZZIF...;
    Info(Option<FlexInfo>),
    /// Audio gain: ZZAG000; (0-100)
    AudioGain(Option<u8>),
    /// RF power level: ZZPC000; (0-100)
    RfPower(Option<u8>),
    /// S-meter read: ZZSM;
    SMeter(Option<i16>),
    /// AGC mode: ZZGT0;
    AgcMode(Option<u8>),
    /// Noise reduction: ZZNR0;
    NoiseReduction(Option<bool>),
    /// Auto-information mode: AI0; (off) or AI1; (on) or AI; (query)
    AutoInfo(Option<bool>),
    /// Unknown/unrecognized command (preserves original)
    Unknown(String),
}

/// FlexRadio operating mode (ZZMD values)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexMode {
    /// LSB (00)
    Lsb,
    /// USB (01)
    Usb,
    /// DSB (02)
    Dsb,
    /// CW Lower (03)
    CwL,
    /// CW Upper (04)
    CwU,
    /// FM (05)
    Fm,
    /// AM (06)
    Am,
    /// Digital Upper (07)
    DigU,
    /// Spectrum (08)
    Spec,
    /// Digital Lower (09)
    DigL,
    /// Synchronous AM (10)
    Sam,
    /// Narrow FM (11)
    Nfm,
    /// Digital FM (12)
    Dfm,
    /// FreeDV (20)
    Fdv,
    /// RTTY (30)
    Rtty,
    /// D-STAR (40)
    Dstar,
}

impl FlexMode {
    /// Convert from ZZMD parameter value
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Lsb),
            1 => Some(Self::Usb),
            2 => Some(Self::Dsb),
            3 => Some(Self::CwL),
            4 => Some(Self::CwU),
            5 => Some(Self::Fm),
            6 => Some(Self::Am),
            7 => Some(Self::DigU),
            8 => Some(Self::Spec),
            9 => Some(Self::DigL),
            10 => Some(Self::Sam),
            11 => Some(Self::Nfm),
            12 => Some(Self::Dfm),
            20 => Some(Self::Fdv),
            30 => Some(Self::Rtty),
            40 => Some(Self::Dstar),
            _ => None,
        }
    }

    /// Convert from Kenwood mode number (1-10)
    pub fn from_kenwood_mode(mode: u8) -> Self {
        match mode {
            1 => Self::Lsb,
            2 => Self::Usb,
            3 => Self::CwU,
            4 => Self::Fm,
            5 => Self::Am,
            6 => Self::Rtty,
            7 => Self::CwL,
            9 => Self::Rtty,
            _ => Self::Usb,
        }
    }

    /// Convert to ZZMD parameter value
    pub fn to_code(self) -> u8 {
        match self {
            Self::Lsb => 0,
            Self::Usb => 1,
            Self::Dsb => 2,
            Self::CwL => 3,
            Self::CwU => 4,
            Self::Fm => 5,
            Self::Am => 6,
            Self::DigU => 7,
            Self::Spec => 8,
            Self::DigL => 9,
            Self::Sam => 10,
            Self::Nfm => 11,
            Self::Dfm => 12,
            Self::Fdv => 20,
            Self::Rtty => 30,
            Self::Dstar => 40,
        }
    }

    /// Convert to standard OperatingMode
    pub fn to_operating_mode(self) -> OperatingMode {
        match self {
            Self::Lsb => OperatingMode::Lsb,
            Self::Usb => OperatingMode::Usb,
            Self::Dsb | Self::Am | Self::Sam => OperatingMode::Am,
            Self::CwL => OperatingMode::Cw,
            Self::CwU => OperatingMode::CwR,
            Self::Fm | Self::Dfm => OperatingMode::Fm,
            Self::Nfm => OperatingMode::FmN,
            Self::DigU | Self::Fdv | Self::Dstar => OperatingMode::DigU,
            Self::DigL => OperatingMode::DigL,
            Self::Spec => OperatingMode::Data,
            Self::Rtty => OperatingMode::Rtty,
        }
    }

    /// Convert from standard OperatingMode
    pub fn from_operating_mode(mode: OperatingMode) -> Self {
        match mode {
            OperatingMode::Lsb => Self::Lsb,
            OperatingMode::Usb => Self::Usb,
            OperatingMode::Cw => Self::CwL,
            OperatingMode::CwR => Self::CwU,
            OperatingMode::Am => Self::Am,
            OperatingMode::Fm => Self::Fm,
            OperatingMode::FmN => Self::Nfm,
            OperatingMode::Dig
            | OperatingMode::DigU
            | OperatingMode::DataU
            | OperatingMode::Data
            | OperatingMode::Pkt => Self::DigU,
            OperatingMode::DigL | OperatingMode::DataL => Self::DigL,
            OperatingMode::Rtty => Self::Rtty,
            OperatingMode::RttyR => Self::Rtty,
        }
    }
}

/// Parsed ZZIF response data
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlexInfo {
    /// Current frequency in Hz
    pub frequency_hz: u64,
    /// Frequency step size
    pub step_size: u32,
    /// RIT/XIT offset in Hz
    pub rit_offset: i32,
    /// RIT enabled
    pub rit_on: bool,
    /// XIT enabled
    pub xit_on: bool,
    /// TX enabled (PTT)
    pub tx: bool,
    /// Operating mode
    pub mode: FlexMode,
    /// VFO (0=A, 1=B)
    pub vfo: u8,
    /// Split operation
    pub split: bool,
}

/// Streaming FlexRadio protocol codec
///
/// Uses composition with `KenwoodCodec` to handle standard Kenwood commands,
/// while parsing FlexRadio-specific ZZ commands locally.
pub struct FlexCodec {
    inner: KenwoodCodec,
}

impl FlexCodec {
    /// Create a new FlexRadio codec
    pub fn new() -> Self {
        Self {
            inner: KenwoodCodec::new(),
        }
    }

    /// Parse FlexRadio ZZ-prefixed commands
    fn parse_zz_command(cmd_str: &str) -> Option<FlexCommand> {
        if cmd_str.len() < 4 || !cmd_str.starts_with("ZZ") {
            return None;
        }

        let prefix = &cmd_str[..4];
        let params = &cmd_str[4..];

        match prefix {
            "ZZFA" => Some(Self::parse_frequency_a(params)),
            "ZZFB" => Some(Self::parse_frequency_b(params)),
            "ZZMD" | "ZZME" => Some(Self::parse_mode_flex(params)),
            "ZZTX" => Some(Self::parse_transmit(params)),
            "ZZIF" => Some(Self::parse_info(params)),
            "ZZFR" => Some(Self::parse_vfo_select(params)),
            "ZZSW" => Some(Self::parse_split(params)),
            "ZZAG" => Some(FlexCommand::AudioGain(params.parse().ok())),
            "ZZPC" => Some(FlexCommand::RfPower(params.parse().ok())),
            "ZZSM" => Some(FlexCommand::SMeter(params.parse().ok())),
            "ZZGT" => Some(FlexCommand::AgcMode(params.parse().ok())),
            "ZZNR" => Some(FlexCommand::NoiseReduction(if params.is_empty() {
                None
            } else {
                Some(params != "0")
            })),
            "ZZAI" => Some(FlexCommand::AutoInfo(if params.is_empty() {
                None
            } else {
                Some(params != "0")
            })),
            _ => None,
        }
    }

    /// Parse ZZ frequency commands - returns FlexCommand wrapping Kenwood
    fn parse_frequency_a(params: &str) -> FlexCommand {
        if params.is_empty() {
            FlexCommand::Kenwood(KenwoodCommand::FrequencyA(None))
        } else {
            let freq = params.parse::<u64>().ok();
            FlexCommand::Kenwood(KenwoodCommand::FrequencyA(freq))
        }
    }

    fn parse_frequency_b(params: &str) -> FlexCommand {
        if params.is_empty() {
            FlexCommand::Kenwood(KenwoodCommand::FrequencyB(None))
        } else {
            let freq = params.parse::<u64>().ok();
            FlexCommand::Kenwood(KenwoodCommand::FrequencyB(freq))
        }
    }

    fn parse_mode_flex(params: &str) -> FlexCommand {
        if params.is_empty() {
            FlexCommand::Mode(None)
        } else if let Ok(code) = params.parse::<u8>() {
            FlexCommand::Mode(FlexMode::from_code(code))
        } else {
            FlexCommand::Mode(None)
        }
    }

    fn parse_transmit(params: &str) -> FlexCommand {
        let tx = if params.is_empty() {
            Some(true)
        } else {
            Some(params != "0")
        };
        FlexCommand::Kenwood(KenwoodCommand::Transmit(tx))
    }

    fn parse_info(params: &str) -> FlexCommand {
        if params.is_empty() {
            FlexCommand::Info(None)
        } else if let Some(info) = Self::try_parse_flex_info(params) {
            FlexCommand::Info(Some(info))
        } else {
            FlexCommand::Info(None)
        }
    }

    fn try_parse_flex_info(params: &str) -> Option<FlexInfo> {
        // ZZIF format: 11-digit freq, 4-digit step, 6-digit RIT, RIT on, XIT on, TX, mode, VFO, split
        if params.len() < 28 {
            return None;
        }

        let frequency_hz = params[0..11].parse::<u64>().ok()?;
        let step_size = params[11..15].parse::<u32>().unwrap_or(1);
        let rit_offset = params[15..21].parse::<i32>().unwrap_or(0);
        let rit_on = params.chars().nth(21) == Some('1');
        let xit_on = params.chars().nth(22) == Some('1');
        let tx = params.chars().nth(23) != Some('0');
        let mode_code = params[24..26].parse::<u8>().unwrap_or(1);
        let mode = FlexMode::from_code(mode_code).unwrap_or(FlexMode::Usb);
        let vfo = params[26..27].parse::<u8>().unwrap_or(0);
        let split = params.chars().nth(27) == Some('1');

        Some(FlexInfo {
            frequency_hz,
            step_size,
            rit_offset,
            rit_on,
            xit_on,
            tx,
            mode,
            vfo,
            split,
        })
    }

    fn parse_vfo_select(params: &str) -> FlexCommand {
        let vfo = if params.is_empty() {
            None
        } else {
            params.parse().ok()
        };
        FlexCommand::Kenwood(KenwoodCommand::VfoSelect(vfo))
    }

    fn parse_split(params: &str) -> FlexCommand {
        let split = if params.is_empty() {
            None
        } else {
            Some(params != "0")
        };
        FlexCommand::Kenwood(KenwoodCommand::Split(split))
    }

    /// Convert a Kenwood command to FlexCommand, handling Mode specially
    fn convert_kenwood_command(kw: KenwoodCommand) -> FlexCommand {
        match kw {
            // Mode needs special handling for FlexMode conversion
            KenwoodCommand::Mode(Some(m)) => {
                FlexCommand::Mode(Some(FlexMode::from_kenwood_mode(m)))
            }
            KenwoodCommand::Mode(None) => FlexCommand::Mode(None),
            // AutoInfo is handled as Flex-specific for ZZ encoding
            KenwoodCommand::AutoInfo(enabled) => FlexCommand::AutoInfo(enabled),
            // Info uses FlexInfo structure
            KenwoodCommand::Info(Some(info)) => FlexCommand::Info(Some(FlexInfo {
                frequency_hz: info.frequency_hz,
                step_size: 1,
                rit_offset: info.rit_offset as i32,
                rit_on: info.rit_on,
                xit_on: info.xit_on,
                tx: info.tx,
                mode: FlexMode::from_kenwood_mode(info.mode),
                vfo: info.vfo,
                split: info.split,
            })),
            KenwoodCommand::Info(None) => FlexCommand::Info(None),
            // Unknown commands that might be Flex-specific
            KenwoodCommand::Unknown(s) => FlexCommand::Unknown(s),
            // All other Kenwood commands wrap directly
            other => FlexCommand::Kenwood(other),
        }
    }
}

impl Default for FlexCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl ProtocolCodec for FlexCodec {
    type Command = FlexCommand;

    fn push_bytes(&mut self, data: &[u8]) {
        self.inner.push_bytes(data);
    }

    fn next_command(&mut self) -> Option<Self::Command> {
        self.next_command_with_bytes().map(|(cmd, _)| cmd)
    }

    fn next_command_with_bytes(&mut self) -> Option<(Self::Command, Vec<u8>)> {
        // Get the next Kenwood command with its raw bytes
        let (kenwood_cmd, raw_bytes) = self.inner.next_command_with_bytes()?;

        // Check if it's an unknown command that might be FlexRadio-specific (ZZ prefix)
        if let KenwoodCommand::Unknown(ref s) = kenwood_cmd {
            if let Some(flex_cmd) = Self::parse_zz_command(s) {
                return Some((flex_cmd, raw_bytes));
            }
        }

        // Convert Kenwood command to FlexCommand
        Some((Self::convert_kenwood_command(kenwood_cmd), raw_bytes))
    }

    fn clear(&mut self) {
        self.inner.clear();
    }
}

impl ToRadioCommand for FlexCommand {
    fn to_radio_command(&self) -> RadioCommand {
        match self {
            FlexCommand::Kenwood(kw) => kw.to_radio_command(),
            FlexCommand::Mode(Some(m)) => RadioCommand::SetMode {
                mode: m.to_operating_mode(),
            },
            FlexCommand::Mode(None) => RadioCommand::GetMode,
            FlexCommand::Info(Some(info)) => RadioCommand::StatusReport {
                frequency_hz: Some(info.frequency_hz),
                mode: Some(info.mode.to_operating_mode()),
                ptt: Some(info.tx),
                vfo: Some(if info.vfo == 0 { Vfo::A } else { Vfo::B }),
            },
            FlexCommand::Info(None) => RadioCommand::GetStatus,
            FlexCommand::AudioGain(_)
            | FlexCommand::RfPower(_)
            | FlexCommand::SMeter(_)
            | FlexCommand::AgcMode(_)
            | FlexCommand::NoiseReduction(_) => RadioCommand::Unknown { data: vec![] },
            FlexCommand::AutoInfo(Some(enabled)) => {
                RadioCommand::EnableAutoInfo { enabled: *enabled }
            }
            FlexCommand::AutoInfo(None) => RadioCommand::GetAutoInfo,
            FlexCommand::Unknown(s) => RadioCommand::Unknown {
                data: s.as_bytes().to_vec(),
            },
        }
    }
}

impl FromRadioCommand for FlexCommand {
    fn from_radio_command(cmd: &RadioCommand) -> Option<Self> {
        match cmd {
            // Mode uses FlexMode
            RadioCommand::SetMode { mode } => Some(FlexCommand::Mode(Some(
                FlexMode::from_operating_mode(*mode),
            ))),
            RadioCommand::GetMode => Some(FlexCommand::Mode(None)),
            RadioCommand::ModeReport { mode } => Some(FlexCommand::Mode(Some(
                FlexMode::from_operating_mode(*mode),
            ))),
            // AutoInfo uses Flex-specific encoding
            RadioCommand::EnableAutoInfo { enabled } => Some(FlexCommand::AutoInfo(Some(*enabled))),
            RadioCommand::GetAutoInfo => Some(FlexCommand::AutoInfo(None)),
            RadioCommand::AutoInfoReport { enabled } => Some(FlexCommand::AutoInfo(Some(*enabled))),
            // Status uses FlexInfo
            RadioCommand::GetStatus => Some(FlexCommand::Info(None)),
            // Everything else delegates to Kenwood
            _ => KenwoodCommand::from_radio_command(cmd).map(FlexCommand::Kenwood),
        }
    }
}

impl EncodeCommand for FlexCommand {
    fn encode(&self) -> Vec<u8> {
        let cmd = match self {
            FlexCommand::Kenwood(kw) => {
                // For Flex output, use ZZ commands where available for better precision
                match kw {
                    KenwoodCommand::FrequencyA(Some(hz)) => format!("ZZFA{:011}", hz),
                    KenwoodCommand::FrequencyA(None) => "ZZFA".to_string(),
                    KenwoodCommand::FrequencyB(Some(hz)) => format!("ZZFB{:011}", hz),
                    KenwoodCommand::FrequencyB(None) => "ZZFB".to_string(),
                    KenwoodCommand::Transmit(Some(true)) => "ZZTX1".to_string(),
                    KenwoodCommand::Transmit(Some(false)) => "ZZTX0".to_string(),
                    KenwoodCommand::Transmit(None) => "ZZTX".to_string(),
                    KenwoodCommand::VfoSelect(Some(v)) => format!("ZZFR{}", v),
                    KenwoodCommand::VfoSelect(None) => "ZZFR".to_string(),
                    KenwoodCommand::Split(Some(s)) => format!("ZZSW{}", if *s { 1 } else { 0 }),
                    KenwoodCommand::Split(None) => "ZZSW".to_string(),
                    // Use standard Kenwood encoding for others
                    _ => return kw.encode(),
                }
            }
            FlexCommand::Mode(Some(m)) => format!("ZZMD{:02}", m.to_code()),
            FlexCommand::Mode(None) => "ZZMD".to_string(),
            FlexCommand::Info(_) => "ZZIF".to_string(),
            FlexCommand::AudioGain(Some(g)) => format!("ZZAG{:03}", g),
            FlexCommand::AudioGain(None) => "ZZAG".to_string(),
            FlexCommand::RfPower(Some(p)) => format!("ZZPC{:03}", p),
            FlexCommand::RfPower(None) => "ZZPC".to_string(),
            FlexCommand::SMeter(_) => "ZZSM".to_string(),
            FlexCommand::AgcMode(Some(m)) => format!("ZZGT{}", m),
            FlexCommand::AgcMode(None) => "ZZGT".to_string(),
            FlexCommand::NoiseReduction(Some(on)) => format!("ZZNR{}", if *on { 1 } else { 0 }),
            FlexCommand::NoiseReduction(None) => "ZZNR".to_string(),
            // FlexRadio uses standard Kenwood AI command, not ZZAI
            FlexCommand::AutoInfo(Some(enabled)) => {
                format!("AI{}", if *enabled { 1 } else { 0 })
            }
            FlexCommand::AutoInfo(None) => "AI".to_string(),
            FlexCommand::Unknown(s) => s.clone(),
        };
        format!("{};", cmd).into_bytes()
    }
}

/// Generate a probe command to detect FlexRadio radios
pub fn probe_command() -> Vec<u8> {
    b"ID;".to_vec()
}

/// Check if a response looks like a valid FlexRadio ID response
pub fn is_valid_id_response(data: &[u8]) -> bool {
    // Valid responses: ID904; ID905; ID906; ID907; ID908; ID909; ID910; ID911; ID912; ID913;
    if data.len() >= 5 && data.starts_with(b"ID") && data.ends_with(b";") {
        let id_part = &data[2..data.len() - 1];
        if id_part.iter().all(|b| b.is_ascii_digit()) {
            // Check for FlexRadio ID range (904-913)
            if let Ok(id_str) = std::str::from_utf8(id_part) {
                if let Ok(id_num) = id_str.parse::<u16>() {
                    return (904..=913).contains(&id_num);
                }
            }
        }
    }
    false
}

/// Extract the model code from an ID response
pub fn extract_model_code(data: &[u8]) -> Option<&str> {
    if is_valid_id_response(data) {
        std::str::from_utf8(&data[2..data.len() - 1]).ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_zzfa() {
        let mut codec = FlexCodec::new();
        codec.push_bytes(b"ZZFA00014250000;");

        let cmd = codec.next_command().unwrap();
        match cmd {
            FlexCommand::Kenwood(KenwoodCommand::FrequencyA(Some(14_250_000))) => {}
            other => panic!("Expected FrequencyA(14250000), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_fa() {
        let mut codec = FlexCodec::new();
        codec.push_bytes(b"FA00014250000;");

        let cmd = codec.next_command().unwrap();
        match cmd {
            FlexCommand::Kenwood(KenwoodCommand::FrequencyA(Some(14_250_000))) => {}
            other => panic!("Expected FrequencyA(14250000), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_zzmd() {
        let mut codec = FlexCodec::new();
        codec.push_bytes(b"ZZMD01;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, FlexCommand::Mode(Some(FlexMode::Usb)));
    }

    #[test]
    fn test_parse_zzmd_digu() {
        let mut codec = FlexCodec::new();
        codec.push_bytes(b"ZZMD07;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, FlexCommand::Mode(Some(FlexMode::DigU)));
    }

    #[test]
    fn test_parse_kenwood_mode() {
        let mut codec = FlexCodec::new();
        codec.push_bytes(b"MD2;");

        let cmd = codec.next_command().unwrap();
        // Kenwood mode 2 = USB, converted to FlexMode::Usb
        assert_eq!(cmd, FlexCommand::Mode(Some(FlexMode::Usb)));
    }

    #[test]
    fn test_parse_id() {
        let mut codec = FlexCodec::new();
        codec.push_bytes(b"ID905;");

        let cmd = codec.next_command().unwrap();
        match cmd {
            FlexCommand::Kenwood(KenwoodCommand::Id(Some(ref id))) if id == "905" => {}
            other => panic!("Expected Id(905), got {:?}", other),
        }
    }

    #[test]
    fn test_encode_zzfa() {
        let cmd = FlexCommand::Kenwood(KenwoodCommand::FrequencyA(Some(14_250_000)));
        let encoded = cmd.encode();
        assert_eq!(encoded, b"ZZFA00014250000;");
    }

    #[test]
    fn test_encode_zzmd() {
        let cmd = FlexCommand::Mode(Some(FlexMode::DigU));
        let encoded = cmd.encode();
        assert_eq!(encoded, b"ZZMD07;");
    }

    #[test]
    fn test_streaming_parse() {
        let mut codec = FlexCodec::new();

        // Push partial data
        codec.push_bytes(b"ZZFA000142");
        assert!(codec.next_command().is_none());

        // Push rest
        codec.push_bytes(b"50000;");
        let cmd = codec.next_command().unwrap();
        match cmd {
            FlexCommand::Kenwood(KenwoodCommand::FrequencyA(Some(14_250_000))) => {}
            other => panic!("Expected FrequencyA(14250000), got {:?}", other),
        }
    }

    #[test]
    fn test_multiple_commands() {
        let mut codec = FlexCodec::new();
        codec.push_bytes(b"ZZFA00014250000;ZZMD01;ZZTX1;");

        match codec.next_command() {
            Some(FlexCommand::Kenwood(KenwoodCommand::FrequencyA(Some(14_250_000)))) => {}
            other => panic!("Expected FrequencyA, got {:?}", other),
        }
        assert_eq!(
            codec.next_command(),
            Some(FlexCommand::Mode(Some(FlexMode::Usb)))
        );
        match codec.next_command() {
            Some(FlexCommand::Kenwood(KenwoodCommand::Transmit(Some(true)))) => {}
            other => panic!("Expected Transmit(true), got {:?}", other),
        }
        assert!(codec.next_command().is_none());
    }

    #[test]
    fn test_to_radio_command() {
        let cmd = FlexCommand::Kenwood(KenwoodCommand::FrequencyA(Some(7_074_000)));
        let radio_cmd = cmd.to_radio_command();
        assert_eq!(radio_cmd, RadioCommand::FrequencyReport { hz: 7_074_000 });
    }

    #[test]
    fn test_from_radio_command() {
        let radio_cmd = RadioCommand::SetFrequency { hz: 14_250_000 };
        let cmd = FlexCommand::from_radio_command(&radio_cmd).unwrap();
        match cmd {
            FlexCommand::Kenwood(KenwoodCommand::FrequencyA(Some(14_250_000))) => {}
            other => panic!("Expected FrequencyA(14250000), got {:?}", other),
        }
    }

    #[test]
    fn test_mode_conversion() {
        // FlexMode -> OperatingMode -> FlexMode should round-trip for common modes
        assert_eq!(FlexMode::Usb.to_operating_mode(), OperatingMode::Usb);
        assert_eq!(FlexMode::Lsb.to_operating_mode(), OperatingMode::Lsb);
        assert_eq!(FlexMode::CwL.to_operating_mode(), OperatingMode::Cw);
        assert_eq!(FlexMode::DigU.to_operating_mode(), OperatingMode::DigU);
    }

    #[test]
    fn test_is_valid_id_response() {
        assert!(is_valid_id_response(b"ID904;"));
        assert!(is_valid_id_response(b"ID905;"));
        assert!(is_valid_id_response(b"ID913;"));
        assert!(!is_valid_id_response(b"ID019;")); // Kenwood TS-2000
        assert!(!is_valid_id_response(b"ID;"));
    }

    #[test]
    fn test_parse_zzai_query() {
        let mut codec = FlexCodec::new();
        codec.push_bytes(b"ZZAI;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, FlexCommand::AutoInfo(None));
        assert_eq!(cmd.to_radio_command(), RadioCommand::GetAutoInfo);
    }

    #[test]
    fn test_parse_zzai_enable() {
        let mut codec = FlexCodec::new();
        codec.push_bytes(b"ZZAI1;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, FlexCommand::AutoInfo(Some(true)));
        assert_eq!(
            cmd.to_radio_command(),
            RadioCommand::EnableAutoInfo { enabled: true }
        );
    }

    #[test]
    fn test_parse_zzai_disable() {
        let mut codec = FlexCodec::new();
        codec.push_bytes(b"ZZAI0;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, FlexCommand::AutoInfo(Some(false)));
        assert_eq!(
            cmd.to_radio_command(),
            RadioCommand::EnableAutoInfo { enabled: false }
        );
    }

    #[test]
    fn test_parse_ai_kenwood_style() {
        let mut codec = FlexCodec::new();
        codec.push_bytes(b"AI1;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, FlexCommand::AutoInfo(Some(true)));
    }

    #[test]
    fn test_encode_ai() {
        // FlexRadio uses standard Kenwood AI command
        assert_eq!(FlexCommand::AutoInfo(None).encode(), b"AI;");
        assert_eq!(FlexCommand::AutoInfo(Some(true)).encode(), b"AI1;");
        assert_eq!(FlexCommand::AutoInfo(Some(false)).encode(), b"AI0;");
    }

    #[test]
    fn test_from_radio_command_auto_info() {
        let cmd = FlexCommand::from_radio_command(&RadioCommand::EnableAutoInfo { enabled: true })
            .unwrap();
        assert_eq!(cmd, FlexCommand::AutoInfo(Some(true)));

        let cmd = FlexCommand::from_radio_command(&RadioCommand::GetAutoInfo).unwrap();
        assert_eq!(cmd, FlexCommand::AutoInfo(None));
    }
}
