//! Virtual amplifier for testing
//!
//! This module provides a simulated amplifier that tracks frequency/mode state
//! and can echo or respond to commands. Useful for testing multiplexer logic
//! without real hardware.

use cat_protocol::{OperatingMode, Protocol};
use tracing::error;

/// Virtual amplifier for testing
///
/// Tracks frequency/mode/PTT state based on commands received. Used by the
/// virtual amplifier actor task to maintain state that can be reported to the UI.
pub struct VirtualAmplifier {
    /// Identifier for logging
    id: String,
    protocol: Protocol,
    civ_address: Option<u8>,
    frequency_hz: u64,
    mode: OperatingMode,
    ptt: bool,
    /// Commands received (for test verification)
    received_commands: Vec<Vec<u8>>,
}

impl VirtualAmplifier {
    /// Create a new virtual amplifier
    pub fn new(id: impl Into<String>, protocol: Protocol, civ_address: Option<u8>) -> Self {
        Self {
            id: id.into(),
            protocol,
            civ_address,
            frequency_hz: 14_250_000,
            mode: OperatingMode::Usb,
            ptt: false,
            received_commands: Vec::new(),
        }
    }

    /// Get the identifier
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get the protocol
    pub fn protocol(&self) -> Protocol {
        self.protocol
    }

    /// Get the CI-V address
    pub fn civ_address(&self) -> Option<u8> {
        self.civ_address
    }

    /// Get current frequency
    pub fn frequency_hz(&self) -> u64 {
        self.frequency_hz
    }

    /// Get current mode
    pub fn mode(&self) -> OperatingMode {
        self.mode
    }

    /// Get current PTT state
    pub fn ptt(&self) -> bool {
        self.ptt
    }

    /// Process a command sent to the amplifier
    ///
    /// Updates internal state based on the command and returns true if state
    /// changed. Stores the command for test verification.
    pub fn process_command(&mut self, data: &[u8]) -> bool {
        self.received_commands.push(data.to_vec());

        // Parse and update state for frequency/mode/PTT commands
        match self.protocol {
            Protocol::Kenwood | Protocol::Elecraft => self.process_kenwood_command(data),
            Protocol::IcomCIV => self.process_icom_command(data),
            // These protocols are not yet supported for amplifier simulation
            Protocol::Yaesu | Protocol::YaesuAscii | Protocol::FlexRadio => {
                error!("Virtual Amp doesn't support protocol: {:?}", self.protocol);
                false
            }
        }
    }

    /// Process a Kenwood-style command
    ///
    /// Returns true if state changed, false otherwise.
    fn process_kenwood_command(&mut self, data: &[u8]) -> bool {
        let mut changed = false;

        // Simple parsing for frequency commands like "FA14250000;"
        if data.starts_with(b"FA") && data.ends_with(b";") {
            if let Ok(freq_str) = std::str::from_utf8(&data[2..data.len() - 1]) {
                if let Ok(freq) = freq_str.parse::<u64>() {
                    if self.frequency_hz != freq {
                        self.frequency_hz = freq;
                        changed = true;
                    }
                }
            }
        }
        // Mode commands like "MD1;" (USB)
        if data.starts_with(b"MD") && data.ends_with(b";") && data.len() == 4 {
            if let Some(mode) = Self::kenwood_mode_from_byte(data[2]) {
                if self.mode != mode {
                    self.mode = mode;
                    changed = true;
                }
            }
        }
        // PTT commands like "TX;" or "RX;" or "TX0;", "TX1;"
        if data.starts_with(b"TX") && data.ends_with(b";") {
            if !self.ptt {
                self.ptt = true;
                changed = true;
            }
        }
        if data.starts_with(b"RX") && data.ends_with(b";") {
            if self.ptt {
                self.ptt = false;
                changed = true;
            }
        }

        changed
    }

    /// Process an Icom CI-V command
    ///
    /// Returns true if state changed, false otherwise.
    fn process_icom_command(&mut self, data: &[u8]) -> bool {
        // CI-V frames: FE FE <to> <from> <cmd> [<sub>] [<data>] FD
        if data.len() < 6 || data[0] != 0xFE || data[1] != 0xFE {
            return false;
        }

        // Find the terminator
        let Some(fd_pos) = data.iter().position(|&b| b == 0xFD) else {
            return false;
        };
        if fd_pos < 5 {
            return false;
        }

        let cmd = data[4];
        let mut changed = false;

        // Command 0x00 or 0x05 with sub-command 0x00 = set frequency
        if cmd == 0x00 || (cmd == 0x05 && data.get(5) == Some(&0x00)) {
            // BCD-encoded frequency follows
            let freq_start = if cmd == 0x05 { 6 } else { 5 };
            if let Some(freq) = Self::parse_icom_bcd_frequency(&data[freq_start..fd_pos]) {
                if self.frequency_hz != freq {
                    self.frequency_hz = freq;
                    changed = true;
                }
            }
        }

        // Command 0x01 or 0x06 = set mode
        if cmd == 0x01 || cmd == 0x06 {
            if let Some(&mode_byte) = data.get(5) {
                if let Some(mode) = Self::icom_mode_from_byte(mode_byte) {
                    if self.mode != mode {
                        self.mode = mode;
                        changed = true;
                    }
                }
            }
        }

        // Command 0x1C sub 0x00 = PTT control
        if cmd == 0x1C && data.get(5) == Some(&0x00) {
            if let Some(&ptt_byte) = data.get(6) {
                let new_ptt = ptt_byte != 0x00;
                if self.ptt != new_ptt {
                    self.ptt = new_ptt;
                    changed = true;
                }
            }
        }

        changed
    }

    /// Parse BCD-encoded frequency from Icom data
    fn parse_icom_bcd_frequency(data: &[u8]) -> Option<u64> {
        if data.len() < 5 {
            return None;
        }

        // Icom sends frequency as 5 bytes BCD, little-endian
        // Each byte contains two BCD digits
        let mut freq: u64 = 0;
        let mut multiplier: u64 = 1;

        for &byte in &data[..5] {
            let low = (byte & 0x0F) as u64;
            let high = ((byte >> 4) & 0x0F) as u64;
            freq += low * multiplier;
            multiplier *= 10;
            freq += high * multiplier;
            multiplier *= 10;
        }

        Some(freq)
    }

    /// Convert Kenwood mode byte to OperatingMode
    fn kenwood_mode_from_byte(b: u8) -> Option<OperatingMode> {
        match b {
            b'1' => Some(OperatingMode::Lsb),
            b'2' => Some(OperatingMode::Usb),
            b'3' => Some(OperatingMode::Cw),
            b'4' => Some(OperatingMode::Fm),
            b'5' => Some(OperatingMode::Am),
            b'6' => Some(OperatingMode::Dig),
            b'7' => Some(OperatingMode::CwR),
            b'9' => Some(OperatingMode::DigL),
            _ => None,
        }
    }

    /// Convert Icom mode byte to OperatingMode
    fn icom_mode_from_byte(b: u8) -> Option<OperatingMode> {
        match b {
            0x00 => Some(OperatingMode::Lsb),
            0x01 => Some(OperatingMode::Usb),
            0x02 => Some(OperatingMode::Am),
            0x03 => Some(OperatingMode::Cw),
            0x04 => Some(OperatingMode::Rtty),
            0x05 => Some(OperatingMode::Fm),
            0x07 => Some(OperatingMode::CwR),
            0x08 => Some(OperatingMode::RttyR),
            _ => None,
        }
    }

    /// Get all received commands (for test verification)
    pub fn received_commands(&self) -> &[Vec<u8>] {
        &self.received_commands
    }

    /// Clear received commands
    pub fn clear_received(&mut self) {
        self.received_commands.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtual_amplifier_kenwood_frequency() {
        let mut amp = VirtualAmplifier::new("test", Protocol::Kenwood, None);

        amp.process_command(b"FA14250000;");
        assert_eq!(amp.frequency_hz(), 14_250_000);

        amp.process_command(b"FA07150000;");
        assert_eq!(amp.frequency_hz(), 7_150_000);
    }

    #[test]
    fn test_virtual_amplifier_kenwood_mode() {
        let mut amp = VirtualAmplifier::new("test", Protocol::Kenwood, None);

        amp.process_command(b"MD1;");
        assert_eq!(amp.mode(), OperatingMode::Lsb);

        amp.process_command(b"MD2;");
        assert_eq!(amp.mode(), OperatingMode::Usb);

        amp.process_command(b"MD3;");
        assert_eq!(amp.mode(), OperatingMode::Cw);
    }

    #[test]
    fn test_virtual_amplifier_kenwood_ptt() {
        let mut amp = VirtualAmplifier::new("test", Protocol::Kenwood, None);

        assert!(!amp.ptt());

        amp.process_command(b"TX;");
        assert!(amp.ptt());

        amp.process_command(b"RX;");
        assert!(!amp.ptt());
    }

    #[test]
    fn test_virtual_amplifier_tracks_commands() {
        let mut amp = VirtualAmplifier::new("test", Protocol::Kenwood, None);

        amp.process_command(b"FA14250000;");
        amp.process_command(b"MD2;");

        assert_eq!(amp.received_commands().len(), 2);
        assert_eq!(amp.received_commands()[0], b"FA14250000;");
        assert_eq!(amp.received_commands()[1], b"MD2;");

        amp.clear_received();
        assert!(amp.received_commands().is_empty());
    }

    #[test]
    fn test_process_command_returns_true_on_change() {
        let mut amp = VirtualAmplifier::new("test", Protocol::Kenwood, None);
        // Default frequency is 14_250_000

        // Setting same as initial should return false
        assert!(!amp.process_command(b"FA14250000;"));
        // Different value should return true
        assert!(amp.process_command(b"FA07074000;"));
        // Same value should return false
        assert!(!amp.process_command(b"FA07074000;"));
        // Back to different value should return true
        assert!(amp.process_command(b"FA14250000;"));
    }
}
