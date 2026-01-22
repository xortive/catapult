//! Serial I/O handling for radio and amplifier connections

use std::io::{Read, Write};
use std::sync::mpsc::Sender;
use std::time::Duration;

use cat_mux::RadioHandle;
use cat_protocol::{
    elecraft::ElecraftCommand, flex::FlexCommand, icom::CivCodec, icom::CivCommand,
    kenwood::KenwoodCodec, kenwood::KenwoodCommand, yaesu::YaesuCodec, EncodeCommand,
    FromRadioCommand, Protocol, ProtocolCodec, RadioCommand, ToRadioCommand,
};
use serialport::SerialPort;
use tracing::{debug, warn};

use crate::app::BackgroundMessage;

/// Serial connection to a radio
pub struct RadioConnection {
    /// Radio handle
    handle: RadioHandle,
    /// Serial port
    port: Box<dyn SerialPort>,
    /// Protocol being used
    protocol: Protocol,
    /// Protocol codec
    codec: ProtocolCodecBox,
    /// Message sender
    tx: Sender<BackgroundMessage>,
    /// Read buffer
    buffer: Vec<u8>,
    /// CI-V address for Icom radios
    civ_address: Option<u8>,
}

/// Boxed protocol codec
enum ProtocolCodecBox {
    Kenwood(KenwoodCodec),
    Icom(CivCodec),
    Yaesu(YaesuCodec),
}

impl RadioConnection {
    /// Create a new radio connection
    pub fn new(
        handle: RadioHandle,
        port_name: &str,
        baud_rate: u32,
        protocol: Protocol,
        tx: Sender<BackgroundMessage>,
    ) -> Result<Self, serialport::Error> {
        let port = serialport::new(port_name, baud_rate)
            .timeout(Duration::from_millis(100))
            .open()?;

        let codec = match protocol {
            Protocol::Kenwood | Protocol::Elecraft => {
                ProtocolCodecBox::Kenwood(KenwoodCodec::new())
            }
            Protocol::IcomCIV => ProtocolCodecBox::Icom(CivCodec::new()),
            Protocol::Yaesu | Protocol::YaesuAscii => ProtocolCodecBox::Yaesu(YaesuCodec::new()),
            Protocol::FlexRadio => {
                // FlexRadio uses Ethernet/TCP, not serial - use Kenwood as fallback
                ProtocolCodecBox::Kenwood(KenwoodCodec::new())
            }
        };

        Ok(Self {
            handle,
            port,
            protocol,
            codec,
            tx,
            buffer: vec![0; 256],
            civ_address: None,
        })
    }

    /// Set the CI-V address for Icom radios
    pub fn set_civ_address(&mut self, addr: u8) {
        self.civ_address = Some(addr);
    }

    /// Enable auto-information mode on the radio
    /// This causes the radio to send unsolicited updates when parameters change
    pub fn enable_auto_info(&mut self) -> Result<(), std::io::Error> {
        let cmd = RadioCommand::EnableAutoInfo { enabled: true };
        let encoded = self.encode_radio_command(&cmd);

        if let Some(data) = encoded {
            debug!(
                "Enabling auto-info on radio {:?} with protocol {:?}",
                self.handle, self.protocol
            );
            self.write(&data)?;
        }
        Ok(())
    }

    /// Encode a RadioCommand to protocol-specific bytes
    fn encode_radio_command(&self, cmd: &RadioCommand) -> Option<Vec<u8>> {
        match self.protocol {
            Protocol::Kenwood => KenwoodCommand::from_radio_command(cmd).map(|c| c.encode()),
            Protocol::Elecraft => ElecraftCommand::from_radio_command(cmd).map(|c| c.encode()),
            Protocol::FlexRadio => FlexCommand::from_radio_command(cmd).map(|c| c.encode()),
            Protocol::IcomCIV => {
                // For Icom, we need a valid CI-V address
                let addr = self.civ_address.unwrap_or(0x94); // Default to IC-7300
                CivCommand::from_radio_command(cmd).map(|c| {
                    CivCommand::new(addr, cat_protocol::icom::CONTROLLER_ADDR, c.command).encode()
                })
            }
            Protocol::Yaesu | Protocol::YaesuAscii => {
                // Legacy Yaesu binary protocol doesn't support AI
                None
            }
        }
    }

    /// Read and process incoming data
    pub fn poll(&mut self) -> Option<RadioCommand> {
        match self.port.read(&mut self.buffer) {
            Ok(n) if n > 0 => {
                let data = &self.buffer[..n];
                debug!("Read {} bytes from radio {:?}", n, self.handle);

                // Send traffic notification
                if let Err(e) = self.tx.send(BackgroundMessage::TrafficIn {
                    radio: self.handle,
                    data: data.to_vec(),
                }) {
                    warn!("Channel send failed for radio {:?}: {}", self.handle, e);
                }

                // Push to codec and try to parse
                match &mut self.codec {
                    ProtocolCodecBox::Kenwood(c) => {
                        c.push_bytes(data);
                        c.next_command().map(|cmd| cmd.to_radio_command())
                    }
                    ProtocolCodecBox::Icom(c) => {
                        c.push_bytes(data);
                        c.next_command().map(|cmd| cmd.to_radio_command())
                    }
                    ProtocolCodecBox::Yaesu(c) => {
                        c.push_bytes(data);
                        c.next_command().map(|cmd| cmd.to_radio_command())
                    }
                }
            }
            Ok(_) => None,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => None,
            Err(e) => {
                warn!("Error reading from radio {:?}: {}", self.handle, e);
                // Report error to traffic monitor via IoError message
                let _ = self.tx.send(BackgroundMessage::IoError {
                    source: format!("Radio {:?}", self.handle),
                    message: format!("Read error: {}", e),
                });
                None
            }
        }
    }

    /// Write data to the radio
    pub fn write(&mut self, data: &[u8]) -> Result<(), std::io::Error> {
        self.port.write_all(data)?;
        self.port.flush()?;
        Ok(())
    }
}

/// Serial connection to the amplifier
pub struct AmplifierConnection {
    /// Serial port
    port: Box<dyn SerialPort>,
    /// Message sender
    tx: Sender<BackgroundMessage>,
}

impl AmplifierConnection {
    /// Create a new amplifier connection
    pub fn new(
        port_name: &str,
        baud_rate: u32,
        tx: Sender<BackgroundMessage>,
    ) -> Result<Self, serialport::Error> {
        let port = serialport::new(port_name, baud_rate)
            .timeout(Duration::from_millis(100))
            .open()?;

        Ok(Self { port, tx })
    }

    /// Write data to the amplifier
    pub fn write(&mut self, data: &[u8]) -> Result<(), std::io::Error> {
        self.port.write_all(data)?;
        self.port.flush()?;

        // Send traffic notification
        if let Err(e) = self.tx.send(BackgroundMessage::TrafficOut {
            data: data.to_vec(),
        }) {
            warn!("Amplifier channel send failed: {}", e);
        }

        Ok(())
    }

    /// Poll for incoming data from the amplifier
    /// Returns true if data was received
    pub fn poll(&mut self) -> bool {
        let mut buffer = [0u8; 256];
        match self.port.read(&mut buffer) {
            Ok(n) if n > 0 => {
                let data = buffer[..n].to_vec();
                if let Err(e) = self.tx.send(BackgroundMessage::AmpTrafficIn { data }) {
                    warn!("Amplifier channel send failed: {}", e);
                }
                true
            }
            Ok(_) => false,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => false,
            Err(e) => {
                warn!("Amplifier read error: {}", e);
                // Report error to traffic monitor via IoError message
                let _ = self.tx.send(BackgroundMessage::IoError {
                    source: "Amplifier".to_string(),
                    message: format!("Read error: {}", e),
                });
                false
            }
        }
    }
}
