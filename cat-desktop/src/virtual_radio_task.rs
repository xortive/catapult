//! Virtual Radio Task
//!
//! This module provides an async task for handling virtual/simulated radios.
//! Moving virtual radio processing to async tasks ensures parity with COM radios
//! and keeps the UI thread responsive.

use std::sync::mpsc::Sender;

use cat_mux::{MuxActorCommand, RadioHandle};
use cat_protocol::{
    icom::CivCodec, kenwood::KenwoodCodec, yaesu::YaesuCodec, Protocol, ProtocolCodec,
    RadioCommand, ToRadioCommand,
};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::info;

use crate::app::BackgroundMessage;

/// Commands that can be sent to a virtual radio task
#[derive(Debug)]
pub enum VirtualRadioCommand {
    /// Process outgoing data from simulation (radio -> mux)
    SimulationOutput(Vec<u8>),
    /// Process command sent to radio (mux -> radio)
    CommandSent(Vec<u8>),
    /// Shutdown the task
    Shutdown,
}

/// Run the virtual radio task
///
/// This handles all async processing for a virtual radio, including:
/// - Parsing incoming data from the simulation context
/// - Sending commands to the mux actor
/// - Handling UI-initiated frequency/mode/PTT changes
pub async fn run_virtual_radio_task(
    mut cmd_rx: tokio_mpsc::Receiver<VirtualRadioCommand>,
    handle: RadioHandle,
    sim_id: String,
    protocol: Protocol,
    mux_tx: tokio_mpsc::Sender<MuxActorCommand>,
    _bg_tx: Sender<BackgroundMessage>,
) {
    info!(
        "Virtual radio task starting for {} (handle {}, protocol {:?})",
        sim_id, handle.0, protocol
    );

    loop {
        match cmd_rx.recv().await {
            Some(VirtualRadioCommand::SimulationOutput(data)) => {
                // Send raw data to mux actor for traffic monitoring
                let _ = mux_tx
                    .send(MuxActorCommand::RadioRawData {
                        handle,
                        data: data.clone(),
                    })
                    .await;

                // Parse and send commands to mux actor
                for cmd in parse_radio_data(&data, protocol) {
                    let _ = mux_tx
                        .send(MuxActorCommand::RadioCommand {
                            handle,
                            command: cmd,
                        })
                        .await;
                }
            }

            Some(VirtualRadioCommand::CommandSent(data)) => {
                // Send raw data to mux actor for traffic monitoring (outgoing)
                let _ = mux_tx
                    .send(MuxActorCommand::RadioRawDataOut { handle, data })
                    .await;
            }

            Some(VirtualRadioCommand::Shutdown) | None => {
                break;
            }
        }
    }

    info!(
        "Virtual radio task shutting down for {} (handle {})",
        sim_id, handle.0
    );
}

/// Parse raw radio data into RadioCommands (moved from app.rs)
fn parse_radio_data(data: &[u8], protocol: Protocol) -> Vec<RadioCommand> {
    let mut commands = Vec::new();
    match protocol {
        Protocol::Kenwood | Protocol::Elecraft => {
            let mut codec = KenwoodCodec::new();
            codec.push_bytes(data);
            while let Some(cmd) = codec.next_command() {
                commands.push(cmd.to_radio_command());
            }
        }
        Protocol::IcomCIV => {
            let mut codec = CivCodec::new();
            codec.push_bytes(data);
            while let Some(cmd) = codec.next_command() {
                commands.push(cmd.to_radio_command());
            }
        }
        Protocol::Yaesu | Protocol::YaesuAscii => {
            let mut codec = YaesuCodec::new();
            codec.push_bytes(data);
            while let Some(cmd) = codec.next_command() {
                commands.push(cmd.to_radio_command());
            }
        }
        Protocol::FlexRadio => {
            // FlexRadio uses Kenwood-style commands
            let mut codec = KenwoodCodec::new();
            codec.push_bytes(data);
            while let Some(cmd) = codec.next_command() {
                commands.push(cmd.to_radio_command());
            }
        }
    }
    commands
}
