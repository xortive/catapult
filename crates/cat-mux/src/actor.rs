//! Multiplexer Actor
//!
//! This module provides an async actor for processing radio commands through
//! the multiplexer. All command processing happens in this actor, keeping the
//! UI thread free for display and input handling.
//!
//! Both real COM radios and virtual radios send commands to this actor through
//! channels, ensuring unified code paths.
//!
//! # Architecture
//!
//! The actor receives commands through a channel and emits events through another.
//! This allows the UI to:
//! - Send control commands (add/remove radios, change settings)
//! - Receive all events (traffic, state changes, errors) through a unified stream
//!
//! # Example
//!
//! ```rust,ignore
//! use cat_mux::actor::{run_mux_actor, MuxActorCommand};
//! use cat_mux::MuxEvent;
//! use tokio::sync::mpsc;
//!
//! let (cmd_tx, cmd_rx) = mpsc::channel(256);
//! let (event_tx, mut event_rx) = mpsc::channel(256);
//!
//! // Spawn the actor
//! tokio::spawn(run_mux_actor(cmd_rx, event_tx));
//!
//! // Send commands and receive events
//! ```

use std::collections::HashMap;

use cat_protocol::{OperatingMode, Protocol, RadioCommand};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

use crate::amplifier::AmplifierChannel;
use crate::channel::RadioChannelMeta;
use crate::codec::ProtocolCodecBox;
use crate::engine::Multiplexer;
use crate::error::MuxError;
use crate::events::MuxEvent;
use crate::state::{AmplifierConfig, RadioHandle, SwitchingMode};

/// Summary of a radio's state for sync purposes
///
/// This is a simplified snapshot of RadioState that can be sent across channels.
#[derive(Debug, Clone)]
pub struct RadioStateSummary {
    /// Current frequency in Hz
    pub frequency_hz: Option<u64>,
    /// Current operating mode
    pub mode: Option<OperatingMode>,
    /// PTT active
    pub ptt: bool,
}

impl RadioStateSummary {
    /// Create from a RadioState
    pub fn from_state(state: &crate::state::RadioState) -> Self {
        Self {
            frequency_hz: state.frequency_hz,
            mode: state.mode,
            ptt: state.ptt,
        }
    }
}

/// Commands sent to the multiplexer actor
#[derive(Debug)]
pub enum MuxActorCommand {
    /// Register a new radio with the multiplexer
    RegisterRadio {
        /// Metadata about the radio to register
        meta: RadioChannelMeta,
        /// Channel to send back the assigned handle
        response: oneshot::Sender<RadioHandle>,
    },

    /// Unregister a radio from the multiplexer
    UnregisterRadio {
        /// Handle of the radio to remove
        handle: RadioHandle,
    },

    /// Process a radio command (from radio data parsing)
    RadioCommand {
        /// Handle of the source radio
        handle: RadioHandle,
        /// The parsed command
        command: RadioCommand,
    },

    /// Raw data received from a radio (emits RadioDataIn event, then parses)
    RadioRawData {
        /// Handle of the source radio
        handle: RadioHandle,
        /// Raw bytes received
        data: Vec<u8>,
    },

    /// Raw data sent to a radio (emits RadioDataOut event)
    RadioRawDataOut {
        /// Handle of the target radio
        handle: RadioHandle,
        /// Raw bytes sent
        data: Vec<u8>,
    },

    /// Raw data received from the amplifier (emits AmpDataIn event)
    AmpRawData {
        /// Raw bytes received
        data: Vec<u8>,
    },

    /// Set the active radio
    SetActiveRadio {
        /// Handle of the radio to make active
        handle: RadioHandle,
    },

    /// Query the state of a specific radio
    QueryRadioState {
        /// Handle of the radio to query
        handle: RadioHandle,
        /// Channel to send back the state (or None if not found)
        response: oneshot::Sender<Option<RadioStateSummary>>,
    },

    /// Update a radio's metadata
    UpdateRadioMeta {
        /// Handle of the radio to update
        handle: RadioHandle,
        /// New display name (if Some)
        name: Option<String>,
    },

    /// Connect an amplifier
    ConnectAmplifier {
        /// The amplifier channel
        channel: AmplifierChannel,
    },

    /// Disconnect the amplifier
    DisconnectAmplifier,

    /// Set the amplifier configuration (protocol, port, etc.)
    SetAmplifierConfig {
        /// Port name
        port: String,
        /// Protocol to use
        protocol: Protocol,
        /// Baud rate
        baud_rate: u32,
        /// CI-V address for Icom
        civ_address: Option<u8>,
    },

    /// Set the switching mode
    SetSwitchingMode {
        /// New switching mode
        mode: SwitchingMode,
    },

    /// Report an error from an async task (emits MuxEvent::Error)
    ReportError {
        /// Source of the error (e.g., "Radio", "Amplifier")
        source: String,
        /// Error message
        message: String,
    },

    /// Shutdown the actor
    Shutdown,
}

/// Internal state for the mux actor
struct MuxActorState {
    /// The multiplexer engine
    multiplexer: Multiplexer,
    /// Registered radio channels (keyed by handle)
    radio_channels: HashMap<RadioHandle, RadioChannelMeta>,
    /// Protocol codecs for parsing raw data (keyed by handle)
    codecs: HashMap<RadioHandle, ProtocolCodecBox>,
    /// Amplifier data sender (for sending translated commands)
    amp_tx: Option<mpsc::Sender<Vec<u8>>>,
    /// Amplifier metadata
    amp_meta: Option<crate::amplifier::AmplifierChannelMeta>,
}

impl MuxActorState {
    fn new() -> Self {
        Self {
            multiplexer: Multiplexer::new(),
            radio_channels: HashMap::new(),
            codecs: HashMap::new(),
            amp_tx: None,
            amp_meta: None,
        }
    }

    fn get_radio_meta(&self, handle: RadioHandle) -> Option<&RadioChannelMeta> {
        self.radio_channels.get(&handle)
    }
}

/// Process a radio command through the multiplexer and emit events
///
/// This helper is used by both the RadioCommand handler (for direct command injection)
/// and the RadioRawData handler (after parsing commands from raw bytes).
async fn process_radio_command(
    state: &mut MuxActorState,
    event_tx: &mpsc::Sender<MuxEvent>,
    handle: RadioHandle,
    command: RadioCommand,
) {
    let Some(meta) = state.get_radio_meta(handle) else {
        warn!("Unknown radio handle {} in process_radio_command", handle.0);
        return;
    };

    debug!(
        "Processing command from radio {} (handle {}): {:?}",
        meta.display_name, handle.0, command
    );

    // Track state changes for event emission
    let old_freq = state
        .multiplexer
        .get_radio(handle)
        .and_then(|r| r.frequency_hz);
    let old_mode = state.multiplexer.get_radio(handle).and_then(|r| r.mode);
    let old_ptt = state.multiplexer.get_radio(handle).map(|r| r.ptt);
    let old_active = state.multiplexer.active_radio();

    // Process through multiplexer
    let amp_data = state.multiplexer.process_radio_command(handle, command);

    // Check for state changes
    let new_freq = state
        .multiplexer
        .get_radio(handle)
        .and_then(|r| r.frequency_hz);
    let new_mode = state.multiplexer.get_radio(handle).and_then(|r| r.mode);
    let new_ptt = state.multiplexer.get_radio(handle).map(|r| r.ptt);
    let new_active = state.multiplexer.active_radio();

    // Emit state change event if anything changed
    let freq_changed = old_freq != new_freq;
    let mode_changed = old_mode != new_mode;
    let ptt_changed = old_ptt != new_ptt;

    if freq_changed || mode_changed || ptt_changed {
        let _ = event_tx
            .send(MuxEvent::RadioStateChanged {
                handle,
                freq: if freq_changed { new_freq } else { None },
                mode: if mode_changed { new_mode } else { None },
                ptt: if ptt_changed { new_ptt } else { None },
            })
            .await;
    }

    // Emit active radio change event if needed
    if old_active != new_active {
        if let Some(to) = new_active {
            let _ = event_tx
                .send(MuxEvent::ActiveRadioChanged {
                    from: old_active,
                    to,
                })
                .await;
        }
    }

    // Send to amplifier if there's data
    if let Some(data) = amp_data {
        let amp_protocol = state.multiplexer.amplifier_config().protocol;

        // Emit traffic event for data going to amplifier
        let _ = event_tx
            .send(MuxEvent::AmpDataOut {
                data: data.clone(),
                protocol: amp_protocol,
            })
            .await;

        // Send to amplifier if connected
        if let Some(ref tx) = state.amp_tx {
            if let Err(e) = tx.send(data).await {
                warn!("Failed to send to amplifier: {}", e);
                let _ = event_tx
                    .send(MuxEvent::Error {
                        source: "Amplifier".to_string(),
                        message: format!("Send failed: {}", e),
                    })
                    .await;
            }
        }
    }
}

/// Run the multiplexer actor
///
/// This async function processes all radio commands through the multiplexer
/// and emits events through the event channel.
///
/// # Arguments
///
/// * `cmd_rx` - Receiver for commands sent to the actor
/// * `event_tx` - Sender for events emitted by the actor
pub async fn run_mux_actor(
    mut cmd_rx: mpsc::Receiver<MuxActorCommand>,
    event_tx: mpsc::Sender<MuxEvent>,
) {
    let mut state = MuxActorState::new();
    info!("Multiplexer actor started");

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            MuxActorCommand::RegisterRadio { meta, response } => {
                let name = meta.display_name.clone();
                let port = meta
                    .port_name
                    .clone()
                    .unwrap_or_else(|| "[VRT]".to_string());
                let protocol = meta.protocol;

                // Add to multiplexer
                let handle = state
                    .multiplexer
                    .add_radio(name.clone(), port.clone(), protocol);

                // Store the channel metadata
                state.radio_channels.insert(handle, meta.clone());

                // Create codec for parsing raw data
                state.codecs.insert(handle, ProtocolCodecBox::new(protocol));

                // Send back the handle
                let _ = response.send(handle);

                // Emit event
                let _ = event_tx
                    .send(MuxEvent::RadioConnected { handle, meta })
                    .await;

                info!("Registered radio: {} (handle {})", name, handle.0);
            }

            MuxActorCommand::UnregisterRadio { handle } => {
                if let Some(meta) = state.radio_channels.remove(&handle) {
                    state.multiplexer.remove_radio(handle);
                    state.codecs.remove(&handle);

                    // Emit event
                    let _ = event_tx.send(MuxEvent::RadioDisconnected { handle }).await;

                    info!(
                        "Unregistered radio: {} (handle {})",
                        meta.display_name, handle.0
                    );
                }
            }

            MuxActorCommand::RadioCommand { handle, command } => {
                // Direct command injection - useful for testing and virtual radios
                process_radio_command(&mut state, &event_tx, handle, command).await;
            }

            MuxActorCommand::SetActiveRadio { handle } => {
                let old_active = state.multiplexer.active_radio();

                match state.multiplexer.select_radio(handle) {
                    Ok(()) => {
                        if old_active != Some(handle) {
                            let _ = event_tx
                                .send(MuxEvent::ActiveRadioChanged {
                                    from: old_active,
                                    to: handle,
                                })
                                .await;
                        }
                    }
                    Err(MuxError::SwitchingLocked {
                        requested,
                        current,
                        remaining_ms,
                    }) => {
                        let _ = event_tx
                            .send(MuxEvent::SwitchingBlocked {
                                requested,
                                current,
                                remaining_ms,
                            })
                            .await;
                    }
                    Err(e) => {
                        warn!("Failed to select radio {}: {}", handle.0, e);
                        let _ = event_tx
                            .send(MuxEvent::Error {
                                source: "Multiplexer".to_string(),
                                message: format!("Select failed: {}", e),
                            })
                            .await;
                    }
                }
            }

            MuxActorCommand::QueryRadioState { handle, response } => {
                let summary = state
                    .multiplexer
                    .get_radio(handle)
                    .map(RadioStateSummary::from_state);
                let _ = response.send(summary);
            }

            MuxActorCommand::UpdateRadioMeta { handle, name } => {
                if let Some(new_name) = name {
                    state.multiplexer.rename_radio(handle, new_name.clone());

                    if let Some(meta) = state.radio_channels.get_mut(&handle) {
                        meta.display_name = new_name.clone();
                    }

                    info!("Updated radio {} name to: {}", handle.0, new_name);
                }
            }

            MuxActorCommand::ConnectAmplifier { channel } => {
                state.amp_tx = Some(channel.command_tx);
                state.amp_meta = Some(channel.meta.clone());

                let _ = event_tx
                    .send(MuxEvent::AmpConnected { meta: channel.meta })
                    .await;

                info!("Amplifier connected");
            }

            MuxActorCommand::DisconnectAmplifier => {
                state.amp_tx = None;
                state.amp_meta = None;

                let _ = event_tx.send(MuxEvent::AmpDisconnected).await;

                info!("Amplifier disconnected");
            }

            MuxActorCommand::SetAmplifierConfig {
                port,
                protocol,
                baud_rate,
                civ_address,
            } => {
                let config = AmplifierConfig {
                    port,
                    protocol,
                    baud_rate,
                    civ_address,
                };
                state.multiplexer.set_amplifier_config(config);
                info!("Updated amplifier config");
            }

            MuxActorCommand::SetSwitchingMode { mode } => {
                state.multiplexer.set_switching_mode(mode);

                let _ = event_tx.send(MuxEvent::SwitchingModeChanged { mode }).await;

                info!("Set switching mode to {:?}", mode);
            }

            MuxActorCommand::RadioRawData { handle, data } => {
                // Look up protocol for this radio
                let protocol = state
                    .get_radio_meta(handle)
                    .map(|m| m.protocol)
                    .unwrap_or(cat_protocol::Protocol::Kenwood);

                // Emit traffic event
                let _ = event_tx
                    .send(MuxEvent::RadioDataIn {
                        handle,
                        data: data.clone(),
                        protocol,
                    })
                    .await;

                // Parse commands from raw data using the codec
                // Collect commands first to avoid borrow conflict with state
                let commands: Vec<_> = if let Some(codec) = state.codecs.get_mut(&handle) {
                    codec.push_bytes(&data);
                    std::iter::from_fn(|| codec.next_command()).collect()
                } else {
                    debug!(
                        "No codec found for radio {} (handle {}), skipping parse",
                        handle.0, handle.0
                    );
                    Vec::new()
                };

                // Process all complete commands
                for command in commands {
                    process_radio_command(&mut state, &event_tx, handle, command).await;
                }
            }

            MuxActorCommand::RadioRawDataOut { handle, data } => {
                // Look up protocol for this radio
                let protocol = state
                    .get_radio_meta(handle)
                    .map(|m| m.protocol)
                    .unwrap_or(cat_protocol::Protocol::Kenwood);

                // Emit traffic event
                let _ = event_tx
                    .send(MuxEvent::RadioDataOut {
                        handle,
                        data,
                        protocol,
                    })
                    .await;
            }

            MuxActorCommand::AmpRawData { data } => {
                // Get amplifier protocol
                let protocol = state.multiplexer.amplifier_config().protocol;

                // Emit traffic event
                let _ = event_tx.send(MuxEvent::AmpDataIn { data, protocol }).await;
            }

            MuxActorCommand::Shutdown => {
                info!("Multiplexer actor shutting down");
                break;
            }

            MuxActorCommand::ReportError { source, message } => {
                let _ = event_tx.send(MuxEvent::Error { source, message }).await;
            }
        }
    }

    info!("Multiplexer actor stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::RadioChannelMeta;

    #[tokio::test]
    async fn test_register_radio() {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = mpsc::channel(16);

        // Start actor
        let actor_handle = tokio::spawn(run_mux_actor(cmd_rx, event_tx));

        // Create radio metadata
        let meta = RadioChannelMeta::new_virtual(
            "Test Radio".to_string(),
            "sim-001".to_string(),
            Protocol::Kenwood,
        );

        // Register the radio
        let (resp_tx, resp_rx) = oneshot::channel();
        cmd_tx
            .send(MuxActorCommand::RegisterRadio {
                meta,
                response: resp_tx,
            })
            .await
            .unwrap();

        // Get the handle
        let handle = resp_rx.await.unwrap();
        assert_eq!(handle.0, 1);

        // Check for event
        let event = event_rx.recv().await.unwrap();
        match event {
            MuxEvent::RadioConnected { handle: h, meta } => {
                assert_eq!(h.0, 1);
                assert_eq!(meta.display_name, "Test Radio");
            }
            _ => panic!("Expected RadioConnected event"),
        }

        // Shutdown
        cmd_tx.send(MuxActorCommand::Shutdown).await.unwrap();
        actor_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_radio_state_changes() {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = mpsc::channel(16);

        // Start actor
        let actor_handle = tokio::spawn(run_mux_actor(cmd_rx, event_tx));

        // Register a radio
        let meta =
            RadioChannelMeta::new_virtual("Test".to_string(), "sim".to_string(), Protocol::Kenwood);

        let (resp_tx, resp_rx) = oneshot::channel();
        cmd_tx
            .send(MuxActorCommand::RegisterRadio {
                meta,
                response: resp_tx,
            })
            .await
            .unwrap();
        let handle = resp_rx.await.unwrap();

        // Drain the connected event
        let _ = event_rx.recv().await;

        // Send a frequency command
        cmd_tx
            .send(MuxActorCommand::RadioCommand {
                handle,
                command: RadioCommand::SetFrequency { hz: 14_250_000 },
            })
            .await
            .unwrap();

        // Should get a state change event
        let event = event_rx.recv().await.unwrap();
        match event {
            MuxEvent::RadioStateChanged {
                handle: h, freq, ..
            } => {
                assert_eq!(h, handle);
                assert_eq!(freq, Some(14_250_000));
            }
            _ => panic!("Expected RadioStateChanged event"),
        }

        cmd_tx.send(MuxActorCommand::Shutdown).await.unwrap();
        actor_handle.await.unwrap();
    }
}
