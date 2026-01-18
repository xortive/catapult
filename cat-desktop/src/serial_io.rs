//! Serial I/O handling for radio and amplifier connections

use std::io::{Read, Write};
use std::sync::mpsc::Sender;
use std::time::Duration;

use cat_mux::RadioHandle;
use cat_protocol::{
    icom::CivCodec, kenwood::KenwoodCodec, yaesu::YaesuCodec, Protocol, ProtocolCodec,
    RadioCommand, ToRadioCommand,
};
use serialport::SerialPort;
use tracing::{debug, warn};

use crate::app::BackgroundMessage;

/// Serial connection to a radio
#[allow(dead_code)]
pub struct RadioConnection {
    /// Radio handle
    handle: RadioHandle,
    /// Serial port
    port: Box<dyn SerialPort>,
    /// Protocol codec
    codec: ProtocolCodecBox,
    /// Message sender
    tx: Sender<BackgroundMessage>,
    /// Read buffer
    buffer: Vec<u8>,
}

/// Boxed protocol codec
#[allow(dead_code)]
enum ProtocolCodecBox {
    Kenwood(KenwoodCodec),
    Icom(CivCodec),
    Yaesu(YaesuCodec),
}

#[allow(dead_code)]
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
            Protocol::Yaesu => ProtocolCodecBox::Yaesu(YaesuCodec::new()),
            Protocol::FlexRadio => {
                // FlexRadio uses Ethernet/TCP, not serial - use Kenwood as fallback
                ProtocolCodecBox::Kenwood(KenwoodCodec::new())
            }
        };

        Ok(Self {
            handle,
            port,
            codec,
            tx,
            buffer: vec![0; 256],
        })
    }

    /// Read and process incoming data
    pub fn poll(&mut self) -> Option<RadioCommand> {
        match self.port.read(&mut self.buffer) {
            Ok(n) if n > 0 => {
                let data = &self.buffer[..n];
                debug!("Read {} bytes from radio {:?}", n, self.handle);

                // Send traffic notification
                let _ = self.tx.send(BackgroundMessage::TrafficIn {
                    radio: self.handle,
                    data: data.to_vec(),
                });

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
        let _ = self.tx.send(BackgroundMessage::TrafficOut {
            data: data.to_vec(),
        });

        Ok(())
    }

    /// Read response from amplifier (if any)
    #[allow(dead_code)]
    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize, std::io::Error> {
        self.port.read(buffer)
    }
}
