//! Elecraft Protocol Implementation
//!
//! Elecraft radios (K3, K3S, KX3, KX2) use a Kenwood-compatible ASCII protocol
//! with extensions for Elecraft-specific features.
//!
//! # Format
//! Same as Kenwood: semicolon-terminated ASCII commands
//!
//! # Extensions
//! - `K2;` / `K3;` - Radio identification
//! - `DS;` - Display string
//! - `IC;` - Icon status
//! - Extended parameter ranges and additional commands

use crate::command::{OperatingMode, RadioCommand, Vfo};
use crate::kenwood::{KenwoodCodec, KenwoodCommand};
use crate::{EncodeCommand, FromRadioCommand, ProtocolCodec, ToRadioCommand};

/// Elecraft-specific commands (in addition to Kenwood base)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElecraftCommand {
    /// Base Kenwood command
    Kenwood(KenwoodCommand),
    /// K2 identification (response to K2;)
    K2Id(Option<String>),
    /// K3 identification (response to K3;)
    K3Id(Option<String>),
    /// KX identification
    KxId(Option<String>),
    /// Display reading: DSxxxxxx;
    Display(Option<String>),
    /// Icon/indicator status: ICxx;
    Icon(Option<u8>),
    /// Band selection: BNxx;
    Band(Option<u8>),
    /// Power level: PCxxx;
    Power(Option<u16>),
    /// VFO A/B extended info: VAnn...; VBnn...;
    VfoAInfo(Option<VfoInfo>),
    VfoBInfo(Option<VfoInfo>),
    /// Keyer speed: KSxxx;
    KeyerSpeed(Option<u8>),
    /// RIT/XIT offset: RO+/-xxxxx;
    RitOffset(Option<i32>),
    /// TX meter reading: TMx;
    TxMeter(Option<u8>),
}

/// VFO information (extended)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfoInfo {
    pub frequency_hz: u64,
    pub mode: OperatingMode,
}

/// Streaming Elecraft protocol codec
pub struct ElecraftCodec {
    inner: KenwoodCodec,
}

impl ElecraftCodec {
    /// Create a new Elecraft codec
    pub fn new() -> Self {
        Self {
            inner: KenwoodCodec::new(),
        }
    }

    /// Parse Elecraft-specific commands
    fn parse_elecraft(cmd_str: &str) -> Option<ElecraftCommand> {
        if cmd_str.len() < 2 {
            return None;
        }

        let prefix = &cmd_str[..2];
        let params = &cmd_str[2..];

        match prefix {
            "K2" => Some(ElecraftCommand::K2Id(if params.is_empty() {
                None
            } else {
                Some(params.to_string())
            })),
            "K3" => Some(ElecraftCommand::K3Id(if params.is_empty() {
                None
            } else {
                Some(params.to_string())
            })),
            "KX" => Some(ElecraftCommand::KxId(if params.is_empty() {
                None
            } else {
                Some(params.to_string())
            })),
            "DS" => Some(ElecraftCommand::Display(if params.is_empty() {
                None
            } else {
                Some(params.to_string())
            })),
            "IC" => Some(ElecraftCommand::Icon(params.parse().ok())),
            "BN" => Some(ElecraftCommand::Band(params.parse().ok())),
            "PC" => Some(ElecraftCommand::Power(params.parse().ok())),
            "KS" => Some(ElecraftCommand::KeyerSpeed(params.parse().ok())),
            "TM" => Some(ElecraftCommand::TxMeter(params.parse().ok())),
            "RO" => {
                let offset = parse_rit_offset(params);
                Some(ElecraftCommand::RitOffset(offset))
            }
            _ => None, // Fall through to Kenwood parsing
        }
    }
}

impl Default for ElecraftCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl ProtocolCodec for ElecraftCodec {
    type Command = ElecraftCommand;

    fn push_bytes(&mut self, data: &[u8]) {
        self.inner.push_bytes(data);
    }

    fn next_command(&mut self) -> Option<Self::Command> {
        // Try to get the next Kenwood command
        let kenwood_cmd = self.inner.next_command()?;

        // Check if it's an Elecraft-specific command
        if let KenwoodCommand::Unknown(s) = &kenwood_cmd {
            // Try to parse as Elecraft-specific
            if let Some(elecraft_cmd) = Self::parse_elecraft(s) {
                return Some(elecraft_cmd);
            }
        }

        // Return as wrapped Kenwood command
        Some(ElecraftCommand::Kenwood(kenwood_cmd))
    }

    fn clear(&mut self) {
        self.inner.clear();
    }
}

impl ToRadioCommand for ElecraftCommand {
    fn to_radio_command(&self) -> RadioCommand {
        match self {
            ElecraftCommand::Kenwood(kw) => kw.to_radio_command(),
            ElecraftCommand::K2Id(Some(id)) => RadioCommand::IdReport {
                id: format!("K2:{}", id),
            },
            ElecraftCommand::K2Id(None) => RadioCommand::GetId,
            ElecraftCommand::K3Id(Some(id)) => RadioCommand::IdReport {
                id: format!("K3:{}", id),
            },
            ElecraftCommand::K3Id(None) => RadioCommand::GetId,
            ElecraftCommand::KxId(Some(id)) => RadioCommand::IdReport {
                id: format!("KX:{}", id),
            },
            ElecraftCommand::KxId(None) => RadioCommand::GetId,
            ElecraftCommand::Display(Some(s)) => RadioCommand::Unknown {
                data: s.as_bytes().to_vec(),
            },
            ElecraftCommand::Display(None) => RadioCommand::GetStatus,
            ElecraftCommand::VfoAInfo(Some(info)) => RadioCommand::StatusReport {
                frequency_hz: Some(info.frequency_hz),
                mode: Some(info.mode),
                ptt: None,
                vfo: Some(Vfo::A),
            },
            ElecraftCommand::VfoBInfo(Some(info)) => RadioCommand::StatusReport {
                frequency_hz: Some(info.frequency_hz),
                mode: Some(info.mode),
                ptt: None,
                vfo: Some(Vfo::B),
            },
            _ => RadioCommand::Unknown { data: vec![] },
        }
    }
}

impl FromRadioCommand for ElecraftCommand {
    fn from_radio_command(cmd: &RadioCommand) -> Option<Self> {
        // First try Elecraft-specific mappings
        match cmd {
            RadioCommand::GetId => Some(ElecraftCommand::K3Id(None)),
            _ => {
                // Fall back to Kenwood
                KenwoodCommand::from_radio_command(cmd).map(ElecraftCommand::Kenwood)
            }
        }
    }
}

impl EncodeCommand for ElecraftCommand {
    fn encode(&self) -> Vec<u8> {
        match self {
            ElecraftCommand::Kenwood(kw) => kw.encode(),
            ElecraftCommand::K2Id(None) => b"K2;".to_vec(),
            ElecraftCommand::K2Id(Some(id)) => format!("K2{};", id).into_bytes(),
            ElecraftCommand::K3Id(None) => b"K3;".to_vec(),
            ElecraftCommand::K3Id(Some(id)) => format!("K3{};", id).into_bytes(),
            ElecraftCommand::KxId(None) => b"KX;".to_vec(),
            ElecraftCommand::KxId(Some(id)) => format!("KX{};", id).into_bytes(),
            ElecraftCommand::Display(None) => b"DS;".to_vec(),
            ElecraftCommand::Display(Some(s)) => format!("DS{};", s).into_bytes(),
            ElecraftCommand::Icon(None) => b"IC;".to_vec(),
            ElecraftCommand::Icon(Some(v)) => format!("IC{:02};", v).into_bytes(),
            ElecraftCommand::Band(None) => b"BN;".to_vec(),
            ElecraftCommand::Band(Some(v)) => format!("BN{:02};", v).into_bytes(),
            ElecraftCommand::Power(None) => b"PC;".to_vec(),
            ElecraftCommand::Power(Some(v)) => format!("PC{:03};", v).into_bytes(),
            ElecraftCommand::KeyerSpeed(None) => b"KS;".to_vec(),
            ElecraftCommand::KeyerSpeed(Some(v)) => format!("KS{:03};", v).into_bytes(),
            ElecraftCommand::TxMeter(None) => b"TM;".to_vec(),
            ElecraftCommand::TxMeter(Some(v)) => format!("TM{};", v).into_bytes(),
            ElecraftCommand::RitOffset(None) => b"RO;".to_vec(),
            ElecraftCommand::RitOffset(Some(v)) => {
                if *v >= 0 {
                    format!("RO+{:05};", v).into_bytes()
                } else {
                    format!("RO{:06};", v).into_bytes()
                }
            }
            ElecraftCommand::VfoAInfo(None) => b"VA;".to_vec(),
            ElecraftCommand::VfoAInfo(Some(info)) => format!(
                "VA{:011}{};",
                info.frequency_hz,
                elecraft_mode_code(info.mode)
            )
            .into_bytes(),
            ElecraftCommand::VfoBInfo(None) => b"VB;".to_vec(),
            ElecraftCommand::VfoBInfo(Some(info)) => format!(
                "VB{:011}{};",
                info.frequency_hz,
                elecraft_mode_code(info.mode)
            )
            .into_bytes(),
        }
    }
}

/// Parse RIT offset from string like "+00100" or "-00050"
fn parse_rit_offset(s: &str) -> Option<i32> {
    if s.is_empty() {
        return None;
    }

    s.parse().ok()
}

/// Get Elecraft mode code for encoding
fn elecraft_mode_code(mode: OperatingMode) -> u8 {
    match mode {
        OperatingMode::Lsb => 1,
        OperatingMode::Usb => 2,
        OperatingMode::Cw => 3,
        OperatingMode::Fm => 4,
        OperatingMode::Am => 5,
        OperatingMode::Data | OperatingMode::DataL => 6,
        OperatingMode::CwR => 7,
        OperatingMode::DataU => 9,
        _ => 2, // Default to USB
    }
}

/// Generate probe commands to detect Elecraft radios
/// Returns multiple commands to try in sequence
pub fn probe_commands() -> Vec<Vec<u8>> {
    vec![
        b"K3;".to_vec(), // K3/K3S
        b"K2;".to_vec(), // K2
        b"ID;".to_vec(), // Fall back to standard Kenwood ID
    ]
}

/// Check if a response indicates an Elecraft radio
pub fn is_elecraft_response(data: &[u8]) -> Option<&'static str> {
    let s = std::str::from_utf8(data).ok()?;

    if s.starts_with("K3") {
        Some("K3")
    } else if s.starts_with("K2") {
        Some("K2")
    } else if s.starts_with("KX3") {
        Some("KX3")
    } else if s.starts_with("KX2") {
        Some("KX2")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_k3_id() {
        let mut codec = ElecraftCodec::new();
        codec.push_bytes(b"K3;");

        let cmd = codec.next_command().unwrap();
        // Note: "K3" is treated as Unknown by Kenwood codec, then parsed as Elecraft
        // Since "K3" without more info becomes K3Id(None)
        match cmd {
            ElecraftCommand::K3Id(_) => {}
            other => panic!("Expected K3Id, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_kenwood_through_elecraft() {
        let mut codec = ElecraftCodec::new();
        codec.push_bytes(b"FA00014250000;");

        let cmd = codec.next_command().unwrap();
        match cmd {
            ElecraftCommand::Kenwood(KenwoodCommand::FrequencyA(Some(14250000))) => {}
            other => panic!("Expected FrequencyA, got {:?}", other),
        }
    }

    #[test]
    fn test_encode_k3_probe() {
        let cmd = ElecraftCommand::K3Id(None);
        assert_eq!(cmd.encode(), b"K3;");
    }

    #[test]
    fn test_encode_power() {
        let cmd = ElecraftCommand::Power(Some(100));
        assert_eq!(cmd.encode(), b"PC100;");
    }

    #[test]
    fn test_to_radio_command() {
        let cmd = ElecraftCommand::Kenwood(KenwoodCommand::FrequencyA(Some(7_074_000)));
        let radio_cmd = cmd.to_radio_command();
        assert_eq!(radio_cmd, RadioCommand::SetFrequency { hz: 7_074_000 });
    }
}
