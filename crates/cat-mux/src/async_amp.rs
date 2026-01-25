//! Async amplifier connection handling
//!
//! This module provides async I/O for amplifier connections, supporting both
//! physical amplifiers over serial ports and virtual/simulated amplifiers
//! through a single generic implementation.

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc as tokio_mpsc, oneshot};
use tracing::{debug, info};

use crate::{MuxActorCommand, MuxEvent};

/// Async amplifier connection that runs in a spawned task
///
/// Generic over the I/O type to support both real serial ports and virtual amplifiers.
/// For virtual amplifiers, use `VirtualAmplifier` from cat-sim.
pub struct AsyncAmpConnection<T> {
    io: T,
    mux_tx: tokio_mpsc::Sender<MuxActorCommand>,
    event_tx: tokio_mpsc::Sender<MuxEvent>,
}

impl<T> AsyncAmpConnection<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send,
{
    /// Create a new async amplifier connection
    ///
    /// # Arguments
    ///
    /// * `io` - Any type implementing AsyncRead + AsyncWrite (SerialStream, VirtualAmplifier, etc.)
    /// * `mux_tx` - Channel sender for communicating with the mux actor
    /// * `event_tx` - Channel sender for emitting MuxEvents (errors)
    pub fn new(
        io: T,
        mux_tx: tokio_mpsc::Sender<MuxActorCommand>,
        event_tx: tokio_mpsc::Sender<MuxEvent>,
    ) -> Self {
        Self {
            io,
            mux_tx,
            event_tx,
        }
    }

    /// Run the amplifier I/O loop
    ///
    /// This handles all async I/O with the amplifier. Returns when shutdown is
    /// received, the data channel closes, or a fatal error occurs.
    ///
    /// # Arguments
    ///
    /// * `shutdown_rx` - Oneshot receiver for shutdown signal
    /// * `data_rx` - Channel receiver for data to write to the amplifier
    pub async fn run(
        mut self,
        mut shutdown_rx: oneshot::Receiver<()>,
        mut data_rx: tokio_mpsc::Receiver<Vec<u8>>,
    ) {
        info!("Amplifier connection starting");

        let mut buffer = vec![0u8; 256];

        loop {
            tokio::select! {
                // Check for shutdown signal
                _ = &mut shutdown_rx => {
                    break;
                }

                // Check for data to write (from mux actor)
                Some(data) = data_rx.recv() => {
                    debug!("Amp connection writing {} bytes", data.len());
                    if let Err(e) = self.io.write_all(&data).await {
                        let _ = self.event_tx.send(MuxEvent::Error {
                            source: "Amplifier".to_string(),
                            message: format!("Write error: {}", e),
                        }).await;
                    } else {
                        let _ = self.io.flush().await;
                    }
                }

                // Read from amplifier with timeout
                result = tokio::time::timeout(
                    Duration::from_millis(100),
                    self.io.read(&mut buffer)
                ) => {
                    match result {
                        Ok(Ok(n)) if n > 0 => {
                            let data = buffer[..n].to_vec();
                            debug!("Amp connection received {} bytes", n);
                            // Send raw amp data to mux actor for traffic monitoring
                            let _ = self.mux_tx.send(MuxActorCommand::AmpRawData { data }).await;
                        }
                        Ok(Ok(_)) => {} // 0 bytes
                        Ok(Err(e)) => {
                            // For virtual amplifiers, WouldBlock/TimedOut is expected
                            if e.kind() != std::io::ErrorKind::WouldBlock
                                && e.kind() != std::io::ErrorKind::TimedOut
                            {
                                let _ = self.event_tx.send(MuxEvent::Error {
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

        info!("Amplifier connection shutting down");
    }
}
