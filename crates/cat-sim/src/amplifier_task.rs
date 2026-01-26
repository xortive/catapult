//! Virtual amplifier actor task
//!
//! This module provides a pure async task that owns a VirtualAmplifier and communicates
//! via an async stream. The task uses a select! loop to:
//! - Read CAT commands from the connection stream and process them
//! - Handle shutdown commands from a channel
//! - Emit state change events via a broadcast channel

use std::io;
use std::time::Duration;

use cat_protocol::{
    create_radio_codec, EncodeCommand, FromRadioCommand, OperatingMode, Protocol, RadioCommand,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{broadcast, mpsc};
use tokio::time::{interval, Interval};
use tracing::{debug, info, warn};

use crate::VirtualAmplifier;

/// Commands that can be sent to a virtual amplifier actor
#[derive(Debug, Clone)]
pub enum VirtualAmpCommand {
    /// Shutdown the virtual amplifier actor
    Shutdown,
}

/// Behavior mode for virtual amplifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VirtualAmpMode {
    /// Receive state via auto-info (mux pushes updates)
    #[default]
    AutoInfo,
    /// Actively poll the mux for frequency
    Polling,
}

/// Default polling interval in milliseconds
const POLLING_INTERVAL_MS: u64 = 500;

/// State event emitted when virtual amplifier state changes
#[derive(Debug, Clone)]
pub struct VirtualAmpStateEvent {
    /// Current frequency in Hz
    pub frequency_hz: u64,
    /// Current operating mode
    pub mode: OperatingMode,
    /// Current PTT state
    pub ptt: bool,
}

/// Run the virtual amplifier actor task
///
/// This task owns the VirtualAmplifier and processes:
/// 1. CAT commands read from the stream (sent by the mux via AsyncAmpConnection)
/// 2. Shutdown commands from the command channel
///
/// State changes are emitted via the broadcast channel for UI subscription.
///
/// The `mode` parameter determines how the virtual amp gets state:
/// - `AutoInfo`: Send AI2 to enable auto-info, receive pushed updates
/// - `Polling`: Actively poll the mux for frequency
pub async fn run_virtual_amp_task<S>(
    mut stream: S,
    mut amp: VirtualAmplifier,
    mut cmd_rx: mpsc::Receiver<VirtualAmpCommand>,
    state_tx: broadcast::Sender<VirtualAmpStateEvent>,
    mode: VirtualAmpMode,
) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut codec = create_radio_codec(amp.protocol());
    let mut buf = [0u8; 1024];

    info!(
        "Starting virtual amplifier task for {} ({})",
        amp.id(),
        amp.protocol().name()
    );

    // Set up based on mode
    let polling_enabled = mode == VirtualAmpMode::Polling;
    let mut poll_timer: Interval = interval(Duration::from_millis(POLLING_INTERVAL_MS));

    // Send auto-info enable command if in AutoInfo mode
    if mode == VirtualAmpMode::AutoInfo {
        if let Some(ai_cmd) = encode_query(
            amp.protocol(),
            amp.civ_address(),
            &RadioCommand::EnableAutoInfo { enabled: true },
        ) {
            debug!(
                "Virtual amp {} sending auto-info enable: {:02X?}",
                amp.id(),
                ai_cmd
            );
            if let Err(e) = stream.write_all(&ai_cmd).await {
                warn!("Failed to send auto-info enable: {}", e);
            } else {
                let _ = stream.flush().await;
            }
        }
    }

    info!("Virtual amp {} running in {:?} mode", amp.id(), mode);

    // Emit initial state
    let _ = state_tx.send(VirtualAmpStateEvent {
        frequency_hz: amp.frequency_hz(),
        mode: amp.mode(),
        ptt: amp.ptt(),
    });

    loop {
        tokio::select! {
            // Read CAT commands from the connection stream
            result = stream.read(&mut buf) => {
                match result {
                    Ok(0) => {
                        debug!("Virtual amplifier stream closed for {}", amp.id());
                        break;
                    }
                    Ok(n) => {
                        let data = &buf[..n];
                        debug!(
                            "Virtual amplifier {} received {} bytes: {:02X?}",
                            amp.id(), n, data
                        );

                        // Parse bytes into commands using the codec
                        codec.push_bytes(data);
                        while let Some(cmd) = codec.next_command() {
                            debug!("Virtual amplifier {} processing command: {:?}", amp.id(), cmd);
                        }

                        // Process raw bytes directly through the virtual amplifier
                        // and emit state change if anything changed
                        if amp.process_command(data) {
                            let event = VirtualAmpStateEvent {
                                frequency_hz: amp.frequency_hz(),
                                mode: amp.mode(),
                                ptt: amp.ptt(),
                            };
                            debug!(
                                "Virtual amplifier {} state changed: freq={}, mode={:?}, ptt={}",
                                amp.id(), event.frequency_hz, event.mode, event.ptt
                            );
                            let _ = state_tx.send(event);
                        }
                    }
                    Err(e) => {
                        warn!("Virtual amplifier {} stream error: {}", amp.id(), e);
                        return Err(e);
                    }
                }
            }

            // Handle commands from the channel
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(VirtualAmpCommand::Shutdown) => {
                        info!("Shutdown requested for virtual amplifier {}", amp.id());
                        break;
                    }
                    None => {
                        debug!("Command channel closed for virtual amplifier {}", amp.id());
                        break;
                    }
                }
            }

            // Polling timer - send frequency query when enabled
            _ = poll_timer.tick(), if polling_enabled => {
                // Amps only care about frequency for band switching
                if let Some(encoded) = encode_query(amp.protocol(), amp.civ_address(), &RadioCommand::GetFrequency) {
                    debug!(
                        "Virtual amp {} polling frequency: {:02X?}",
                        amp.id(), encoded
                    );
                    if let Err(e) = stream.write_all(&encoded).await {
                        warn!("Failed to send frequency poll: {}", e);
                    } else {
                        let _ = stream.flush().await;
                    }
                }
            }
        }
    }

    info!("Virtual amplifier task ended for {}", amp.id());
    Ok(())
}

/// Encode a query command for the given protocol
fn encode_query(
    protocol: Protocol,
    civ_address: Option<u8>,
    query: &RadioCommand,
) -> Option<Vec<u8>> {
    use cat_protocol::icom::CivCommand;
    use cat_protocol::kenwood::KenwoodCommand;

    match protocol {
        Protocol::Kenwood | Protocol::Elecraft => {
            Some(KenwoodCommand::from_radio_command(query)?.encode())
        }
        Protocol::IcomCIV => {
            let civ_cmd = CivCommand::from_radio_command(query)?;
            let to_addr = 0xE0; // Controller/radio address
            let from_addr = civ_address.unwrap_or(0x00); // Amp's CI-V address
            Some(CivCommand::new(to_addr, from_addr, civ_cmd.command).encode())
        }
        Protocol::Yaesu | Protocol::YaesuAscii | Protocol::FlexRadio => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cat_protocol::Protocol;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn test_virtual_amp_processes_frequency_command() {
        // Create duplex streams
        let (mut connection_stream, amp_stream) = tokio::io::duplex(1024);

        // Create the amp and channels
        let amp = VirtualAmplifier::new("Test", Protocol::Kenwood, None);
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (state_tx, mut state_rx) = broadcast::channel(32);

        // Spawn the task in AutoInfo mode
        let task_handle = tokio::spawn(run_virtual_amp_task(
            amp_stream,
            amp,
            cmd_rx,
            state_tx,
            VirtualAmpMode::AutoInfo,
        ));

        // Drain the initial state event
        let initial = state_rx.recv().await.unwrap();
        assert_eq!(initial.frequency_hz, 14_250_000); // Default frequency

        // Send a frequency command
        connection_stream.write_all(b"FA07074000;").await.unwrap();

        // Should get a state change event
        let event = tokio::time::timeout(std::time::Duration::from_millis(100), state_rx.recv())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(event.frequency_hz, 7_074_000);

        // Shutdown
        drop(cmd_tx);
        drop(connection_stream);
        let _ = task_handle.await;
    }

    #[tokio::test]
    async fn test_virtual_amp_emits_ptt_changes() {
        let (mut connection_stream, amp_stream) = tokio::io::duplex(1024);

        let amp = VirtualAmplifier::new("Test", Protocol::Kenwood, None);
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (state_tx, mut state_rx) = broadcast::channel(32);

        let task_handle = tokio::spawn(run_virtual_amp_task(
            amp_stream,
            amp,
            cmd_rx,
            state_tx,
            VirtualAmpMode::AutoInfo,
        ));

        // Drain initial state
        let _ = state_rx.recv().await.unwrap();

        // Send TX command
        connection_stream.write_all(b"TX;").await.unwrap();

        let event = tokio::time::timeout(std::time::Duration::from_millis(100), state_rx.recv())
            .await
            .unwrap()
            .unwrap();

        assert!(event.ptt);

        // Send RX command
        connection_stream.write_all(b"RX;").await.unwrap();

        let event = tokio::time::timeout(std::time::Duration::from_millis(100), state_rx.recv())
            .await
            .unwrap()
            .unwrap();

        assert!(!event.ptt);

        drop(cmd_tx);
        drop(connection_stream);
        let _ = task_handle.await;
    }

    #[tokio::test]
    async fn test_virtual_amp_shutdown_command() {
        let (_connection_stream, amp_stream) = tokio::io::duplex(1024);

        let amp = VirtualAmplifier::new("Test", Protocol::Kenwood, None);
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (state_tx, _state_rx) = broadcast::channel(32);

        let task_handle = tokio::spawn(run_virtual_amp_task(
            amp_stream,
            amp,
            cmd_rx,
            state_tx,
            VirtualAmpMode::AutoInfo,
        ));

        // Send shutdown command
        cmd_tx.send(VirtualAmpCommand::Shutdown).await.unwrap();

        // Task should complete
        let result = tokio::time::timeout(std::time::Duration::from_millis(100), task_handle)
            .await
            .unwrap();

        assert!(result.is_ok());
    }
}
