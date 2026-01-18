//! FlexRadio SmartSDR CAT Protocol Implementation
//!
//! FlexRadio uses a Kenwood-compatible ASCII protocol with extensions.
//! Commands are semicolon-terminated and support both:
//! - Standard Kenwood 2-letter commands (FA, FB, MD, TX, RX, ID, IF)
//! - FlexRadio extended 4-letter ZZ commands (ZZFA, ZZFB, ZZMD, ZZTX, ZZIF)
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
use crate::error::ParseError;
use crate::{EncodeCommand, FromRadioCommand, ProtocolCodec, ToRadioCommand};

/// Maximum command length (reasonable limit to prevent buffer overflow)
const MAX_COMMAND_LEN: usize = 128;

/// FlexRadio protocol command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlexCommand {
    /// Set/get VFO A frequency: ZZFA00014250000; or FA00014250000;
    FrequencyA(Option<u64>),
    /// Set/get VFO B frequency: ZZFB00007074000; or FB00007074000;
    FrequencyB(Option<u64>),
    /// Set/get mode: ZZMD01; (2-digit) or MD1; (Kenwood 1-digit)
    Mode(Option<FlexMode>),
    /// Transmit: ZZTX1; or TX1;
    Transmit(Option<bool>),
    /// Receive: RX;
    Receive,
    /// Radio identification query: ID;
    Id(Option<String>),
    /// Information/status query: ZZIF...; or IF...;
    Info(Option<FlexInfo>),
    /// VFO select: FR0; or ZZFR;
    VfoSelect(Option<u8>),
    /// Split mode toggle: ZZSW0; or FT0;
    Split(Option<bool>),
    /// Power control: PS0; or PS1;
    Power(Option<bool>),
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
            OperatingMode::Dig | OperatingMode::DigU | OperatingMode::DataU | OperatingMode::Data | OperatingMode::Pkt => {
                Self::DigU
            }
            OperatingMode::DigL | OperatingMode::DataL => Self::DigL,
            OperatingMode::Rtty => Self::Rtty,
            OperatingMode::RttyR => Self::Rtty,
        }
    }
}

/// Parsed ZZIF/IF response data
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
pub struct FlexCodec {
    buffer: Vec<u8>,
}

impl FlexCodec {
    /// Create a new FlexRadio codec
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(128),
        }
    }

    /// Parse a complete command string (without terminator)
    fn parse_command(cmd: &str) -> Result<FlexCommand, ParseError> {
        if cmd.is_empty() {
            return Err(ParseError::InvalidFrame("empty command".into()));
        }

        // Check for ZZ-prefixed commands first (4-letter)
        if cmd.starts_with("ZZ") && cmd.len() >= 4 {
            return Self::parse_zz_command(cmd);
        }

        // Standard Kenwood-style commands (2-letter)
        if cmd.len() < 2 {
            return Err(ParseError::InvalidFrame("command too short".into()));
        }

        let prefix = &cmd[..2];
        let params = &cmd[2..];

        match prefix {
            "FA" => Self::parse_frequency_a(params),
            "FB" => Self::parse_frequency_b(params),
            "MD" => Self::parse_mode_kenwood(params),
            "TX" => Self::parse_transmit(params),
            "RX" => Ok(FlexCommand::Receive),
            "ID" => Self::parse_id(params),
            "IF" => Self::parse_info(params),
            "FR" => Self::parse_vfo_select(params),
            "FT" => Self::parse_split(params),
            "PS" => Self::parse_power(params),
            _ => Ok(FlexCommand::Unknown(cmd.to_string())),
        }
    }

    /// Parse ZZ-prefixed FlexRadio commands
    fn parse_zz_command(cmd: &str) -> Result<FlexCommand, ParseError> {
        let prefix = &cmd[..4];
        let params = &cmd[4..];

        match prefix {
            "ZZFA" => Self::parse_frequency_a(params),
            "ZZFB" => Self::parse_frequency_b(params),
            "ZZMD" | "ZZME" => Self::parse_mode_flex(params),
            "ZZTX" => Self::parse_transmit(params),
            "ZZIF" => Self::parse_info(params),
            "ZZFR" => Self::parse_vfo_select(params),
            "ZZSW" => Self::parse_split(params),
            "ZZAG" => Self::parse_audio_gain(params),
            "ZZPC" => Self::parse_rf_power(params),
            "ZZSM" => Self::parse_smeter(params),
            "ZZGT" => Self::parse_agc_mode(params),
            "ZZNR" => Self::parse_noise_reduction(params),
            _ => Ok(FlexCommand::Unknown(cmd.to_string())),
        }
    }

    fn parse_frequency_a(params: &str) -> Result<FlexCommand, ParseError> {
        if params.is_empty() {
            Ok(FlexCommand::FrequencyA(None))
        } else {
            let freq = params
                .parse::<u64>()
                .map_err(|_| ParseError::InvalidFrequency(params.into()))?;
            Ok(FlexCommand::FrequencyA(Some(freq)))
        }
    }

    fn parse_frequency_b(params: &str) -> Result<FlexCommand, ParseError> {
        if params.is_empty() {
            Ok(FlexCommand::FrequencyB(None))
        } else {
            let freq = params
                .parse::<u64>()
                .map_err(|_| ParseError::InvalidFrequency(params.into()))?;
            Ok(FlexCommand::FrequencyB(Some(freq)))
        }
    }

    fn parse_mode_kenwood(params: &str) -> Result<FlexCommand, ParseError> {
        if params.is_empty() {
            Ok(FlexCommand::Mode(None))
        } else {
            // Kenwood uses single digit, map to FlexMode
            let mode_num = params
                .parse::<u8>()
                .map_err(|_| ParseError::InvalidMode(params.into()))?;
            // Map Kenwood mode numbers to FlexMode
            let flex_mode = match mode_num {
                1 => FlexMode::Lsb,
                2 => FlexMode::Usb,
                3 => FlexMode::CwU,
                4 => FlexMode::Fm,
                5 => FlexMode::Am,
                6 => FlexMode::Rtty,
                7 => FlexMode::CwL,
                9 => FlexMode::Rtty,
                _ => FlexMode::Usb,
            };
            Ok(FlexCommand::Mode(Some(flex_mode)))
        }
    }

    fn parse_mode_flex(params: &str) -> Result<FlexCommand, ParseError> {
        if params.is_empty() {
            Ok(FlexCommand::Mode(None))
        } else {
            let mode_num = params
                .parse::<u8>()
                .map_err(|_| ParseError::InvalidMode(params.into()))?;
            let flex_mode =
                FlexMode::from_code(mode_num).ok_or_else(|| ParseError::InvalidMode(params.into()))?;
            Ok(FlexCommand::Mode(Some(flex_mode)))
        }
    }

    fn parse_transmit(params: &str) -> Result<FlexCommand, ParseError> {
        if params.is_empty() {
            Ok(FlexCommand::Transmit(Some(true)))
        } else {
            let tx = params != "0";
            Ok(FlexCommand::Transmit(Some(tx)))
        }
    }

    fn parse_id(params: &str) -> Result<FlexCommand, ParseError> {
        if params.is_empty() {
            Ok(FlexCommand::Id(None))
        } else {
            Ok(FlexCommand::Id(Some(params.to_string())))
        }
    }

    fn parse_info(params: &str) -> Result<FlexCommand, ParseError> {
        if params.is_empty() {
            Ok(FlexCommand::Info(None))
        } else {
            // ZZIF format: 11-digit freq, 4-digit step, 6-digit RIT, RIT on, XIT on, TX, mode, VFO, split
            if params.len() < 28 {
                return Err(ParseError::InvalidFrame(format!(
                    "IF response too short: {} chars",
                    params.len()
                )));
            }

            let frequency_hz = params[0..11]
                .parse::<u64>()
                .map_err(|_| ParseError::InvalidFrequency(params[0..11].into()))?;

            let step_size = params[11..15].parse::<u32>().unwrap_or(1);

            let rit_offset = params[15..21].parse::<i32>().unwrap_or(0);
            let rit_on = params.chars().nth(21) == Some('1');
            let xit_on = params.chars().nth(22) == Some('1');
            let tx = params.chars().nth(23) != Some('0');

            let mode_code = params[24..26].parse::<u8>().unwrap_or(1);
            let mode = FlexMode::from_code(mode_code).unwrap_or(FlexMode::Usb);

            let vfo = params[26..27].parse::<u8>().unwrap_or(0);
            let split = params.chars().nth(27) == Some('1');

            Ok(FlexCommand::Info(Some(FlexInfo {
                frequency_hz,
                step_size,
                rit_offset,
                rit_on,
                xit_on,
                tx,
                mode,
                vfo,
                split,
            })))
        }
    }

    fn parse_vfo_select(params: &str) -> Result<FlexCommand, ParseError> {
        if params.is_empty() {
            Ok(FlexCommand::VfoSelect(None))
        } else {
            let vfo = params
                .parse::<u8>()
                .map_err(|_| ParseError::InvalidFrame("invalid VFO".into()))?;
            Ok(FlexCommand::VfoSelect(Some(vfo)))
        }
    }

    fn parse_split(params: &str) -> Result<FlexCommand, ParseError> {
        if params.is_empty() {
            Ok(FlexCommand::Split(None))
        } else {
            let split = params != "0";
            Ok(FlexCommand::Split(Some(split)))
        }
    }

    fn parse_power(params: &str) -> Result<FlexCommand, ParseError> {
        if params.is_empty() {
            Ok(FlexCommand::Power(None))
        } else {
            let on = params != "0";
            Ok(FlexCommand::Power(Some(on)))
        }
    }

    fn parse_audio_gain(params: &str) -> Result<FlexCommand, ParseError> {
        if params.is_empty() {
            Ok(FlexCommand::AudioGain(None))
        } else {
            let gain = params.parse::<u8>().unwrap_or(50);
            Ok(FlexCommand::AudioGain(Some(gain)))
        }
    }

    fn parse_rf_power(params: &str) -> Result<FlexCommand, ParseError> {
        if params.is_empty() {
            Ok(FlexCommand::RfPower(None))
        } else {
            let power = params.parse::<u8>().unwrap_or(100);
            Ok(FlexCommand::RfPower(Some(power)))
        }
    }

    fn parse_smeter(params: &str) -> Result<FlexCommand, ParseError> {
        if params.is_empty() {
            Ok(FlexCommand::SMeter(None))
        } else {
            let value = params.parse::<i16>().unwrap_or(0);
            Ok(FlexCommand::SMeter(Some(value)))
        }
    }

    fn parse_agc_mode(params: &str) -> Result<FlexCommand, ParseError> {
        if params.is_empty() {
            Ok(FlexCommand::AgcMode(None))
        } else {
            let mode = params.parse::<u8>().unwrap_or(0);
            Ok(FlexCommand::AgcMode(Some(mode)))
        }
    }

    fn parse_noise_reduction(params: &str) -> Result<FlexCommand, ParseError> {
        if params.is_empty() {
            Ok(FlexCommand::NoiseReduction(None))
        } else {
            let on = params != "0";
            Ok(FlexCommand::NoiseReduction(Some(on)))
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
                tracing::warn!("Failed to parse FlexRadio command: {}", e);
                Some(FlexCommand::Unknown(cmd_str.into_owned()))
            }
        }
    }

    fn clear(&mut self) {
        self.buffer.clear();
    }
}

impl ToRadioCommand for FlexCommand {
    fn to_radio_command(&self) -> RadioCommand {
        match self {
            FlexCommand::FrequencyA(Some(hz)) => RadioCommand::SetFrequency { hz: *hz },
            FlexCommand::FrequencyA(None) => RadioCommand::GetFrequency,
            FlexCommand::FrequencyB(Some(hz)) => RadioCommand::SetFrequency { hz: *hz },
            FlexCommand::FrequencyB(None) => RadioCommand::GetFrequency,
            FlexCommand::Mode(Some(m)) => RadioCommand::SetMode {
                mode: m.to_operating_mode(),
            },
            FlexCommand::Mode(None) => RadioCommand::GetMode,
            FlexCommand::Transmit(Some(tx)) => RadioCommand::SetPtt { active: *tx },
            FlexCommand::Transmit(None) => RadioCommand::GetPtt,
            FlexCommand::Receive => RadioCommand::SetPtt { active: false },
            FlexCommand::Id(Some(id)) => RadioCommand::IdReport { id: id.clone() },
            FlexCommand::Id(None) => RadioCommand::GetId,
            FlexCommand::Info(Some(info)) => RadioCommand::StatusReport {
                frequency_hz: Some(info.frequency_hz),
                mode: Some(info.mode.to_operating_mode()),
                ptt: Some(info.tx),
                vfo: Some(if info.vfo == 0 { Vfo::A } else { Vfo::B }),
            },
            FlexCommand::Info(None) => RadioCommand::GetStatus,
            FlexCommand::VfoSelect(Some(v)) => RadioCommand::SetVfo {
                vfo: if *v == 0 { Vfo::A } else { Vfo::B },
            },
            FlexCommand::VfoSelect(None) => RadioCommand::GetVfo,
            FlexCommand::Split(Some(s)) => RadioCommand::SetVfo {
                vfo: if *s { Vfo::Split } else { Vfo::A },
            },
            FlexCommand::Split(None) => RadioCommand::GetVfo,
            FlexCommand::Power(Some(on)) => RadioCommand::SetPower { on: *on },
            FlexCommand::Power(None) | FlexCommand::AudioGain(_) | FlexCommand::RfPower(_)
            | FlexCommand::SMeter(_) | FlexCommand::AgcMode(_) | FlexCommand::NoiseReduction(_) => {
                RadioCommand::Unknown { data: vec![] }
            }
            FlexCommand::Unknown(s) => RadioCommand::Unknown {
                data: s.as_bytes().to_vec(),
            },
        }
    }
}

impl FromRadioCommand for FlexCommand {
    fn from_radio_command(cmd: &RadioCommand) -> Option<Self> {
        match cmd {
            RadioCommand::SetFrequency { hz } => Some(FlexCommand::FrequencyA(Some(*hz))),
            RadioCommand::GetFrequency => Some(FlexCommand::FrequencyA(None)),
            RadioCommand::FrequencyReport { hz } => Some(FlexCommand::FrequencyA(Some(*hz))),
            RadioCommand::SetMode { mode } => Some(FlexCommand::Mode(Some(
                FlexMode::from_operating_mode(*mode),
            ))),
            RadioCommand::GetMode => Some(FlexCommand::Mode(None)),
            RadioCommand::ModeReport { mode } => Some(FlexCommand::Mode(Some(
                FlexMode::from_operating_mode(*mode),
            ))),
            RadioCommand::SetPtt { active: true } => Some(FlexCommand::Transmit(Some(true))),
            RadioCommand::SetPtt { active: false } => Some(FlexCommand::Receive),
            RadioCommand::GetPtt => Some(FlexCommand::Transmit(None)),
            RadioCommand::PttReport { active } => Some(FlexCommand::Transmit(Some(*active))),
            RadioCommand::SetVfo { vfo } => match vfo {
                Vfo::A => Some(FlexCommand::VfoSelect(Some(0))),
                Vfo::B => Some(FlexCommand::VfoSelect(Some(1))),
                Vfo::Split => Some(FlexCommand::Split(Some(true))),
                Vfo::Memory => Some(FlexCommand::VfoSelect(Some(2))),
            },
            RadioCommand::GetVfo => Some(FlexCommand::VfoSelect(None)),
            RadioCommand::GetId => Some(FlexCommand::Id(None)),
            RadioCommand::IdReport { id } => Some(FlexCommand::Id(Some(id.clone()))),
            RadioCommand::GetStatus => Some(FlexCommand::Info(None)),
            RadioCommand::SetPower { on } => Some(FlexCommand::Power(Some(*on))),
            _ => None,
        }
    }
}

impl EncodeCommand for FlexCommand {
    fn encode(&self) -> Vec<u8> {
        let cmd = match self {
            // Use ZZ commands for better precision
            FlexCommand::FrequencyA(Some(hz)) => format!("ZZFA{:011}", hz),
            FlexCommand::FrequencyA(None) => "ZZFA".to_string(),
            FlexCommand::FrequencyB(Some(hz)) => format!("ZZFB{:011}", hz),
            FlexCommand::FrequencyB(None) => "ZZFB".to_string(),
            FlexCommand::Mode(Some(m)) => format!("ZZMD{:02}", m.to_code()),
            FlexCommand::Mode(None) => "ZZMD".to_string(),
            FlexCommand::Transmit(Some(true)) => "ZZTX1".to_string(),
            FlexCommand::Transmit(Some(false)) => "ZZTX0".to_string(),
            FlexCommand::Transmit(None) => "ZZTX".to_string(),
            FlexCommand::Receive => "RX".to_string(),
            FlexCommand::Id(Some(id)) => format!("ID{}", id),
            FlexCommand::Id(None) => "ID".to_string(),
            FlexCommand::Info(_) => "ZZIF".to_string(),
            FlexCommand::VfoSelect(Some(v)) => format!("ZZFR{}", v),
            FlexCommand::VfoSelect(None) => "ZZFR".to_string(),
            FlexCommand::Split(Some(s)) => format!("ZZSW{}", if *s { 1 } else { 0 }),
            FlexCommand::Split(None) => "ZZSW".to_string(),
            FlexCommand::Power(Some(on)) => format!("PS{}", if *on { 1 } else { 0 }),
            FlexCommand::Power(None) => "PS".to_string(),
            FlexCommand::AudioGain(Some(g)) => format!("ZZAG{:03}", g),
            FlexCommand::AudioGain(None) => "ZZAG".to_string(),
            FlexCommand::RfPower(Some(p)) => format!("ZZPC{:03}", p),
            FlexCommand::RfPower(None) => "ZZPC".to_string(),
            FlexCommand::SMeter(_) => "ZZSM".to_string(),
            FlexCommand::AgcMode(Some(m)) => format!("ZZGT{}", m),
            FlexCommand::AgcMode(None) => "ZZGT".to_string(),
            FlexCommand::NoiseReduction(Some(on)) => format!("ZZNR{}", if *on { 1 } else { 0 }),
            FlexCommand::NoiseReduction(None) => "ZZNR".to_string(),
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
        assert_eq!(cmd, FlexCommand::FrequencyA(Some(14_250_000)));
    }

    #[test]
    fn test_parse_fa() {
        let mut codec = FlexCodec::new();
        codec.push_bytes(b"FA00014250000;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, FlexCommand::FrequencyA(Some(14_250_000)));
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
    fn test_parse_id() {
        let mut codec = FlexCodec::new();
        codec.push_bytes(b"ID905;");

        let cmd = codec.next_command().unwrap();
        assert_eq!(cmd, FlexCommand::Id(Some("905".to_string())));
    }

    #[test]
    fn test_encode_zzfa() {
        let cmd = FlexCommand::FrequencyA(Some(14_250_000));
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
        assert_eq!(cmd, FlexCommand::FrequencyA(Some(14_250_000)));
    }

    #[test]
    fn test_multiple_commands() {
        let mut codec = FlexCodec::new();
        codec.push_bytes(b"ZZFA00014250000;ZZMD01;ZZTX1;");

        assert_eq!(
            codec.next_command(),
            Some(FlexCommand::FrequencyA(Some(14_250_000)))
        );
        assert_eq!(codec.next_command(), Some(FlexCommand::Mode(Some(FlexMode::Usb))));
        assert_eq!(codec.next_command(), Some(FlexCommand::Transmit(Some(true))));
        assert!(codec.next_command().is_none());
    }

    #[test]
    fn test_to_radio_command() {
        let cmd = FlexCommand::FrequencyA(Some(7_074_000));
        let radio_cmd = cmd.to_radio_command();
        assert_eq!(radio_cmd, RadioCommand::SetFrequency { hz: 7_074_000 });
    }

    #[test]
    fn test_from_radio_command() {
        let radio_cmd = RadioCommand::SetFrequency { hz: 14_250_000 };
        let cmd = FlexCommand::from_radio_command(&radio_cmd).unwrap();
        assert_eq!(cmd, FlexCommand::FrequencyA(Some(14_250_000)));
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
}
