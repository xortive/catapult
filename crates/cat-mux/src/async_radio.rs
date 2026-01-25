//! Async serial I/O handling for radio connections
//!
//! This module provides non-blocking async serial communication using tokio_serial.
//! Each radio connection runs in its own spawned task, communicating with the
//! multiplexer via channels.
//!
//! Radio commands are sent directly to the multiplexer actor through mux_tx,
//! ensuring that both real and virtual radios use the same code path.
//!
//! ## Virtual Radio Support
//!
//! Virtual radios use `DuplexStream` from `tokio::io::duplex()` connected to
//! a virtual radio actor task.

use std::io::ErrorKind;
use std::time::Duration;

use cat_protocol::{
    elecraft::ElecraftCommand, flex::FlexCommand, icom::CivCommand, kenwood::KenwoodCommand,
    yaesu::YaesuCommand, yaesu_ascii::YaesuAsciiCommand, EncodeCommand, FromRadioCommand, Protocol,
    RadioCommand, RadioDatabase,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc as tokio_mpsc;
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tracing::{debug, info, warn};

use crate::{MuxActorCommand, MuxEvent, RadioHandle};

/// Commands that can be sent to an async radio connection task
#[derive(Debug)]
pub enum RadioTaskCommand {
    /// Shutdown the task
    Shutdown,
}

/// Async radio connection that runs in a spawned task
///
/// Generic over the I/O type to support both real serial ports and virtual radios.
/// For virtual radios, use `DuplexStream` from `tokio::io::duplex()`.
pub struct AsyncRadioConnection<T> {
    handle: RadioHandle,
    port_name: String,
    io: T,
    protocol: Protocol,
    event_tx: tokio_mpsc::Sender<MuxEvent>,
    mux_tx: tokio_mpsc::Sender<MuxActorCommand>,
    buffer: Vec<u8>,
    civ_address: Option<u8>,
}

impl AsyncRadioConnection<SerialStream> {
    /// Create a new async radio connection to a serial port
    pub fn connect(
        handle: RadioHandle,
        port_name: &str,
        baud_rate: u32,
        protocol: Protocol,
        event_tx: tokio_mpsc::Sender<MuxEvent>,
        mux_tx: tokio_mpsc::Sender<MuxActorCommand>,
    ) -> Result<Self, tokio_serial::Error> {
        let stream = tokio_serial::new(port_name, baud_rate)
            .timeout(Duration::from_millis(100))
            .open_native_async()?;

        Ok(Self {
            handle,
            port_name: port_name.to_string(),
            io: stream,
            protocol,
            event_tx,
            mux_tx,
            buffer: vec![0u8; 1024],
            civ_address: None,
        })
    }
}

impl<T> AsyncRadioConnection<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send,
{
    /// Create a new async radio connection with a custom I/O type
    ///
    /// For virtual radios, use `DuplexStream` from `tokio::io::duplex()`.
    pub fn new(
        handle: RadioHandle,
        name: String,
        io: T,
        protocol: Protocol,
        event_tx: tokio_mpsc::Sender<MuxEvent>,
        mux_tx: tokio_mpsc::Sender<MuxActorCommand>,
    ) -> Self {
        Self {
            handle,
            port_name: name,
            io,
            protocol,
            event_tx,
            mux_tx,
            buffer: vec![0u8; 1024],
            civ_address: None,
        }
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
                    CivCommand::new(cat_protocol::icom::CONTROLLER_ADDR, addr, c.command).encode()
                })
            }
            Protocol::Yaesu => YaesuCommand::from_radio_command(cmd).map(|c| c.encode()),
            Protocol::YaesuAscii => YaesuAsciiCommand::from_radio_command(cmd).map(|c| c.encode()),
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
            match tokio::time::timeout(timeout, self.io.read(&mut self.buffer)).await {
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
        self.io.write_all(data).await?;
        self.io.flush().await?;

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

                // Read from I/O with timeout
                result = tokio::time::timeout(
                    Duration::from_millis(100),
                    self.io.read(&mut self.buffer)
                ) => {
                    match result {
                        Ok(Ok(n)) if n > 0 => {
                            let data = &self.buffer[..n];
                            debug!("Read {} bytes from {:?}: {:02X?}", n, self.handle, data);

                            // Send raw data to mux actor for parsing and processing
                            let _ = self.mux_tx.send(MuxActorCommand::RadioRawData {
                                handle: self.handle,
                                data: data.to_vec(),
                            }).await;
                        }
                        Ok(Ok(_)) => {} // 0 bytes
                        Ok(Err(e)) => {
                            // For virtual radios, WouldBlock just means no data available
                            if e.kind() == ErrorKind::WouldBlock {
                                continue;
                            }
                            // ConnectionAborted means the virtual radio channel was closed - expected behavior
                            if e.kind() == ErrorKind::ConnectionAborted {
                                debug!("Virtual radio channel closed for {:?}", self.handle);
                                break;
                            }
                            warn!("Read error on {:?}: {}", self.handle, e);
                            let _ = self.event_tx.send(MuxEvent::Error {
                                source: format!("Radio {:?}", self.handle),
                                message: format!("Read error: {}", e),
                            }).await;
                            break;
                        }
                        Err(_) => {} // Timeout, continue
                    }
                }
            }
        }

        info!("Read loop ended for radio {:?}", self.handle);
        let _ = self.event_tx.send(MuxEvent::RadioDisconnected {
            handle: self.handle,
        }).await;
    }
}
