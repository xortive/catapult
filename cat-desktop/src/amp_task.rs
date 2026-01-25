//! Amplifier Task
//!
//! This module provides an async task for communicating with amplifiers.
//! Supports both physical amplifiers over serial ports and virtual/simulated amplifiers
//! through a single generic implementation.

use std::time::Duration;

use cat_mux::MuxActorCommand;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc as tokio_mpsc, oneshot};
use tracing::{debug, info};

/// Run the async amplifier task
///
/// This handles all async I/O with the amplifier through a generic I/O type.
/// Works with both real serial ports (SerialStream) and virtual amplifiers (VirtualAmplifierIo).
///
/// # Arguments
///
/// * `shutdown_rx` - Oneshot receiver for shutdown signal
/// * `data_rx` - Channel receiver for data to write to the amplifier
/// * `io` - Any type implementing AsyncRead + AsyncWrite (SerialStream, VirtualAmplifierIo, etc.)
/// * `mux_tx` - Channel sender for communicating back to the mux actor
///
/// # Notes
///
/// - Serial port opening is handled by the caller
/// - For virtual amplifiers, pass a VirtualAmplifierIo instance
pub async fn run_amp_task<T>(
    mut shutdown_rx: oneshot::Receiver<()>,
    mut data_rx: tokio_mpsc::Receiver<Vec<u8>>,
    mut io: T,
    mux_tx: tokio_mpsc::Sender<MuxActorCommand>,
)
where
    T: AsyncRead + AsyncWrite + Unpin + Send,
{
    info!("Amplifier task starting");

    let mut buffer = vec![0u8; 256];

    loop {
        tokio::select! {
            // Check for shutdown signal
            _ = &mut shutdown_rx => {
                break;
            }

            // Check for data to write (from mux actor)
            Some(data) = data_rx.recv() => {
                debug!("Amp task writing {} bytes", data.len());
                if let Err(e) = io.write_all(&data).await {
                    let _ = mux_tx.send(MuxActorCommand::ReportError {
                        source: "Amplifier".to_string(),
                        message: format!("Write error: {}", e),
                    }).await;
                } else {
                    let _ = io.flush().await;
                }
            }

            // Read from amplifier with timeout
            result = tokio::time::timeout(
                Duration::from_millis(100),
                io.read(&mut buffer)
            ) => {
                match result {
                    Ok(Ok(n)) if n > 0 => {
                        let data = buffer[..n].to_vec();
                        debug!("Amp task received {} bytes", n);
                        // Send raw amp data to mux actor for traffic monitoring
                        let _ = mux_tx.send(MuxActorCommand::AmpRawData { data }).await;
                    }
                    Ok(Ok(_)) => {} // 0 bytes
                    Ok(Err(e)) => {
                        // For virtual amplifiers, WouldBlock/TimedOut is expected
                        if e.kind() != std::io::ErrorKind::WouldBlock
                            && e.kind() != std::io::ErrorKind::TimedOut
                        {
                            let _ = mux_tx.send(MuxActorCommand::ReportError {
                                source: "Amplifier".to_string(),
                                message: format!("Read error: {}", e),
                            }).await;
                            break;
                        }
                    }
                    Err(_) => {} // Timeout, continue
                }
            }
        }
    }

    info!("Amplifier task shutting down");
}
