//! Async serial I/O handling for radio connections
//!
//! This module provides non-blocking async serial communication using tokio_serial.
//! Each radio connection runs in its own spawned task, communicating with the UI
//! thread via channels.
//!
//! Radio commands are sent directly to the multiplexer actor through mux_tx,
//! ensuring that both real and virtual radios use the same code path.

use std::sync::mpsc::Sender;
use std::time::Duration;

use cat_protocol::{
    elecraft::ElecraftCommand, flex::FlexCommand, icom::CivCodec, icom::CivCommand,
    kenwood::KenwoodCodec, kenwood::KenwoodCommand, yaesu::YaesuCodec,
    yaesu_ascii::YaesuAsciiCommand, EncodeCommand, FromRadioCommand, Protocol, ProtocolCodec,
    RadioCommand, RadioDatabase, ToRadioCommand,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc as tokio_mpsc;
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tracing::{debug, info, warn};

use cat_mux::{MuxActorCommand, RadioHandle};

use crate::app::BackgroundMessage;

/// Boxed protocol codec for async operations
enum ProtocolCodecBox {
    Kenwood(KenwoodCodec),
    Icom(CivCodec),
    Yaesu(YaesuCodec),
}

impl ProtocolCodecBox {
    fn new(protocol: Protocol) -> Self {
        match protocol {
            Protocol::Kenwood | Protocol::Elecraft => Self::Kenwood(KenwoodCodec::new()),
            Protocol::IcomCIV => Self::Icom(CivCodec::new()),
            Protocol::Yaesu | Protocol::YaesuAscii => Self::Yaesu(YaesuCodec::new()),
            Protocol::FlexRadio => {
                // FlexRadio uses Ethernet/TCP, not serial - use Kenwood as fallback
                Self::Kenwood(KenwoodCodec::new())
            }
        }
    }

    fn push_bytes(&mut self, data: &[u8]) {
        match self {
            Self::Kenwood(c) => c.push_bytes(data),
            Self::Icom(c) => c.push_bytes(data),
            Self::Yaesu(c) => c.push_bytes(data),
        }
    }

    fn next_command(&mut self) -> Option<RadioCommand> {
        match self {
            Self::Kenwood(c) => c.next_command().map(|cmd| cmd.to_radio_command()),
            Self::Icom(c) => c.next_command().map(|cmd| cmd.to_radio_command()),
            Self::Yaesu(c) => c.next_command().map(|cmd| cmd.to_radio_command()),
        }
    }
}

/// Commands that can be sent to an async radio connection task
#[derive(Debug)]
pub enum RadioTaskCommand {
    /// Shutdown the task
    Shutdown,
}

/// Async radio connection that runs in a spawned task
pub struct AsyncRadioConnection {
    handle: RadioHandle,
    port_name: String,
    stream: SerialStream,
    protocol: Protocol,
    codec: ProtocolCodecBox,
    tx: Sender<BackgroundMessage>,
    mux_tx: tokio_mpsc::Sender<MuxActorCommand>,
    buffer: Vec<u8>,
    civ_address: Option<u8>,
}

impl AsyncRadioConnection {
    /// Create a new async radio connection
    pub fn connect(
        handle: RadioHandle,
        port_name: &str,
        baud_rate: u32,
        protocol: Protocol,
        tx: Sender<BackgroundMessage>,
        mux_tx: tokio_mpsc::Sender<MuxActorCommand>,
    ) -> Result<Self, tokio_serial::Error> {
        let stream = tokio_serial::new(port_name, baud_rate)
            .timeout(Duration::from_millis(100))
            .open_native_async()?;

        let codec = ProtocolCodecBox::new(protocol);

        Ok(Self {
            handle,
            port_name: port_name.to_string(),
            stream,
            protocol,
            codec,
            tx,
            mux_tx,
            buffer: vec![0u8; 1024],
            civ_address: None,
        })
    }

    /// Set the CI-V address for Icom radios
    pub fn set_civ_address(&mut self, addr: u8) {
        self.civ_address = Some(addr);
    }

    /// Encode a command for the ID query
    fn encode_id_command(&self) -> Option<Vec<u8>> {
        let id_cmd = RadioCommand::GetId;
        match self.protocol {
            Protocol::Kenwood => KenwoodCommand::from_radio_command(&id_cmd).map(|c| c.encode()),
            Protocol::Elecraft => ElecraftCommand::from_radio_command(&id_cmd).map(|c| c.encode()),
            Protocol::FlexRadio => FlexCommand::from_radio_command(&id_cmd).map(|c| c.encode()),
            Protocol::YaesuAscii => {
                YaesuAsciiCommand::from_radio_command(&id_cmd).map(|c| c.encode())
            }
            Protocol::IcomCIV | Protocol::Yaesu => {
                // Icom and legacy Yaesu don't use ASCII ID command
                None
            }
        }
    }

    /// Encode a RadioCommand to protocol-specific bytes
    fn encode_radio_command(&self, cmd: &RadioCommand) -> Option<Vec<u8>> {
        match self.protocol {
            Protocol::Kenwood => KenwoodCommand::from_radio_command(cmd).map(|c| c.encode()),
            Protocol::Elecraft => ElecraftCommand::from_radio_command(cmd).map(|c| c.encode()),
            Protocol::FlexRadio => FlexCommand::from_radio_command(cmd).map(|c| c.encode()),
            Protocol::IcomCIV => {
                let addr = self.civ_address.unwrap_or(0x94);
                CivCommand::from_radio_command(cmd).map(|c| {
                    CivCommand::new(addr, cat_protocol::icom::CONTROLLER_ADDR, c.command).encode()
                })
            }
            Protocol::Yaesu | Protocol::YaesuAscii => None,
        }
    }

    /// Try to parse an ID response and look up the model name
    fn try_parse_id_response(&self, data: &[u8]) -> Option<String> {
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

    /// Query the radio's ID and return the model name if identified
    pub async fn query_id(&mut self) -> Option<String> {
        let id_cmd = self.encode_id_command()?;

        debug!(
            "Querying ID on radio {:?} with protocol {:?}",
            self.handle, self.protocol
        );

        if self.write(&id_cmd).await.is_err() {
            return None;
        }

        let timeout = Duration::from_millis(500);
        let mut response = Vec::new();

        loop {
            match tokio::time::timeout(timeout, self.stream.read(&mut self.buffer)).await {
                Ok(Ok(n)) if n > 0 => {
                    let data = &self.buffer[..n];
                    // Send raw data to mux actor for traffic monitoring
                    let _ = self
                        .mux_tx
                        .send(MuxActorCommand::RadioRawData {
                            handle: self.handle,
                            data: data.to_vec(),
                        })
                        .await;
                    response.extend_from_slice(data);
                    if let Some(model) = self.try_parse_id_response(&response) {
                        info!("Identified radio as {}", model);
                        return Some(model);
                    }
                }
                Ok(Ok(_)) => {}
                Ok(Err(_)) | Err(_) => break,
            }
        }
        None
    }

    /// Query the radio's current frequency and mode
    pub async fn query_initial_state(&mut self) -> Result<(), std::io::Error> {
        // Query frequency
        if let Some(data) = self.encode_radio_command(&RadioCommand::GetFrequency) {
            debug!(
                "Querying frequency on radio {:?} with protocol {:?}",
                self.handle, self.protocol
            );
            self.write(&data).await?;
        }

        // Query mode
        if let Some(data) = self.encode_radio_command(&RadioCommand::GetMode) {
            debug!(
                "Querying mode on radio {:?} with protocol {:?}",
                self.handle, self.protocol
            );
            self.write(&data).await?;
        }

        Ok(())
    }

    /// Enable auto-information mode on the radio
    pub async fn enable_auto_info(&mut self) -> Result<(), std::io::Error> {
        let cmd = RadioCommand::EnableAutoInfo { enabled: true };
        if let Some(data) = self.encode_radio_command(&cmd) {
            debug!(
                "Enabling auto-info on radio {:?} with protocol {:?}",
                self.handle, self.protocol
            );
            self.write(&data).await?;
        }
        Ok(())
    }

    /// Write data to the radio
    pub async fn write(&mut self, data: &[u8]) -> Result<(), std::io::Error> {
        self.stream.write_all(data).await?;
        self.stream.flush().await?;

        // Send traffic notification to mux actor
        let _ = self
            .mux_tx
            .send(MuxActorCommand::RadioRawDataOut {
                handle: self.handle,
                data: data.to_vec(),
            })
            .await;

        Ok(())
    }

    /// Main read loop - runs until connection fails, shutdown is requested, or channel closed
    pub async fn run_read_loop(mut self, mut cmd_rx: tokio_mpsc::Receiver<RadioTaskCommand>) {
        info!(
            "Starting read loop for radio {:?} on {}",
            self.handle, self.port_name
        );

        loop {
            tokio::select! {
                // Check for incoming commands
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(RadioTaskCommand::Shutdown) | None => {
                            info!("Shutdown requested for radio {:?}", self.handle);
                            break;
                        }
                    }
                }

                // Read from serial port with timeout
                result = tokio::time::timeout(
                    Duration::from_millis(100),
                    self.stream.read(&mut self.buffer)
                ) => {
                    match result {
                        Ok(Ok(n)) if n > 0 => {
                            let data = &self.buffer[..n];
                            debug!("Read {} bytes from {:?}: {:02X?}", n, self.handle, data);

                            // Send raw data to mux actor for traffic monitoring
                            let _ = self.mux_tx.send(MuxActorCommand::RadioRawData {
                                handle: self.handle,
                                data: data.to_vec(),
                            }).await;

                            // Parse commands and send directly to mux actor
                            self.codec.push_bytes(data);
                            while let Some(cmd) = self.codec.next_command() {
                                // Send directly to mux actor (same path as virtual radios)
                                let _ = self.mux_tx.send(MuxActorCommand::RadioCommand {
                                    handle: self.handle,
                                    command: cmd,
                                }).await;
                            }
                        }
                        Ok(Ok(_)) => {} // 0 bytes
                        Ok(Err(e)) => {
                            warn!("Read error on {:?}: {}", self.handle, e);
                            let _ = self.tx.send(BackgroundMessage::IoError {
                                source: format!("Radio {:?}", self.handle),
                                message: format!("Read error: {}", e),
                            });
                            break; // Exit loop on error
                        }
                        Err(_) => {} // Timeout, continue
                    }
                }
            }
        }

        info!("Read loop ended for radio {:?}", self.handle);
        let _ = self.tx.send(BackgroundMessage::RadioDisconnected {
            handle: self.handle,
        });
    }
}
