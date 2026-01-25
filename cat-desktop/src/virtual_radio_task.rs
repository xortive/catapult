//! Virtual Radio Task
//!
//! This module provides an async task for handling virtual/simulated radios.
//! Moving virtual radio processing to async tasks ensures parity with COM radios
//! and keeps the UI thread responsive.

use std::sync::mpsc::Sender;

use cat_mux::{MuxActorCommand, RadioHandle};
use cat_protocol::Protocol;
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
                // Send raw data to mux actor (which now handles parsing)
                let _ = mux_tx
                    .send(MuxActorCommand::RadioRawData { handle, data })
                    .await;
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
