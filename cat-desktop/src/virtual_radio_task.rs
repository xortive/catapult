//! Virtual radio actor task
//!
//! This module provides a pure async task that owns a VirtualRadio and communicates
//! via a `DuplexStream`. The task uses a select! loop to:
//! - Read CAT commands from the connection stream and process them
//! - Handle UI commands from a channel for state changes
//! - Write protocol-encoded responses back to the stream

use std::io;

use cat_protocol::{create_radio_codec, OperatingMode, RadioModel};
use cat_sim::VirtualRadio;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Commands that can be sent from the UI to a virtual radio actor
#[derive(Debug)]
pub enum VirtualRadioCommand {
    /// Set the radio's frequency in Hz
    SetFrequency(u64),
    /// Set the radio's operating mode
    SetMode(OperatingMode),
    /// Set the radio's PTT state
    SetPtt(bool),
    /// Set the radio's model
    SetModel(Option<RadioModel>),
    /// Shutdown the virtual radio actor
    Shutdown,
}

/// Run the virtual radio actor task
///
/// This task owns the VirtualRadio and processes:
/// 1. CAT commands read from the stream (sent by AsyncRadioConnection)
/// 2. UI commands from the command channel (set frequency, mode, PTT from SimulationPanel)
///
/// Responses are written back to the stream for AsyncRadioConnection to read.
pub async fn run_virtual_radio_task(
    mut stream: DuplexStream,
    mut radio: VirtualRadio,
    mut cmd_rx: mpsc::Receiver<VirtualRadioCommand>,
) -> io::Result<()> {
    let mut codec = create_radio_codec(radio.protocol());
    let mut buf = [0u8; 1024];

    info!(
        "Starting virtual radio task for {} ({})",
        radio.id(),
        radio.protocol().name()
    );

    loop {
        tokio::select! {
            // Read CAT commands from the connection stream
            result = stream.read(&mut buf) => {
                match result {
                    Ok(0) => {
                        debug!("Virtual radio stream closed for {}", radio.id());
                        break;
                    }
                    Ok(n) => {
                        let data = &buf[..n];
                        debug!(
                            "Virtual radio {} received {} bytes: {:02X?}",
                            radio.id(), n, data
                        );

                        // Parse bytes into commands using the codec
                        codec.push_bytes(data);
                        while let Some(cmd) = codec.next_command() {
                            debug!("Virtual radio {} processing command: {:?}", radio.id(), cmd);
                            radio.handle_command(&cmd);
                        }

                        // Write any pending output (responses) to the stream
                        while let Some(output) = radio.take_output() {
                            debug!(
                                "Virtual radio {} sending {} bytes: {:02X?}",
                                radio.id(), output.len(), output
                            );
                            if let Err(e) = stream.write_all(&output).await {
                                warn!("Failed to write to virtual radio stream: {}", e);
                                return Err(e);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Virtual radio {} stream error: {}", radio.id(), e);
                        return Err(e);
                    }
                }
            }

            // Handle UI commands from the channel
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(VirtualRadioCommand::SetFrequency(hz)) => {
                        debug!("Virtual radio {} setting frequency to {} Hz", radio.id(), hz);
                        radio.set_frequency(hz);
                    }
                    Some(VirtualRadioCommand::SetMode(mode)) => {
                        debug!("Virtual radio {} setting mode to {:?}", radio.id(), mode);
                        radio.set_mode(mode);
                    }
                    Some(VirtualRadioCommand::SetPtt(ptt)) => {
                        debug!("Virtual radio {} setting PTT to {}", radio.id(), ptt);
                        radio.set_ptt(ptt);
                    }
                    Some(VirtualRadioCommand::SetModel(model)) => {
                        debug!("Virtual radio {} setting model to {:?}", radio.id(), model);
                        radio.set_model(model);
                    }
                    Some(VirtualRadioCommand::Shutdown) => {
                        info!("Shutdown requested for virtual radio {}", radio.id());
                        break;
                    }
                    None => {
                        debug!("Command channel closed for virtual radio {}", radio.id());
                        break;
                    }
                }

                // Write any auto-info output triggered by state changes
                while let Some(output) = radio.take_output() {
                    debug!(
                        "Virtual radio {} auto-info output {} bytes: {:02X?}",
                        radio.id(), output.len(), output
                    );
                    if let Err(e) = stream.write_all(&output).await {
                        warn!("Failed to write auto-info to virtual radio stream: {}", e);
                        return Err(e);
                    }
                }
            }
        }
    }

    info!("Virtual radio task ended for {}", radio.id());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cat_protocol::Protocol;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn test_virtual_radio_responds_to_get_frequency() {
        // Create duplex streams
        let (mut connection_stream, radio_stream) = tokio::io::duplex(1024);

        // Create the radio and command channel
        let radio = VirtualRadio::new("Test", Protocol::Kenwood);
        let (cmd_tx, cmd_rx) = mpsc::channel(32);

        // Spawn the task
        let task_handle = tokio::spawn(run_virtual_radio_task(radio_stream, radio, cmd_rx));

        // Send a GetFrequency command (Kenwood format: "FA;")
        connection_stream.write_all(b"FA;").await.unwrap();

        // Read the response
        let mut response = vec![0u8; 64];
        tokio::time::timeout(std::time::Duration::from_millis(100), async {
            let n = connection_stream.read(&mut response).await.unwrap();
            response.truncate(n);
        })
        .await
        .unwrap();

        // Should respond with frequency report (FA00014250000;)
        let response_str = String::from_utf8_lossy(&response);
        assert!(response_str.contains("FA"), "Expected FA response, got: {}", response_str);
        assert!(response_str.ends_with(";"), "Expected semicolon terminator");

        // Shutdown
        drop(cmd_tx);
        drop(connection_stream);
        let _ = task_handle.await;
    }

    #[tokio::test]
    async fn test_virtual_radio_ui_command_with_auto_info() {
        // Create duplex streams
        let (mut connection_stream, radio_stream) = tokio::io::duplex(1024);

        // Create the radio with auto_info enabled
        let mut radio = VirtualRadio::new("Test", Protocol::Kenwood);
        radio.set_auto_info(true);
        radio.clear_output(); // Clear the AI enable response

        let (cmd_tx, cmd_rx) = mpsc::channel(32);

        // Spawn the task
        let task_handle = tokio::spawn(run_virtual_radio_task(radio_stream, radio, cmd_rx));

        // Send frequency change via UI command
        cmd_tx.send(VirtualRadioCommand::SetFrequency(7_074_000)).await.unwrap();

        // Read the auto-info response from connection_stream
        let mut response = vec![0u8; 64];
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            connection_stream.read(&mut response),
        )
        .await;

        if let Ok(Ok(n)) = result {
            response.truncate(n);
            let response_str = String::from_utf8_lossy(&response);
            assert!(response_str.contains("FA"), "Expected FA response, got: {}", response_str);
            assert!(response_str.contains("7074000"), "Expected frequency in response");
        }

        // Shutdown
        let _ = cmd_tx.send(VirtualRadioCommand::Shutdown).await;
        let _ = task_handle.await;
    }
}
