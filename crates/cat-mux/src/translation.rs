//! Protocol translation between radios and amplifier
//!
//! This module handles translating CAT responses from any source
//! protocol to any target protocol. Responses flow from radios
//! through the mux to amplifiers.

use cat_protocol::{
    elecraft::{ElecraftCodec, ElecraftCommand},
    flex::{FlexCodec, FlexCommand},
    icom::{CivCodec, CivCommand, CONTROLLER_ADDR},
    kenwood::{KenwoodCodec, KenwoodCommand},
    yaesu::{YaesuCodec, YaesuCommand},
    yaesu_ascii::YaesuAsciiCommand,
    EncodeCommand, FromRadioResponse, Protocol, ProtocolCodec, RadioResponse, ToRadioResponse,
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

    /// Translate a RadioResponse to the target protocol bytes
    ///
    /// This is used to send responses (frequency reports, mode reports, etc.)
    /// from the radio to an amplifier in the amplifier's native protocol.
    pub fn translate_response(&self, resp: &RadioResponse) -> Result<Vec<u8>, MuxError> {
        // Apply frequency rounding if needed
        let resp = self.normalize_response(resp);

        match self.target_protocol {
            Protocol::Kenwood => self.to_kenwood(&resp),
            Protocol::Elecraft => self.to_elecraft(&resp),
            Protocol::IcomCIV => self.to_icom(&resp),
            Protocol::Yaesu | Protocol::YaesuAscii => self.to_yaesu(&resp),
            Protocol::FlexRadio => self.to_flex(&resp),
        }
    }

    /// Translate from a specific source protocol to the target protocol
    ///
    /// Parses the source data as a radio response and translates it to the target protocol.
    pub fn translate_from(
        &self,
        source_protocol: Protocol,
        data: &[u8],
    ) -> Result<Vec<u8>, MuxError> {
        // Parse the source protocol as a response (data FROM a radio)
        let resp = self.parse_source_response(source_protocol, data)?;

        // Translate to target
        self.translate_response(&resp)
    }

    /// Parse bytes from a source protocol into a RadioResponse
    ///
    /// This interprets the data as a response FROM a radio (frequency reports, etc.)
    fn parse_source_response(
        &self,
        protocol: Protocol,
        data: &[u8],
    ) -> Result<RadioResponse, MuxError> {
        match protocol {
            Protocol::Kenwood => {
                let mut codec = KenwoodCodec::new();
                codec.push_bytes(data);
                codec
                    .next_command()
                    .map(|c| c.to_radio_response())
                    .ok_or_else(|| MuxError::TranslationError("incomplete Kenwood data".into()))
            }
            Protocol::Elecraft => {
                let mut codec = ElecraftCodec::new();
                codec.push_bytes(data);
                codec
                    .next_command()
                    .map(|c| c.to_radio_response())
                    .ok_or_else(|| MuxError::TranslationError("incomplete Elecraft data".into()))
            }
            Protocol::IcomCIV => {
                let mut codec = CivCodec::new();
                codec.push_bytes(data);
                codec
                    .next_command()
                    .map(|c| c.to_radio_response())
                    .ok_or_else(|| MuxError::TranslationError("incomplete CI-V data".into()))
            }
            Protocol::Yaesu | Protocol::YaesuAscii => {
                let mut codec = YaesuCodec::new();
                codec.push_bytes(data);
                codec
                    .next_command()
                    .map(|c| c.to_radio_response())
                    .ok_or_else(|| MuxError::TranslationError("incomplete Yaesu data".into()))
            }
            Protocol::FlexRadio => {
                let mut codec = FlexCodec::new();
                codec.push_bytes(data);
                codec
                    .next_command()
                    .map(|c| c.to_radio_response())
                    .ok_or_else(|| MuxError::TranslationError("incomplete FlexRadio data".into()))
            }
        }
    }

    /// Normalize a response (apply precision, etc.)
    fn normalize_response(&self, resp: &RadioResponse) -> RadioResponse {
        match resp {
            RadioResponse::Frequency { hz } => {
                let rounded =
                    (*hz / self.config.frequency_precision_hz) * self.config.frequency_precision_hz;
                RadioResponse::Frequency { hz: rounded }
            }
            RadioResponse::Status {
                frequency_hz: Some(hz),
                mode,
                ptt,
                vfo,
            } => {
                let rounded =
                    (*hz / self.config.frequency_precision_hz) * self.config.frequency_precision_hz;
                RadioResponse::Status {
                    frequency_hz: Some(rounded),
                    mode: *mode,
                    ptt: *ptt,
                    vfo: *vfo,
                }
            }
            _ => resp.clone(),
        }
    }

    /// Translate response to Kenwood protocol
    fn to_kenwood(&self, resp: &RadioResponse) -> Result<Vec<u8>, MuxError> {
        let kw_cmd = KenwoodCommand::from_radio_response(resp)
            .ok_or_else(|| MuxError::TranslationError("cannot translate to Kenwood".into()))?;

        Ok(kw_cmd.encode())
    }

    /// Translate response to Elecraft protocol
    fn to_elecraft(&self, resp: &RadioResponse) -> Result<Vec<u8>, MuxError> {
        let el_cmd = ElecraftCommand::from_radio_response(resp)
            .ok_or_else(|| MuxError::TranslationError("cannot translate to Elecraft".into()))?;

        Ok(el_cmd.encode())
    }

    /// Translate response to Icom CI-V protocol
    fn to_icom(&self, resp: &RadioResponse) -> Result<Vec<u8>, MuxError> {
        let civ_cmd = CivCommand::from_radio_response(resp)
            .ok_or_else(|| MuxError::TranslationError("cannot translate to CI-V".into()))?;

        // Set the proper destination address
        let addr = self.config.target_civ_address.unwrap_or(0x00);
        let civ_cmd = CivCommand::new(addr, CONTROLLER_ADDR, civ_cmd.command);

        Ok(civ_cmd.encode())
    }

    /// Translate response to Yaesu protocol
    fn to_yaesu(&self, resp: &RadioResponse) -> Result<Vec<u8>, MuxError> {
        let yaesu_cmd = YaesuCommand::from_radio_response(resp)
            .ok_or_else(|| MuxError::TranslationError("cannot translate to Yaesu".into()))?;

        Ok(yaesu_cmd.encode())
    }

    /// Translate response to FlexRadio protocol
    fn to_flex(&self, resp: &RadioResponse) -> Result<Vec<u8>, MuxError> {
        let flex_cmd = FlexCommand::from_radio_response(resp)
            .ok_or_else(|| MuxError::TranslationError("cannot translate to FlexRadio".into()))?;

        Ok(flex_cmd.encode())
    }
}

/// Responses that should be forwarded to the amplifier
///
/// Amplifiers typically only care about frequency, mode, and PTT state
/// from the radio.
pub fn should_forward_to_amp(resp: &RadioResponse) -> bool {
    matches!(
        resp,
        RadioResponse::Frequency { .. }
            | RadioResponse::Mode { .. }
            | RadioResponse::Ptt { .. }
            | RadioResponse::Status { .. }
    )
}

/// Filter responses to only those relevant for amplifiers
///
/// Returns the response if it should be forwarded to an amplifier,
/// or None if it should be filtered out. Status responses with
/// frequency information are converted to simple Frequency responses.
pub fn filter_for_amplifier(resp: &RadioResponse) -> Option<RadioResponse> {
    match resp {
        // Always forward frequency info
        RadioResponse::Frequency { .. } => Some(resp.clone()),

        // Forward mode info
        RadioResponse::Mode { .. } => Some(resp.clone()),

        // Forward PTT
        RadioResponse::Ptt { .. } => Some(resp.clone()),

        // Extract frequency from status reports
        RadioResponse::Status {
            frequency_hz: Some(hz),
            ..
        } => Some(RadioResponse::Frequency { hz: *hz }),

        // Don't forward VFO changes, ID, unknown responses, etc.
        _ => None,
    }
}

/// Alias for filter_for_amplifier - used by engine.rs
pub fn filter_response_for_amplifier(resp: &RadioResponse) -> Option<RadioResponse> {
    filter_for_amplifier(resp)
}

/// Translate a RadioResponse to the target protocol bytes
///
/// This is a convenience function that creates a translator and translates
/// a single response. For multiple translations, use ProtocolTranslator directly.
pub fn translate_response(resp: &RadioResponse, protocol: Protocol) -> Result<Vec<u8>, MuxError> {
    match protocol {
        Protocol::Kenwood => KenwoodCommand::from_radio_response(resp)
            .map(|cmd| cmd.encode())
            .ok_or_else(|| MuxError::TranslationError("cannot translate to Kenwood".into())),
        Protocol::Elecraft => ElecraftCommand::from_radio_response(resp)
            .map(|cmd| cmd.encode())
            .ok_or_else(|| MuxError::TranslationError("cannot translate to Elecraft".into())),
        Protocol::IcomCIV => CivCommand::from_radio_response(resp)
            .map(|cmd| {
                // Use default address for standalone translation
                let cmd = CivCommand::new(0x00, CONTROLLER_ADDR, cmd.command);
                cmd.encode()
            })
            .ok_or_else(|| MuxError::TranslationError("cannot translate to CI-V".into())),
        Protocol::Yaesu => YaesuCommand::from_radio_response(resp)
            .map(|cmd| cmd.encode())
            .ok_or_else(|| MuxError::TranslationError("cannot translate to Yaesu".into())),
        Protocol::YaesuAscii => YaesuAsciiCommand::from_radio_response(resp)
            .map(|cmd| cmd.encode())
            .ok_or_else(|| MuxError::TranslationError("cannot translate to Yaesu ASCII".into())),
        Protocol::FlexRadio => FlexCommand::from_radio_response(resp)
            .map(|cmd| cmd.encode())
            .ok_or_else(|| MuxError::TranslationError("cannot translate to FlexRadio".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translate_frequency_kenwood_to_icom() {
        let translator = ProtocolTranslator::new(Protocol::IcomCIV);

        let resp = RadioResponse::Frequency { hz: 14_250_000 };
        let result = translator.translate_response(&resp).unwrap();

        // Should be a valid CI-V frame
        assert_eq!(result[0], 0xFE);
        assert_eq!(result[1], 0xFE);
        assert_eq!(result[result.len() - 1], 0xFD);
    }

    #[test]
    fn test_translate_ptt_to_kenwood() {
        let translator = ProtocolTranslator::new(Protocol::Kenwood);

        let resp = RadioResponse::Ptt { active: true };
        let result = translator.translate_response(&resp).unwrap();

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
        let resp = RadioResponse::Frequency { hz: 14_250_123 };
        let result = translator.translate_response(&resp).unwrap();

        // Check the frequency in the encoded result
        let s = String::from_utf8_lossy(&result);
        assert!(s.contains("14250100"), "Expected 14250100, got {}", s);
    }

    #[test]
    fn test_should_forward() {
        assert!(should_forward_to_amp(&RadioResponse::Frequency {
            hz: 14_250_000
        }));
        assert!(should_forward_to_amp(&RadioResponse::Ptt { active: true }));
        assert!(!should_forward_to_amp(&RadioResponse::Id {
            id: "IC-7300".to_string()
        }));
        assert!(!should_forward_to_amp(&RadioResponse::Vfo {
            vfo: cat_protocol::Vfo::A
        }));
    }
}
