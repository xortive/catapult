//! Amplifier channel types for multiplexer connections
//!
//! This module defines the metadata and channel structures for connecting
//! amplifiers to the multiplexer. Supports both real (COM port) and virtual
//! amplifiers.

use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use cat_protocol::{create_radio_codec, OperatingMode, Protocol, RadioCodec};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

/// Type of amplifier connection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmplifierType {
    /// Real hardware amplifier connected via COM/serial port
    Real,
    /// Virtual/simulated amplifier for testing
    Virtual,
}

/// Metadata for an amplifier channel
#[derive(Debug, Clone)]
pub struct AmplifierChannelMeta {
    /// Protocol used to communicate with this amplifier
    pub protocol: Protocol,
    /// Whether this is a real or virtual amplifier
    pub amp_type: AmplifierType,
    /// Serial port name (for real amplifiers)
    pub port_name: Option<String>,
    /// CI-V address (for Icom amplifiers)
    pub civ_address: Option<u8>,
    /// Baud rate for serial communication
    pub baud_rate: u32,
}

impl AmplifierChannelMeta {
    /// Create metadata for a real amplifier
    pub fn new_real(
        port_name: String,
        protocol: Protocol,
        baud_rate: u32,
        civ_address: Option<u8>,
    ) -> Self {
        Self {
            protocol,
            amp_type: AmplifierType::Real,
            port_name: Some(port_name),
            civ_address,
            baud_rate,
        }
    }

    /// Create metadata for a virtual amplifier
    pub fn new_virtual(protocol: Protocol, civ_address: Option<u8>) -> Self {
        Self {
            protocol,
            amp_type: AmplifierType::Virtual,
            port_name: None,
            civ_address,
            baud_rate: 0, // Not used for virtual
        }
    }

    /// Check if this is a virtual/simulated amplifier
    pub fn is_simulated(&self) -> bool {
        self.amp_type == AmplifierType::Virtual
    }
}

/// Bidirectional amplifier channel
///
/// The multiplexer sends translated commands to the amplifier through `command_tx`,
/// and receives responses through `response_rx`.
pub struct AmplifierChannel {
    /// Metadata about this amplifier
    pub meta: AmplifierChannelMeta,
    /// Sender for commands to the amplifier (mux -> amp)
    pub command_tx: mpsc::Sender<Vec<u8>>,
    /// Receiver for responses from the amplifier (amp -> mux)
    pub response_rx: mpsc::Receiver<Vec<u8>>,
}

impl std::fmt::Debug for AmplifierChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AmplifierChannel")
            .field("meta", &self.meta)
            .field("command_tx", &"<sender>")
            .field("response_rx", &"<receiver>")
            .finish()
    }
}

impl AmplifierChannel {
    /// Create a new amplifier channel
    pub fn new(
        meta: AmplifierChannelMeta,
        command_tx: mpsc::Sender<Vec<u8>>,
        response_rx: mpsc::Receiver<Vec<u8>>,
    ) -> Self {
        Self {
            meta,
            command_tx,
            response_rx,
        }
    }
}

/// Virtual amplifier for testing
///
/// Tracks frequency/mode state and can echo or respond to commands.
/// Useful for testing multiplexer logic without real hardware.
pub struct VirtualAmplifier {
    protocol: Protocol,
    civ_address: Option<u8>,
    frequency_hz: u64,
    mode: OperatingMode,
    /// Commands received (for test verification)
    received_commands: Vec<Vec<u8>>,
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

        // Parse and update state based on protocol
        // For now, just track what was received - parsing can be added as needed
        match self.protocol {
            Protocol::Kenwood | Protocol::Elecraft => self.process_kenwood_command(data),
            Protocol::IcomCIV => self.process_icom_command(data),
            _ => None,
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

/// Async I/O wrapper around VirtualAmplifier
///
/// Implements `AsyncRead + AsyncWrite` so virtual amplifiers can use the same
/// generic amp task as real serial ports.
///
/// - `AsyncWrite::poll_write`: Pushes bytes through codec, processes complete
///   commands via the virtual amplifier, and buffers any responses
/// - `AsyncRead::poll_read`: Drains the response buffer
pub struct VirtualAmplifierIo {
    amplifier: VirtualAmplifier,
    codec: Box<dyn RadioCodec>,
    response_buffer: VecDeque<u8>,
}

impl VirtualAmplifierIo {
    /// Create a new VirtualAmplifierIo
    pub fn new(protocol: Protocol, civ_address: Option<u8>) -> Self {
        Self {
            amplifier: VirtualAmplifier::new(protocol, civ_address),
            codec: create_radio_codec(protocol),
            response_buffer: VecDeque::new(),
        }
    }

    /// Get a reference to the underlying VirtualAmplifier
    pub fn amplifier(&self) -> &VirtualAmplifier {
        &self.amplifier
    }

    /// Get a mutable reference to the underlying VirtualAmplifier
    pub fn amplifier_mut(&mut self) -> &mut VirtualAmplifier {
        &mut self.amplifier
    }
}

impl AsyncRead for VirtualAmplifierIo {
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

impl AsyncWrite for VirtualAmplifierIo {
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
        if let Some(response) = self.amplifier.process_command(buf) {
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

/// Create a channel pair for a virtual amplifier
///
/// Returns (AmplifierChannel for mux, Sender for sending responses, Receiver for getting commands)
pub fn create_virtual_amp_channel(
    protocol: Protocol,
    civ_address: Option<u8>,
    buffer_size: usize,
) -> (
    AmplifierChannel,
    mpsc::Sender<Vec<u8>>,
    mpsc::Receiver<Vec<u8>>,
) {
    let (cmd_tx, cmd_rx) = mpsc::channel(buffer_size);
    let (resp_tx, resp_rx) = mpsc::channel(buffer_size);

    let meta = AmplifierChannelMeta::new_virtual(protocol, civ_address);
    let channel = AmplifierChannel::new(meta, cmd_tx, resp_rx);

    (channel, resp_tx, cmd_rx)
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

    #[test]
    fn test_amplifier_meta_real() {
        let meta = AmplifierChannelMeta::new_real(
            "/dev/ttyUSB1".to_string(),
            Protocol::Kenwood,
            38400,
            None,
        );

        assert_eq!(meta.amp_type, AmplifierType::Real);
        assert!(!meta.is_simulated());
        assert_eq!(meta.port_name, Some("/dev/ttyUSB1".to_string()));
        assert_eq!(meta.baud_rate, 38400);
    }

    #[test]
    fn test_amplifier_meta_virtual() {
        let meta = AmplifierChannelMeta::new_virtual(Protocol::IcomCIV, Some(0x94));

        assert_eq!(meta.amp_type, AmplifierType::Virtual);
        assert!(meta.is_simulated());
        assert!(meta.port_name.is_none());
        assert_eq!(meta.civ_address, Some(0x94));
    }
}
