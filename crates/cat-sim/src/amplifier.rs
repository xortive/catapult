//! Virtual amplifier for testing
//!
//! This module provides a simulated amplifier that tracks frequency/mode state
//! and can echo or respond to commands. Useful for testing multiplexer logic
//! without real hardware.

use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use cat_protocol::{create_radio_codec, OperatingMode, Protocol, RadioCodec};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tracing::error;

/// Virtual amplifier for testing
///
/// Tracks frequency/mode state and can echo or respond to commands.
/// Useful for testing multiplexer logic without real hardware.
///
/// Implements `AsyncRead + AsyncWrite` so virtual amplifiers can use the same
/// generic amp task as real serial ports:
/// - `AsyncWrite::poll_write`: Pushes bytes through codec, processes complete
///   commands via the virtual amplifier, and buffers any responses
/// - `AsyncRead::poll_read`: Drains the response buffer
pub struct VirtualAmplifier {
    protocol: Protocol,
    civ_address: Option<u8>,
    frequency_hz: u64,
    mode: OperatingMode,
    /// Commands received (for test verification)
    received_commands: Vec<Vec<u8>>,
    /// Codec for parsing incoming data
    codec: Box<dyn RadioCodec>,
    /// Buffer for responses to be read
    response_buffer: VecDeque<u8>,
}

impl VirtualAmplifier {
    /// Create a new virtual amplifier
    pub fn new(protocol: Protocol, civ_address: Option<u8>) -> Self {
        Self {
            protocol,
            civ_address,
            frequency_hz: 14_250_000,
            mode: OperatingMode::Usb,
            received_commands: Vec::new(),
            codec: create_radio_codec(protocol),
            response_buffer: VecDeque::new(),
        }
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

    /// Process a command sent to the amplifier
    ///
    /// Updates internal state based on the command and optionally returns
    /// a response. Stores the command for test verification.
    pub fn process_command(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        self.received_commands.push(data.to_vec());

        // Parse and update state for frequency/mode commands
        match self.protocol {
            Protocol::Kenwood | Protocol::Elecraft => self.process_kenwood_command(data),
            Protocol::IcomCIV => self.process_icom_command(data),
            // These protocols are not yet supported for amplifier simulation
            Protocol::Yaesu | Protocol::YaesuAscii | Protocol::FlexRadio => {
                error!("Virtual Amp doesn't support protocol: {:?}", self.protocol);
                None
            }
        }
    }

    /// Process a Kenwood-style command
    fn process_kenwood_command(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        // Simple parsing for frequency commands like "FA14250000;"
        if data.starts_with(b"FA") && data.ends_with(b";") {
            if let Ok(freq_str) = std::str::from_utf8(&data[2..data.len() - 1]) {
                if let Ok(freq) = freq_str.parse::<u64>() {
                    self.frequency_hz = freq;
                }
            }
        }
        // Mode commands like "MD1;" (USB)
        if data.starts_with(b"MD") && data.ends_with(b";") && data.len() == 4 {
            if let Some(mode) = Self::kenwood_mode_from_byte(data[2]) {
                self.mode = mode;
            }
        }
        None // Virtual amp doesn't need to respond
    }

    /// Process an Icom CI-V command
    fn process_icom_command(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        // CI-V frames: FE FE <to> <from> <cmd> [<sub>] [<data>] FD
        if data.len() < 6 || data[0] != 0xFE || data[1] != 0xFE {
            return None;
        }

        // Find the terminator
        let fd_pos = data.iter().position(|&b| b == 0xFD)?;
        if fd_pos < 5 {
            return None;
        }

        let cmd = data[4];

        // Command 0x00 or 0x05 with sub-command 0x00 = set frequency
        if cmd == 0x00 || (cmd == 0x05 && data.get(5) == Some(&0x00)) {
            // BCD-encoded frequency follows
            // For simplicity, we'll parse 5-byte BCD frequency
            let freq_start = if cmd == 0x05 { 6 } else { 5 };
            if let Some(freq) = Self::parse_icom_bcd_frequency(&data[freq_start..fd_pos]) {
                self.frequency_hz = freq;
            }
        }

        // Command 0x01 or 0x06 = set mode
        if cmd == 0x01 || cmd == 0x06 {
            let mode_byte = data.get(5)?;
            if let Some(mode) = Self::icom_mode_from_byte(*mode_byte) {
                self.mode = mode;
            }
        }

        None
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

impl AsyncRead for VirtualAmplifier {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // Drain from response buffer if available
        if !self.response_buffer.is_empty() {
            let to_read = buf.remaining().min(self.response_buffer.len());
            for _ in 0..to_read {
                if let Some(byte) = self.response_buffer.pop_front() {
                    buf.put_slice(&[byte]);
                }
            }
            return Poll::Ready(Ok(()));
        }

        // No data available - return Pending (will never wake since virtual amp
        // only generates responses in poll_write, but that's fine for our use case
        // where we always poll both read and write together with timeout)
        Poll::Pending
    }
}

impl AsyncWrite for VirtualAmplifier {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        // Push bytes through the codec
        self.codec.push_bytes(buf);

        // Process all complete commands
        while let Some(_cmd) = self.codec.next_command() {
            // The VirtualAmplifier.process_command expects raw bytes, not RadioCommand
            // So we pass the original buffer through for processing
            // Note: This is a simplification - in practice the virtual amp processes
            // the raw bytes directly for state tracking
        }

        // Process raw bytes directly through the virtual amplifier
        if let Some(response) = self.process_command(buf) {
            self.response_buffer.extend(response);
        }

        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtual_amplifier_kenwood_frequency() {
        let mut amp = VirtualAmplifier::new(Protocol::Kenwood, None);

        amp.process_command(b"FA14250000;");
        assert_eq!(amp.frequency_hz(), 14_250_000);

        amp.process_command(b"FA07150000;");
        assert_eq!(amp.frequency_hz(), 7_150_000);
    }

    #[test]
    fn test_virtual_amplifier_kenwood_mode() {
        let mut amp = VirtualAmplifier::new(Protocol::Kenwood, None);

        amp.process_command(b"MD1;");
        assert_eq!(amp.mode(), OperatingMode::Lsb);

        amp.process_command(b"MD2;");
        assert_eq!(amp.mode(), OperatingMode::Usb);

        amp.process_command(b"MD3;");
        assert_eq!(amp.mode(), OperatingMode::Cw);
    }

    #[test]
    fn test_virtual_amplifier_tracks_commands() {
        let mut amp = VirtualAmplifier::new(Protocol::Kenwood, None);

        amp.process_command(b"FA14250000;");
        amp.process_command(b"MD2;");

        assert_eq!(amp.received_commands().len(), 2);
        assert_eq!(amp.received_commands()[0], b"FA14250000;");
        assert_eq!(amp.received_commands()[1], b"MD2;");

        amp.clear_received();
        assert!(amp.received_commands().is_empty());
    }
}
