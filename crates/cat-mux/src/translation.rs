//! Protocol translation between radios and amplifier
//!
//! This module handles translating CAT commands from any source
//! protocol to any target protocol.

use cat_protocol::{
    elecraft::{ElecraftCodec, ElecraftCommand},
    flex::{FlexCodec, FlexCommand},
    icom::{CivCodec, CivCommand, CONTROLLER_ADDR},
    kenwood::{KenwoodCodec, KenwoodCommand},
    yaesu::{YaesuCodec, YaesuCommand},
    EncodeCommand, FromRadioCommand, Protocol, ProtocolCodec, RadioCommand, ToRadioCommand,
};
use serde::{Deserialize, Serialize};

use crate::error::MuxError;

/// Configuration for protocol translation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationConfig {
    /// Frequency precision in Hz (round to this value)
    pub frequency_precision_hz: u64,
    /// Whether to translate unsupported modes to closest equivalent
    pub fallback_modes: bool,
    /// CI-V address for amplifier (if target is Icom)
    pub target_civ_address: Option<u8>,
}

impl Default for TranslationConfig {
    fn default() -> Self {
        Self {
            frequency_precision_hz: 10,
            fallback_modes: true,
            target_civ_address: None,
        }
    }
}

/// Protocol translator
pub struct ProtocolTranslator {
    config: TranslationConfig,
    target_protocol: Protocol,
}

impl ProtocolTranslator {
    /// Create a new translator targeting the given protocol
    pub fn new(target_protocol: Protocol) -> Self {
        Self {
            config: TranslationConfig::default(),
            target_protocol,
        }
    }

    /// Create with custom configuration
    pub fn with_config(target_protocol: Protocol, config: TranslationConfig) -> Self {
        Self {
            config,
            target_protocol,
        }
    }

    /// Get the target protocol
    pub fn target_protocol(&self) -> Protocol {
        self.target_protocol
    }

    /// Set the target protocol
    pub fn set_target_protocol(&mut self, protocol: Protocol) {
        self.target_protocol = protocol;
    }

    /// Translate a RadioCommand to the target protocol bytes
    pub fn translate(&self, cmd: &RadioCommand) -> Result<Vec<u8>, MuxError> {
        // Apply frequency rounding if needed
        let cmd = self.normalize_command(cmd);

        match self.target_protocol {
            Protocol::Kenwood => self.to_kenwood(&cmd),
            Protocol::Elecraft => self.to_elecraft(&cmd),
            Protocol::IcomCIV => self.to_icom(&cmd),
            Protocol::Yaesu => self.to_yaesu(&cmd),
            Protocol::FlexRadio => self.to_flex(&cmd),
        }
    }

    /// Translate from a specific source protocol
    pub fn translate_from(
        &self,
        source_protocol: Protocol,
        data: &[u8],
    ) -> Result<Vec<u8>, MuxError> {
        // Parse the source protocol
        let cmd = self.parse_source(source_protocol, data)?;

        // Translate to target
        self.translate(&cmd)
    }

    /// Parse bytes from a source protocol into a RadioCommand
    fn parse_source(&self, protocol: Protocol, data: &[u8]) -> Result<RadioCommand, MuxError> {
        match protocol {
            Protocol::Kenwood => {
                let mut codec = KenwoodCodec::new();
                codec.push_bytes(data);
                codec
                    .next_command()
                    .map(|c| c.to_radio_command())
                    .ok_or_else(|| MuxError::TranslationError("incomplete Kenwood data".into()))
            }
            Protocol::Elecraft => {
                let mut codec = ElecraftCodec::new();
                codec.push_bytes(data);
                codec
                    .next_command()
                    .map(|c| c.to_radio_command())
                    .ok_or_else(|| MuxError::TranslationError("incomplete Elecraft data".into()))
            }
            Protocol::IcomCIV => {
                let mut codec = CivCodec::new();
                codec.push_bytes(data);
                codec
                    .next_command()
                    .map(|c| c.to_radio_command())
                    .ok_or_else(|| MuxError::TranslationError("incomplete CI-V data".into()))
            }
            Protocol::Yaesu => {
                let mut codec = YaesuCodec::new();
                codec.push_bytes(data);
                codec
                    .next_command()
                    .map(|c| c.to_radio_command())
                    .ok_or_else(|| MuxError::TranslationError("incomplete Yaesu data".into()))
            }
            Protocol::FlexRadio => {
                let mut codec = FlexCodec::new();
                codec.push_bytes(data);
                codec
                    .next_command()
                    .map(|c| c.to_radio_command())
                    .ok_or_else(|| MuxError::TranslationError("incomplete FlexRadio data".into()))
            }
        }
    }

    /// Normalize a command (apply precision, etc.)
    fn normalize_command(&self, cmd: &RadioCommand) -> RadioCommand {
        match cmd {
            RadioCommand::SetFrequency { hz } => {
                let rounded =
                    (*hz / self.config.frequency_precision_hz) * self.config.frequency_precision_hz;
                RadioCommand::SetFrequency { hz: rounded }
            }
            RadioCommand::FrequencyReport { hz } => {
                let rounded =
                    (*hz / self.config.frequency_precision_hz) * self.config.frequency_precision_hz;
                RadioCommand::FrequencyReport { hz: rounded }
            }
            _ => cmd.clone(),
        }
    }

    /// Translate to Kenwood protocol
    fn to_kenwood(&self, cmd: &RadioCommand) -> Result<Vec<u8>, MuxError> {
        let kw_cmd = KenwoodCommand::from_radio_command(cmd)
            .ok_or_else(|| MuxError::TranslationError("cannot translate to Kenwood".into()))?;

        Ok(kw_cmd.encode())
    }

    /// Translate to Elecraft protocol
    fn to_elecraft(&self, cmd: &RadioCommand) -> Result<Vec<u8>, MuxError> {
        let el_cmd = ElecraftCommand::from_radio_command(cmd)
            .ok_or_else(|| MuxError::TranslationError("cannot translate to Elecraft".into()))?;

        Ok(el_cmd.encode())
    }

    /// Translate to Icom CI-V protocol
    fn to_icom(&self, cmd: &RadioCommand) -> Result<Vec<u8>, MuxError> {
        let civ_cmd = CivCommand::from_radio_command(cmd)
            .ok_or_else(|| MuxError::TranslationError("cannot translate to CI-V".into()))?;

        // Set the proper destination address
        let addr = self.config.target_civ_address.unwrap_or(0x00);
        let civ_cmd = CivCommand::new(addr, CONTROLLER_ADDR, civ_cmd.command);

        Ok(civ_cmd.encode())
    }

    /// Translate to Yaesu protocol
    fn to_yaesu(&self, cmd: &RadioCommand) -> Result<Vec<u8>, MuxError> {
        let yaesu_cmd = YaesuCommand::from_radio_command(cmd)
            .ok_or_else(|| MuxError::TranslationError("cannot translate to Yaesu".into()))?;

        Ok(yaesu_cmd.encode())
    }

    /// Translate to FlexRadio protocol
    fn to_flex(&self, cmd: &RadioCommand) -> Result<Vec<u8>, MuxError> {
        let flex_cmd = FlexCommand::from_radio_command(cmd)
            .ok_or_else(|| MuxError::TranslationError("cannot translate to FlexRadio".into()))?;

        Ok(flex_cmd.encode())
    }
}

/// Commands that should be forwarded to the amplifier
pub fn should_forward_to_amp(cmd: &RadioCommand) -> bool {
    matches!(
        cmd,
        RadioCommand::SetFrequency { .. }
            | RadioCommand::FrequencyReport { .. }
            | RadioCommand::SetMode { .. }
            | RadioCommand::ModeReport { .. }
            | RadioCommand::SetPtt { .. }
            | RadioCommand::PttReport { .. }
            | RadioCommand::StatusReport { .. }
    )
}

/// Filter commands that shouldn't be forwarded
pub fn filter_for_amplifier(cmd: &RadioCommand) -> Option<RadioCommand> {
    match cmd {
        // Always forward frequency info
        RadioCommand::SetFrequency { .. } | RadioCommand::FrequencyReport { .. } => {
            Some(cmd.clone())
        }

        // Forward mode info
        RadioCommand::SetMode { .. } | RadioCommand::ModeReport { .. } => Some(cmd.clone()),

        // Forward PTT
        RadioCommand::SetPtt { .. } | RadioCommand::PttReport { .. } => Some(cmd.clone()),

        // Extract frequency/mode from status reports
        RadioCommand::StatusReport {
            frequency_hz: Some(hz),
            ..
        } => Some(RadioCommand::SetFrequency { hz: *hz }),

        // Don't forward queries, VFO changes, ID, etc.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translate_frequency_kenwood_to_icom() {
        let translator = ProtocolTranslator::new(Protocol::IcomCIV);

        let cmd = RadioCommand::SetFrequency { hz: 14_250_000 };
        let result = translator.translate(&cmd).unwrap();

        // Should be a valid CI-V frame
        assert_eq!(result[0], 0xFE);
        assert_eq!(result[1], 0xFE);
        assert_eq!(result[result.len() - 1], 0xFD);
    }

    #[test]
    fn test_translate_ptt_to_kenwood() {
        let translator = ProtocolTranslator::new(Protocol::Kenwood);

        let cmd = RadioCommand::SetPtt { active: true };
        let result = translator.translate(&cmd).unwrap();

        assert_eq!(result, b"TX1;");
    }

    #[test]
    fn test_frequency_precision() {
        let config = TranslationConfig {
            frequency_precision_hz: 100,
            ..Default::default()
        };

        let translator = ProtocolTranslator::with_config(Protocol::Kenwood, config);

        // 14.250.123 should round to 14.250.100
        let cmd = RadioCommand::SetFrequency { hz: 14_250_123 };
        let result = translator.translate(&cmd).unwrap();

        // Check the frequency in the encoded result
        let s = String::from_utf8_lossy(&result);
        assert!(s.contains("14250100"), "Expected 14250100, got {}", s);
    }

    #[test]
    fn test_should_forward() {
        assert!(should_forward_to_amp(&RadioCommand::SetFrequency {
            hz: 14_250_000
        }));
        assert!(should_forward_to_amp(&RadioCommand::SetPtt {
            active: true
        }));
        assert!(!should_forward_to_amp(&RadioCommand::GetId));
        assert!(!should_forward_to_amp(&RadioCommand::GetFrequency));
    }
}
