//! Serial I/O handling for radio and amplifier connections

use std::io::{Read, Write};
use std::sync::mpsc::Sender;
use std::time::Duration;

use std::time::Instant;

use cat_mux::RadioHandle;
use cat_protocol::{
    elecraft::ElecraftCommand, flex::FlexCommand, icom::CivCodec, icom::CivCommand,
    kenwood::KenwoodCodec, kenwood::KenwoodCommand, yaesu::YaesuCodec,
    yaesu_ascii::YaesuAsciiCommand, EncodeCommand, FromRadioCommand, Protocol, ProtocolCodec,
    RadioCommand, RadioDatabase, ToRadioCommand,
};
use serialport::SerialPort;
use tracing::{debug, info, warn};

use crate::app::BackgroundMessage;

/// Serial connection to a radio
pub struct RadioConnection {
    /// Radio handle
    handle: RadioHandle,
    /// Serial port name (e.g., "/dev/ttyUSB0")
    port_name: String,
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
            port_name: port_name.to_string(),
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

    /// Query the radio's ID and return the model name if identified
    /// This sends ID; and waits for the response with a timeout
    pub fn query_id(&mut self) -> Option<String> {
        // Send ID query
        let id_cmd = RadioCommand::GetId;
        let encoded = match self.protocol {
            Protocol::Kenwood => KenwoodCommand::from_radio_command(&id_cmd).map(|c| c.encode()),
            Protocol::Elecraft => ElecraftCommand::from_radio_command(&id_cmd).map(|c| c.encode()),
            Protocol::FlexRadio => FlexCommand::from_radio_command(&id_cmd).map(|c| c.encode()),
            Protocol::YaesuAscii => {
                YaesuAsciiCommand::from_radio_command(&id_cmd).map(|c| c.encode())
            }
            Protocol::IcomCIV | Protocol::Yaesu => {
                // Icom and legacy Yaesu don't use ASCII ID command
                return None;
            }
        };

        let data = encoded?;

        debug!(
            "Querying ID on radio {:?} with protocol {:?}",
            self.handle, self.protocol
        );

        if self.write(&data).is_err() {
            return None;
        }

        // Poll for response with 500ms timeout
        let timeout = Duration::from_millis(500);
        let start = Instant::now();
        let mut response_buf = Vec::new();

        while start.elapsed() < timeout {
            match self.port.read(&mut self.buffer) {
                Ok(n) if n > 0 => {
                    let data = &self.buffer[..n];

                    // Send traffic notification
                    let _ = self.tx.send(BackgroundMessage::TrafficIn {
                        radio: self.handle,
                        data: data.to_vec(),
                    });

                    response_buf.extend_from_slice(data);

                    // Try to parse an ID response
                    if let Some(model_name) = self.try_parse_id_response(&response_buf) {
                        info!("Identified radio as {}", model_name);
                        return Some(model_name);
                    }
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    // Continue polling
                }
                Err(_) => break,
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        None
    }

    /// Try to parse an ID response and look up the model name
    fn try_parse_id_response(&self, data: &[u8]) -> Option<String> {
        // Look for semicolon-terminated response
        if !data.contains(&b';') {
            return None;
        }

        match self.protocol {
            Protocol::Kenwood => {
                if cat_protocol::kenwood::is_valid_id_response(data) {
                    let id_str =
                        String::from_utf8_lossy(&data[2..data.iter().position(|&b| b == b';')?]);
                    if let Some(model) = RadioDatabase::by_kenwood_id(&id_str) {
                        return Some(model.model);
                    }
                    // Return generic name with ID if not in database
                    return Some(format!("Kenwood (ID{})", id_str));
                }
            }
            Protocol::Elecraft => {
                if let Some(model_name) = cat_protocol::elecraft::is_elecraft_response(data) {
                    if let Some(model) = RadioDatabase::by_elecraft_id(model_name) {
                        return Some(model.model);
                    }
                    return Some(model_name.to_string());
                }
            }
            Protocol::FlexRadio => {
                if cat_protocol::flex::is_valid_id_response(data) {
                    let id_str =
                        String::from_utf8_lossy(&data[2..data.iter().position(|&b| b == b';')?]);
                    if let Some(model) = RadioDatabase::by_flex_id(&id_str) {
                        return Some(model.model);
                    }
                    return Some(format!("FlexRadio (ID{})", id_str));
                }
            }
            Protocol::YaesuAscii => {
                if cat_protocol::yaesu_ascii::is_valid_id_response(data) {
                    let id_str =
                        String::from_utf8_lossy(&data[2..data.iter().position(|&b| b == b';')?]);
                    if let Some(model) = RadioDatabase::by_yaesu_ascii_id(&id_str) {
                        return Some(model.model);
                    }
                    return Some(format!("Yaesu (ID{})", id_str));
                }
            }
            _ => {}
        }

        None
    }

    /// Query the radio's current frequency and mode
    /// This gets the initial state before enabling auto-info
    pub fn query_initial_state(&mut self) -> Result<(), std::io::Error> {
        // Query frequency
        let freq_cmd = RadioCommand::GetFrequency;
        if let Some(data) = self.encode_radio_command(&freq_cmd) {
            debug!(
                "Querying frequency on radio {:?} with protocol {:?}",
                self.handle, self.protocol
            );
            self.write(&data)?;
        }

        // Query mode
        let mode_cmd = RadioCommand::GetMode;
        if let Some(data) = self.encode_radio_command(&mode_cmd) {
            debug!(
                "Querying mode on radio {:?} with protocol {:?}",
                self.handle, self.protocol
            );
            self.write(&data)?;
        }

        Ok(())
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

        // Send traffic notification for outgoing radio commands
        if let Err(e) = self.tx.send(BackgroundMessage::RadioTrafficOut {
            radio: self.handle,
            data: data.to_vec(),
        }) {
            warn!("Radio channel send failed for {:?}: {}", self.handle, e);
        }

        Ok(())
    }

    /// Get the port name
    pub fn port_name(&self) -> &str {
        &self.port_name
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
