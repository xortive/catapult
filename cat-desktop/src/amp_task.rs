//! Amplifier Task
//!
//! This module provides async tasks for communicating with amplifiers.
//! Supports both physical amplifiers over serial ports and virtual/simulated amplifiers.

use std::time::Duration;

use cat_mux::{MuxActorCommand, VirtualAmplifier};
use cat_protocol::Protocol;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc as tokio_mpsc;
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tracing::{debug, error, info};

/// Commands that can be sent to the amplifier task
#[derive(Debug)]
pub enum AmpTaskCommand {
    /// Shutdown the task
    Shutdown,
}

/// Run the async amplifier task
///
/// This handles all async I/O with the amplifier serial port.
/// Errors are reported through the mux actor via ReportError command.
pub async fn run_amp_task(
    cmd_rx: tokio_mpsc::Receiver<AmpTaskCommand>,
    data_rx: tokio_mpsc::Receiver<Vec<u8>>,
    port_name: String,
    baud_rate: u32,
    mux_tx: tokio_mpsc::Sender<MuxActorCommand>,
) {
    info!("Amplifier task starting on {} @ {}", port_name, baud_rate);

    // Open the serial port
    let stream = match tokio_serial::new(&port_name, baud_rate)
        .timeout(Duration::from_millis(100))
        .open_native_async()
    {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to open serial port {port_name}: {e}");
            return;
        }
    };

    info!("Amplifier connected on {}", port_name);

    run_amp_loop(stream, cmd_rx, data_rx, mux_tx).await;

    info!("Amplifier task shutting down");
}

/// Inner loop for amplifier communication
async fn run_amp_loop(
    mut stream: SerialStream,
    mut cmd_rx: tokio_mpsc::Receiver<AmpTaskCommand>,
    mut data_rx: tokio_mpsc::Receiver<Vec<u8>>,
    mux_tx: tokio_mpsc::Sender<MuxActorCommand>,
) {
    let mut buffer = vec![0u8; 256];

    loop {
        tokio::select! {
            // Check for commands (shutdown)
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(AmpTaskCommand::Shutdown) | None => {
                        break;
                    }
                }
            }

            // Check for data to write (from mux actor)
            Some(data) = data_rx.recv() => {
                if let Err(e) = stream.write_all(&data).await {
                    let _ = mux_tx.send(MuxActorCommand::ReportError {
                        source: "Amplifier".to_string(),
                        message: format!("Write error: {}", e),
                    }).await;
                } else {
                    let _ = stream.flush().await;
                    // Note: AmpDataOut is already emitted by mux actor when it sends data
                }
            }

            // Read from amplifier with timeout
            result = tokio::time::timeout(
                Duration::from_millis(100),
                stream.read(&mut buffer)
            ) => {
                match result {
                    Ok(Ok(n)) if n > 0 => {
                        let data = buffer[..n].to_vec();
                        // Send raw amp data to mux actor for traffic monitoring
                        let _ = mux_tx.send(MuxActorCommand::AmpRawData { data }).await;
                    }
                    Ok(Ok(_)) => {} // 0 bytes
                    Ok(Err(e)) => {
                        let _ = mux_tx.send(MuxActorCommand::ReportError {
                            source: "Amplifier".to_string(),
                            message: format!("Read error: {}", e),
                        }).await;
                        break;
                    }
                    Err(_) => {} // Timeout, continue
                }
            }
        }
    }
}

/// Run the virtual amplifier task
///
/// This handles simulated amplifier communication - it receives commands
/// from the mux actor, processes them through VirtualAmplifier, and
/// emits traffic events for monitoring.
pub async fn run_virtual_amp_task(
    mut cmd_rx: tokio_mpsc::Receiver<AmpTaskCommand>,
    mut data_rx: tokio_mpsc::Receiver<Vec<u8>>,
    protocol: Protocol,
    civ_address: Option<u8>,
    mux_tx: tokio_mpsc::Sender<MuxActorCommand>,
) {
    info!("Virtual amplifier task starting (protocol: {:?})", protocol);

    let mut amp = VirtualAmplifier::new(protocol, civ_address);

    loop {
        tokio::select! {
            // Check for commands (shutdown)
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(AmpTaskCommand::Shutdown) | None => {
                        break;
                    }
                }
            }

            // Check for data to process (from mux actor)
            Some(data) = data_rx.recv() => {
                debug!(
                    "Virtual amp received {} bytes: {:02X?}",
                    data.len(),
                    &data[..data.len().min(16)]
                );

                // Process the command through the virtual amplifier
                if let Some(response) = amp.process_command(&data) {
                    // Send response back to mux actor as amp data in
                    let _ = mux_tx.send(MuxActorCommand::AmpRawData { data: response }).await;
                }

                debug!(
                    "Virtual amp state: freq={} Hz, mode={:?}",
                    amp.frequency_hz(),
                    amp.mode()
                );
            }
        }
    }

    info!("Virtual amplifier task shutting down");
}
