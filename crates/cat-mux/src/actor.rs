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
use std::time::SystemTime;

use cat_protocol::{
    create_radio_codec, OperatingMode, Protocol, RadioCodec, RadioRequest, RadioResponse, Vfo,
};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval, Duration, MissedTickBehavior};
use tracing::{debug, info, warn};

use crate::amplifier::AmplifierChannel;
use crate::async_radio::RadioTaskCommand;
use crate::channel::RadioChannelMeta;
use crate::engine::Multiplexer;
use crate::error::MuxError;
use crate::events::MuxEvent;
use crate::state::{AmplifierConfig, RadioHandle, SwitchingMode};
use crate::translation::translate_response;

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
        /// Optional command channel for sending data to the radio (for AI2 heartbeat)
        cmd_tx: Option<mpsc::Sender<RadioTaskCommand>>,
    },

    /// Unregister a radio from the multiplexer
    UnregisterRadio {
        /// Handle of the radio to remove
        handle: RadioHandle,
    },

    /// Process a radio response (from radio data parsing)
    RadioResponse {
        /// Handle of the source radio
        handle: RadioHandle,
        /// The parsed response
        response: RadioResponse,
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
    codecs: HashMap<RadioHandle, Box<dyn RadioCodec>>,
    /// Command senders for radios (for AI2 heartbeat)
    radio_cmd_tx: HashMap<RadioHandle, mpsc::Sender<RadioTaskCommand>>,
    /// Amplifier data sender (for sending translated commands)
    amp_tx: Option<mpsc::Sender<Vec<u8>>>,
    /// Amplifier metadata
    amp_meta: Option<crate::amplifier::AmplifierChannelMeta>,
    /// Codec for parsing amplifier data
    amp_codec: Option<Box<dyn RadioCodec>>,
    /// Whether auto-info mode is enabled (amp requested updates via AI2)
    auto_info_enabled: bool,
    /// Cached state for responding to amplifier queries
    cached_frequency_hz: Option<u64>,
    cached_mode: Option<OperatingMode>,
    cached_ptt: bool,
    /// Cached control band (0=Main/A, 1=Sub/B) - which VFO has front panel control
    cached_control_band: Option<u8>,
    /// Cached transmit band (0=Main/A, 1=Sub/B) - which VFO is selected for TX
    cached_tx_band: Option<u8>,
    /// Cached RX VFO (0=A, 1=B) - for inferring CB/TB from VFO commands
    cached_rx_vfo: Option<u8>,
    /// Cached split state - for inferring TB from split commands
    cached_split: bool,
}

impl MuxActorState {
    fn new() -> Self {
        Self {
            multiplexer: Multiplexer::new(),
            radio_channels: HashMap::new(),
            codecs: HashMap::new(),
            radio_cmd_tx: HashMap::new(),
            amp_tx: None,
            amp_meta: None,
            amp_codec: None,
            auto_info_enabled: false,
            cached_frequency_hz: None,
            cached_mode: None,
            cached_ptt: false,
            cached_control_band: None,
            cached_tx_band: None,
            cached_rx_vfo: None,
            cached_split: false,
        }
    }

    fn get_radio_meta(&self, handle: RadioHandle) -> Option<&RadioChannelMeta> {
        self.radio_channels.get(&handle)
    }
}

/// Process a radio response through the multiplexer and emit events
///
/// This helper is used by both the RadioResponse handler (for direct response injection)
/// and the RadioRawData handler (after parsing responses from raw bytes).
async fn process_radio_response(
    state: &mut MuxActorState,
    event_tx: &mpsc::Sender<MuxEvent>,
    handle: RadioHandle,
    response: RadioResponse,
) {
    let Some(meta) = state.get_radio_meta(handle) else {
        warn!(
            "Unknown radio handle {} in process_radio_response",
            handle.0
        );
        return;
    };

    debug!(
        "Processing response from radio {} (handle {}): {:?}",
        meta.display_name, handle.0, response
    );

    // Update cached CB/TB state from radio reports (only from active radio)
    if state.multiplexer.active_radio() == Some(handle) {
        match &response {
            RadioResponse::ControlBand { band } => {
                state.cached_control_band = Some(*band);
                debug!("Updated cached control band to {}", band);
            }
            RadioResponse::TransmitBand { band } => {
                state.cached_tx_band = Some(*band);
                debug!("Updated cached transmit band to {}", band);
            }
            // Infer CB/TB from VFO responses (for radios that don't report CB/TB directly)
            RadioResponse::Vfo { vfo } => match vfo {
                Vfo::A => {
                    // VFO A selected - RX on A, control on A
                    state.cached_rx_vfo = Some(0);
                    state.cached_control_band = Some(0);
                    // Selecting VFO A/B clears split mode
                    state.cached_split = false;
                    state.cached_tx_band = Some(0);
                    debug!("VFO A selected: CB=0, TB=0, split=false");
                }
                Vfo::B => {
                    // VFO B selected - RX on B, control on B
                    state.cached_rx_vfo = Some(1);
                    state.cached_control_band = Some(1);
                    // Selecting VFO A/B clears split mode
                    state.cached_split = false;
                    state.cached_tx_band = Some(1);
                    debug!("VFO B selected: CB=1, TB=1, split=false");
                }
                Vfo::Split => {
                    // Split enabled - TX on opposite of current RX VFO
                    state.cached_split = true;
                    let rx = state.cached_rx_vfo.unwrap_or(0);
                    state.cached_tx_band = Some(1 - rx); // Opposite of RX
                                                         // CB stays as current RX VFO
                    debug!(
                        "Split enabled: CB={}, TB={} (RX on {}, TX on opposite)",
                        state.cached_control_band.unwrap_or(0),
                        state.cached_tx_band.unwrap_or(1),
                        rx
                    );
                }
                Vfo::Memory => {
                    // Memory mode - treat as VFO A, no split
                    state.cached_rx_vfo = Some(0);
                    state.cached_control_band = Some(0);
                    state.cached_tx_band = Some(0);
                    state.cached_split = false;
                    debug!("Memory mode: CB=0, TB=0, split=false");
                }
            },
            _ => {}
        }
    }

    // Capture old state with a single lookup
    let (old_freq, old_mode, old_ptt) = state
        .multiplexer
        .get_radio(handle)
        .map(|r| (r.frequency_hz, r.mode, Some(r.ptt)))
        .unwrap_or((None, None, None));
    let old_active = state.multiplexer.active_radio();

    // Process through multiplexer
    let amp_data = state.multiplexer.process_radio_response(handle, &response);

    // Capture new state with a single lookup
    let (new_freq, new_mode, new_ptt) = state
        .multiplexer
        .get_radio(handle)
        .map(|r| (r.frequency_hz, r.mode, Some(r.ptt)))
        .unwrap_or((None, None, None));
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

    // Check if this radio is now the active radio (for auto-info updates)
    let is_active = new_active == Some(handle);

    // Send to amplifier if there's data and auto-info is enabled
    if let Some(data) = amp_data {
        // Only send if auto-info is enabled (amp requested updates via AI2)
        if state.auto_info_enabled {
            let amp_protocol = state.multiplexer.amplifier_config().protocol;

            // Emit traffic event for data going to amplifier
            let _ = event_tx
                .send(MuxEvent::AmpDataOut {
                    data: data.clone(),
                    protocol: amp_protocol,
                    timestamp: SystemTime::now(),
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

        // Always update cached state so we can respond to amp queries
        if let Some(hz) = new_freq {
            state.cached_frequency_hz = Some(hz);
        }
        if let Some(mode) = new_mode {
            state.cached_mode = Some(mode);
        }
        if let Some(ptt) = new_ptt {
            state.cached_ptt = ptt;
        }
    }

    // Send auto-info updates if enabled and this is the active radio
    if is_active && state.auto_info_enabled && state.amp_tx.is_some() {
        // Send unsolicited updates for changed state
        if freq_changed {
            if let Some(hz) = new_freq {
                // Only send if different from what amp already knows
                if state.cached_frequency_hz != Some(hz) {
                    state.cached_frequency_hz = Some(hz);
                    send_to_amp(state, event_tx, RadioResponse::Frequency { hz }).await;
                }
            }
        }
        if mode_changed {
            if let Some(mode) = new_mode {
                if state.cached_mode != Some(mode) {
                    state.cached_mode = Some(mode);
                    send_to_amp(state, event_tx, RadioResponse::Mode { mode }).await;
                }
            }
        }
        if ptt_changed {
            if let Some(ptt) = new_ptt {
                if state.cached_ptt != ptt {
                    state.cached_ptt = ptt;
                    send_to_amp(state, event_tx, RadioResponse::Ptt { active: ptt }).await;
                }
            }
        }
    }
}

/// Handle a query from the amplifier using cached state
///
/// Returns `Some(RadioResponse)` with the response if we can answer,
/// or `None` if we don't have the state to answer (amp should retry later).
fn handle_amp_query(state: &MuxActorState, query: &RadioRequest) -> Option<RadioResponse> {
    match query {
        RadioRequest::GetFrequency => state
            .cached_frequency_hz
            .map(|hz| RadioResponse::Frequency { hz }),

        RadioRequest::GetMode => state.cached_mode.map(|mode| RadioResponse::Mode { mode }),

        RadioRequest::GetPtt => Some(RadioResponse::Ptt {
            active: state.cached_ptt,
        }),

        RadioRequest::GetAutoInfo => Some(RadioResponse::AutoInfo {
            enabled: state.auto_info_enabled,
        }),

        // Always identify as TS-990S (ID022) to amplifiers
        RadioRequest::GetId => Some(RadioResponse::Id {
            id: "022".to_string(), // TS-990S
        }),

        // Control band query - return cached or default to main (0)
        RadioRequest::GetControlBand => Some(RadioResponse::ControlBand {
            band: state.cached_control_band.unwrap_or(0),
        }),

        // Transmit band query - return cached or default to main (0)
        RadioRequest::GetTransmitBand => Some(RadioResponse::TransmitBand {
            band: state.cached_tx_band.unwrap_or(0),
        }),

        _ => None,
    }
}

/// Send a RadioResponse to the amplifier
///
/// Translates the response to the amplifier's protocol and sends it.
async fn send_to_amp(
    state: &MuxActorState,
    event_tx: &mpsc::Sender<MuxEvent>,
    response: RadioResponse,
) {
    let Some(ref tx) = state.amp_tx else {
        return;
    };

    let protocol = state.multiplexer.amplifier_config().protocol;

    let data = match translate_response(&response, protocol) {
        Ok(d) => d,
        Err(e) => {
            debug!("Cannot translate {:?} to {:?}: {}", response, protocol, e);
            return;
        }
    };

    // Emit traffic event
    let _ = event_tx
        .send(MuxEvent::AmpDataOut {
            data: data.clone(),
            protocol,
            timestamp: SystemTime::now(),
        })
        .await;

    // Send to amplifier
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

    // AI2 heartbeat timer - sends AI2; to all Kenwood/Elecraft radios every second
    let mut ai2_timer = interval(Duration::from_secs(1));
    ai2_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else { break; };
                match cmd {
            MuxActorCommand::RegisterRadio {
                meta,
                response,
                cmd_tx,
            } => {
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
                state.codecs.insert(handle, create_radio_codec(protocol));

                // Store the command channel for AI2 heartbeat
                if let Some(tx) = cmd_tx {
                    state.radio_cmd_tx.insert(handle, tx);
                }

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
                    state.radio_cmd_tx.remove(&handle);

                    // Emit event
                    let _ = event_tx.send(MuxEvent::RadioDisconnected { handle }).await;

                    info!(
                        "Unregistered radio: {} (handle {})",
                        meta.display_name, handle.0
                    );
                }
            }

            MuxActorCommand::RadioResponse { handle, response } => {
                // Direct response injection - useful for testing and virtual radios
                process_radio_response(&mut state, &event_tx, handle, response).await;
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

                            // If auto-info is enabled, send new radio's state to amplifier
                            if state.auto_info_enabled && state.amp_tx.is_some() {
                                if let Some(radio) = state.multiplexer.get_radio(handle) {
                                    // Update and send frequency
                                    if let Some(hz) = radio.frequency_hz {
                                        state.cached_frequency_hz = Some(hz);
                                        send_to_amp(
                                            &state,
                                            &event_tx,
                                            RadioResponse::Frequency { hz },
                                        )
                                        .await;
                                    }
                                    // Update and send mode
                                    if let Some(mode) = radio.mode {
                                        state.cached_mode = Some(mode);
                                        send_to_amp(&state, &event_tx, RadioResponse::Mode { mode })
                                            .await;
                                    }
                                    // Update and send PTT
                                    state.cached_ptt = radio.ptt;
                                    send_to_amp(
                                        &state,
                                        &event_tx,
                                        RadioResponse::Ptt { active: radio.ptt },
                                    )
                                    .await;
                                }
                            }
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
                // Reset codec and cached state for new connection
                state.amp_codec = None;
                state.auto_info_enabled = false;
                state.cached_frequency_hz = None;
                state.cached_mode = None;
                state.cached_ptt = false;
                state.cached_control_band = None;
                state.cached_tx_band = None;
                state.cached_rx_vfo = None;
                state.cached_split = false;

                let _ = event_tx
                    .send(MuxEvent::AmpConnected { meta: channel.meta })
                    .await;

                info!("Amplifier connected");
            }

            MuxActorCommand::DisconnectAmplifier => {
                state.amp_tx = None;
                state.amp_meta = None;
                state.amp_codec = None;
                state.auto_info_enabled = false;
                state.cached_frequency_hz = None;
                state.cached_mode = None;
                state.cached_ptt = false;
                state.cached_control_band = None;
                state.cached_tx_band = None;
                state.cached_rx_vfo = None;
                state.cached_split = false;

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
                // Log raw bytes at DEBUG level for diagnostics
                let port_name = state
                    .get_radio_meta(handle)
                    .and_then(|m| m.port_name.clone())
                    .unwrap_or_else(|| format!("handle={}", handle.0));
                debug!(
                    "IN  <-Radio({}) {:02X?}",
                    port_name,
                    &data[..data.len().min(64)]
                );

                // Look up protocol for this radio
                let protocol = state
                    .get_radio_meta(handle)
                    .map(|m| m.protocol)
                    .unwrap_or(cat_protocol::Protocol::Kenwood);

                // Parse responses from raw data using the codec
                // Emit traffic event for EACH response with its specific bytes
                let responses_with_bytes: Vec<_> =
                    if let Some(codec) = state.codecs.get_mut(&handle) {
                        codec.push_bytes(&data);
                        std::iter::from_fn(|| codec.next_response_with_bytes()).collect()
                    } else {
                        debug!(
                            "No codec found for radio {} (handle {}), skipping parse",
                            handle.0, handle.0
                        );
                        Vec::new()
                    };

                // Process each complete response and emit its traffic event
                for (response, raw_bytes) in responses_with_bytes {
                    // Emit traffic event with just this response's bytes
                    let _ = event_tx
                        .send(MuxEvent::RadioDataIn {
                            handle,
                            data: raw_bytes,
                            protocol,
                            timestamp: SystemTime::now(),
                        })
                        .await;

                    process_radio_response(&mut state, &event_tx, handle, response).await;
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
                        timestamp: SystemTime::now(),
                    })
                    .await;
            }

            MuxActorCommand::AmpRawData { data } => {
                // Get amplifier protocol
                let protocol = state.multiplexer.amplifier_config().protocol;

                // Create codec if not exists
                if state.amp_codec.is_none() {
                    state.amp_codec = Some(create_radio_codec(protocol));
                }

                // Parse requests from amplifier data
                // Emit traffic event for EACH request with its specific bytes
                let requests_with_bytes: Vec<_> = if let Some(codec) = state.amp_codec.as_mut() {
                    codec.push_bytes(&data);
                    std::iter::from_fn(|| codec.next_request_with_bytes()).collect()
                } else {
                    Vec::new()
                };

                // Process each request from the amplifier
                for (req, raw_bytes) in requests_with_bytes {
                    // Emit traffic event with just this request's bytes
                    let _ = event_tx
                        .send(MuxEvent::AmpDataIn {
                            data: raw_bytes,
                            protocol,
                            timestamp: SystemTime::now(),
                        })
                        .await;

                    debug!("Amp sent request: {:?}", req);

                    // Handle based on request type - queries get responses, sets are actions
                    if req.is_query() {
                        // Respond to queries from cached state
                        if let Some(response) = handle_amp_query(&state, &req) {
                            debug!("Responding to amp query {:?} with {:?}", req, response);
                            send_to_amp(&state, &event_tx, response).await;
                        } else {
                            debug!("No cached state to respond to amp query {:?}", req);
                        }
                    } else if let RadioRequest::SetAutoInfo { enabled } = req {
                        // Handle auto-info enable/disable
                        state.auto_info_enabled = enabled;
                        debug!("Amp auto-info mode set to {}", enabled);

                        // If auto-info just enabled, send current state
                        if enabled {
                            if let Some(hz) = state.cached_frequency_hz {
                                send_to_amp(&state, &event_tx, RadioResponse::Frequency { hz })
                                    .await;
                            }
                            if let Some(mode) = state.cached_mode {
                                send_to_amp(&state, &event_tx, RadioResponse::Mode { mode }).await;
                            }
                        }
                    }
                }
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
            _ = ai2_timer.tick() => {
                send_ai2_heartbeat(&mut state).await;
            }
        }
    }

    info!("Multiplexer actor stopped");
}

/// Send AI2; heartbeat to all connected Kenwood/Elecraft radios
///
/// This ensures auto-info mode stays enabled even if a radio restarts.
async fn send_ai2_heartbeat(state: &mut MuxActorState) {
    let ai2_bytes = b"AI2;".to_vec();

    for (handle, tx) in &state.radio_cmd_tx {
        // Only send to Kenwood-compatible protocols
        if let Some(meta) = state.radio_channels.get(handle) {
            if matches!(meta.protocol, Protocol::Kenwood | Protocol::Elecraft) {
                let _ = tx
                    .send(RadioTaskCommand::SendData {
                        data: ai2_bytes.clone(),
                    })
                    .await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amplifier::{AmplifierChannel, AmplifierChannelMeta};
    use crate::channel::RadioChannelMeta;

    /// Create a channel pair for a virtual amplifier (test helper)
    fn create_virtual_amp_channel(
        protocol: Protocol,
        civ_address: Option<u8>,
        buffer_size: usize,
    ) -> (
        AmplifierChannel,
        mpsc::Sender<Vec<u8>>,
        mpsc::Receiver<Vec<u8>>,
    ) {
        let (cmd_tx, cmd_rx) = mpsc::channel(buffer_size);
        let (resp_tx, resp_rx) = mpsc::channel(buffer_size);
        let meta = AmplifierChannelMeta::new_virtual(protocol, civ_address);
        let channel = AmplifierChannel::new(meta, cmd_tx, resp_rx);
        (channel, resp_tx, cmd_rx)
    }

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
                cmd_tx: None,
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
                cmd_tx: None,
            })
            .await
            .unwrap();
        let handle = resp_rx.await.unwrap();

        // Drain the connected event
        let _ = event_rx.recv().await;

        // Send a frequency response
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Frequency { hz: 14_250_000 },
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

    #[tokio::test]
    async fn test_amp_query_responds_with_cached_frequency() {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = mpsc::channel(16);

        let actor_handle = tokio::spawn(run_mux_actor(cmd_rx, event_tx));

        // Register a radio
        let meta =
            RadioChannelMeta::new_virtual("Test".to_string(), "sim".to_string(), Protocol::Kenwood);
        let (resp_tx, resp_rx) = oneshot::channel();
        cmd_tx
            .send(MuxActorCommand::RegisterRadio {
                meta,
                response: resp_tx,
                cmd_tx: None,
            })
            .await
            .unwrap();
        let handle = resp_rx.await.unwrap();

        // Drain the connected event
        let _ = event_rx.recv().await;

        // Connect an amplifier using the helper
        let (amp_channel, _resp_tx, mut amp_rx) =
            create_virtual_amp_channel(Protocol::Kenwood, None, 16);
        cmd_tx
            .send(MuxActorCommand::ConnectAmplifier {
                channel: amp_channel,
            })
            .await
            .unwrap();

        // Drain the amp connected event
        let _ = event_rx.recv().await;

        // Set frequency on the radio (this updates the emulated state for queries)
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Frequency { hz: 14_250_000 },
            })
            .await
            .unwrap();

        // Wait for state change event (no AmpDataOut without auto-info enabled)
        loop {
            let event = event_rx.recv().await.unwrap();
            if matches!(event, MuxEvent::RadioStateChanged { .. }) {
                break;
            }
        }

        // Now send a frequency query from the amp (FA;)
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"FA;".to_vec(),
            })
            .await
            .unwrap();

        // Should get AmpDataIn followed by AmpDataOut with response
        loop {
            let event = event_rx.recv().await.unwrap();
            if let MuxEvent::AmpDataOut { data, .. } = event {
                // Should be a frequency report
                let s = String::from_utf8_lossy(&data);
                assert!(
                    s.starts_with("FA") && s.contains("14250000"),
                    "Expected frequency response, got: {}",
                    s
                );
                break;
            }
        }

        // Verify amp received the response
        let amp_data = amp_rx.recv().await.unwrap();
        let s = String::from_utf8_lossy(&amp_data);
        assert!(s.contains("14250000"));

        cmd_tx.send(MuxActorCommand::Shutdown).await.unwrap();
        actor_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_amp_query_no_response_when_no_state() {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = mpsc::channel(16);

        let actor_handle = tokio::spawn(run_mux_actor(cmd_rx, event_tx));

        // Register a radio but don't set any state
        let meta =
            RadioChannelMeta::new_virtual("Test".to_string(), "sim".to_string(), Protocol::Kenwood);
        let (resp_tx, resp_rx) = oneshot::channel();
        cmd_tx
            .send(MuxActorCommand::RegisterRadio {
                meta,
                response: resp_tx,
                cmd_tx: None,
            })
            .await
            .unwrap();
        let _ = resp_rx.await.unwrap();

        // Drain the connected event
        let _ = event_rx.recv().await;

        // Connect an amplifier using the helper
        let (amp_channel, _resp_tx, mut amp_rx) =
            create_virtual_amp_channel(Protocol::Kenwood, None, 16);
        cmd_tx
            .send(MuxActorCommand::ConnectAmplifier {
                channel: amp_channel,
            })
            .await
            .unwrap();

        // Drain the amp connected event
        let _ = event_rx.recv().await;

        // Send a frequency query when no frequency is cached
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"FA;".to_vec(),
            })
            .await
            .unwrap();

        // Should get AmpDataIn but no AmpDataOut (no cached state)
        let event = event_rx.recv().await.unwrap();
        assert!(matches!(event, MuxEvent::AmpDataIn { .. }));

        // Amp should not receive any data (use try_recv to check without blocking)
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        assert!(amp_rx.try_recv().is_err());

        cmd_tx.send(MuxActorCommand::Shutdown).await.unwrap();
        actor_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_auto_info_sends_updates_on_state_change() {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = mpsc::channel(16);

        let actor_handle = tokio::spawn(run_mux_actor(cmd_rx, event_tx));

        // Register a radio
        let meta =
            RadioChannelMeta::new_virtual("Test".to_string(), "sim".to_string(), Protocol::Kenwood);
        let (resp_tx, resp_rx) = oneshot::channel();
        cmd_tx
            .send(MuxActorCommand::RegisterRadio {
                meta,
                response: resp_tx,
                cmd_tx: None,
            })
            .await
            .unwrap();
        let handle = resp_rx.await.unwrap();
        let _ = event_rx.recv().await; // Drain connected event

        // Connect an amplifier using the helper
        let (amp_channel, _resp_tx, mut amp_rx) =
            create_virtual_amp_channel(Protocol::Kenwood, None, 16);
        cmd_tx
            .send(MuxActorCommand::ConnectAmplifier {
                channel: amp_channel,
            })
            .await
            .unwrap();
        let _ = event_rx.recv().await; // Drain amp connected event

        // Enable auto-info mode (AI2; in Kenwood)
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"AI2;".to_vec(),
            })
            .await
            .unwrap();
        let _ = event_rx.recv().await; // Drain AmpDataIn

        // Give it time to process
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Now change frequency - should trigger an unsolicited update
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Frequency { hz: 7_074_000 },
            })
            .await
            .unwrap();

        // Should get multiple events: state change, amp data out (for forwarding),
        // and amp data out (for auto-info)
        let mut found_auto_info_update = false;
        for _ in 0..10 {
            if let Ok(event) =
                tokio::time::timeout(tokio::time::Duration::from_millis(100), event_rx.recv()).await
            {
                if let Some(MuxEvent::AmpDataOut { data, .. }) = event {
                    let s = String::from_utf8_lossy(&data);
                    if s.contains("7074000") {
                        found_auto_info_update = true;
                    }
                }
            } else {
                break;
            }
        }

        assert!(
            found_auto_info_update,
            "Expected auto-info frequency update"
        );

        // Verify amp received the update
        let mut found_in_amp = false;
        while let Ok(data) = amp_rx.try_recv() {
            let s = String::from_utf8_lossy(&data);
            if s.contains("7074000") {
                found_in_amp = true;
            }
        }
        assert!(found_in_amp, "Amp should have received frequency update");

        cmd_tx.send(MuxActorCommand::Shutdown).await.unwrap();
        actor_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_no_unsolicited_updates_without_auto_info() {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = mpsc::channel(16);

        let actor_handle = tokio::spawn(run_mux_actor(cmd_rx, event_tx));

        // Register a radio
        let meta =
            RadioChannelMeta::new_virtual("Test".to_string(), "sim".to_string(), Protocol::Kenwood);
        let (resp_tx, resp_rx) = oneshot::channel();
        cmd_tx
            .send(MuxActorCommand::RegisterRadio {
                meta,
                response: resp_tx,
                cmd_tx: None,
            })
            .await
            .unwrap();
        let handle = resp_rx.await.unwrap();
        let _ = event_rx.recv().await; // Drain connected event

        // Connect an amplifier - but do NOT send AI2; to enable auto-info
        let (amp_channel, _resp_tx, mut amp_rx) =
            create_virtual_amp_channel(Protocol::Kenwood, None, 16);
        cmd_tx
            .send(MuxActorCommand::ConnectAmplifier {
                channel: amp_channel,
            })
            .await
            .unwrap();
        let _ = event_rx.recv().await; // Drain amp connected event

        // Set frequency - without auto-info, nothing should be sent to amp
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Frequency { hz: 14_250_000 },
            })
            .await
            .unwrap();

        // Wait for processing
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Count messages received by amp - should be 0 without auto-info
        let mut amp_messages = Vec::new();
        while let Ok(data) = amp_rx.try_recv() {
            amp_messages.push(data);
        }

        // Should have no messages - amp must send AI2 to enable auto-info first
        assert_eq!(
            amp_messages.len(),
            0,
            "Without auto-info enabled, amp should receive no messages. Got {} messages",
            amp_messages.len()
        );

        cmd_tx.send(MuxActorCommand::Shutdown).await.unwrap();
        actor_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_disconnect_resets_emulated_state() {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = mpsc::channel(16);

        let actor_handle = tokio::spawn(run_mux_actor(cmd_rx, event_tx));

        // Register a radio
        let meta =
            RadioChannelMeta::new_virtual("Test".to_string(), "sim".to_string(), Protocol::Kenwood);
        let (resp_tx, resp_rx) = oneshot::channel();
        cmd_tx
            .send(MuxActorCommand::RegisterRadio {
                meta,
                response: resp_tx,
                cmd_tx: None,
            })
            .await
            .unwrap();
        let handle = resp_rx.await.unwrap();
        let _ = event_rx.recv().await;

        // Connect an amplifier using the helper
        let (amp_channel, _resp_tx, _amp_rx) =
            create_virtual_amp_channel(Protocol::Kenwood, None, 16);
        cmd_tx
            .send(MuxActorCommand::ConnectAmplifier {
                channel: amp_channel,
            })
            .await
            .unwrap();
        let _ = event_rx.recv().await;

        // Set frequency to cache some state
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Frequency { hz: 14_250_000 },
            })
            .await
            .unwrap();

        // Drain events
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        while event_rx.try_recv().is_ok() {}

        // Disconnect amplifier
        cmd_tx
            .send(MuxActorCommand::DisconnectAmplifier)
            .await
            .unwrap();
        let _ = event_rx.recv().await; // AmpDisconnected

        // Reconnect with a new channel
        let (amp_channel2, _resp_tx2, mut amp_rx2) =
            create_virtual_amp_channel(Protocol::Kenwood, None, 16);
        cmd_tx
            .send(MuxActorCommand::ConnectAmplifier {
                channel: amp_channel2,
            })
            .await
            .unwrap();
        let _ = event_rx.recv().await;

        // Query frequency - should have no cached state after disconnect/reconnect
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"FA;".to_vec(),
            })
            .await
            .unwrap();

        // Should not get a response (state was reset)
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        assert!(
            amp_rx2.try_recv().is_err(),
            "Should not have cached state after reconnect"
        );

        cmd_tx.send(MuxActorCommand::Shutdown).await.unwrap();
        actor_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_amp_id_query_responds_with_ts990s() {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = mpsc::channel(16);

        let actor_handle = tokio::spawn(run_mux_actor(cmd_rx, event_tx));

        // Connect an amplifier using the helper (no radio needed for ID query)
        let (amp_channel, _resp_tx, mut amp_rx) =
            create_virtual_amp_channel(Protocol::Kenwood, None, 16);
        cmd_tx
            .send(MuxActorCommand::ConnectAmplifier {
                channel: amp_channel,
            })
            .await
            .unwrap();

        // Drain the amp connected event
        let _ = event_rx.recv().await;

        // Send an ID query from the amp (ID;)
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"ID;".to_vec(),
            })
            .await
            .unwrap();

        // Should get AmpDataIn followed by AmpDataOut with ID022 response
        loop {
            let event = event_rx.recv().await.unwrap();
            if let MuxEvent::AmpDataOut { data, .. } = event {
                // Should be ID022; (TS-990S)
                let s = String::from_utf8_lossy(&data);
                assert_eq!(s, "ID022;", "Expected ID022; response, got: {}", s);
                break;
            }
        }

        // Verify amp received the response
        let amp_data = amp_rx.recv().await.unwrap();
        let s = String::from_utf8_lossy(&amp_data);
        assert_eq!(s, "ID022;");

        cmd_tx.send(MuxActorCommand::Shutdown).await.unwrap();
        actor_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_amp_cb_query_defaults_to_main() {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = mpsc::channel(16);

        let actor_handle = tokio::spawn(run_mux_actor(cmd_rx, event_tx));

        // Connect an amplifier
        let (amp_channel, _resp_tx, mut amp_rx) =
            create_virtual_amp_channel(Protocol::Kenwood, None, 16);
        cmd_tx
            .send(MuxActorCommand::ConnectAmplifier {
                channel: amp_channel,
            })
            .await
            .unwrap();
        let _ = event_rx.recv().await;

        // Send a CB query from the amp (CB;)
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"CB;".to_vec(),
            })
            .await
            .unwrap();

        // Should get CB0; response (default to main)
        loop {
            let event = event_rx.recv().await.unwrap();
            if let MuxEvent::AmpDataOut { data, .. } = event {
                let s = String::from_utf8_lossy(&data);
                assert_eq!(s, "CB0;", "Expected CB0; response, got: {}", s);
                break;
            }
        }

        let amp_data = amp_rx.recv().await.unwrap();
        assert_eq!(String::from_utf8_lossy(&amp_data), "CB0;");

        cmd_tx.send(MuxActorCommand::Shutdown).await.unwrap();
        actor_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_amp_tb_query_defaults_to_main() {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = mpsc::channel(16);

        let actor_handle = tokio::spawn(run_mux_actor(cmd_rx, event_tx));

        // Connect an amplifier
        let (amp_channel, _resp_tx, mut amp_rx) =
            create_virtual_amp_channel(Protocol::Kenwood, None, 16);
        cmd_tx
            .send(MuxActorCommand::ConnectAmplifier {
                channel: amp_channel,
            })
            .await
            .unwrap();
        let _ = event_rx.recv().await;

        // Send a TB query from the amp (TB;)
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"TB;".to_vec(),
            })
            .await
            .unwrap();

        // Should get TB0; response (default to main)
        loop {
            let event = event_rx.recv().await.unwrap();
            if let MuxEvent::AmpDataOut { data, .. } = event {
                let s = String::from_utf8_lossy(&data);
                assert_eq!(s, "TB0;", "Expected TB0; response, got: {}", s);
                break;
            }
        }

        let amp_data = amp_rx.recv().await.unwrap();
        assert_eq!(String::from_utf8_lossy(&amp_data), "TB0;");

        cmd_tx.send(MuxActorCommand::Shutdown).await.unwrap();
        actor_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_cb_tb_cached_from_active_radio() {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = mpsc::channel(16);

        let actor_handle = tokio::spawn(run_mux_actor(cmd_rx, event_tx));

        // Register a radio
        let meta =
            RadioChannelMeta::new_virtual("Test".to_string(), "sim".to_string(), Protocol::Kenwood);
        let (resp_tx, resp_rx) = oneshot::channel();
        cmd_tx
            .send(MuxActorCommand::RegisterRadio {
                meta,
                response: resp_tx,
                cmd_tx: None,
            })
            .await
            .unwrap();
        let handle = resp_rx.await.unwrap();
        let _ = event_rx.recv().await; // Drain connected event

        // Connect an amplifier
        let (amp_channel, _resp_tx, mut amp_rx) =
            create_virtual_amp_channel(Protocol::Kenwood, None, 16);
        cmd_tx
            .send(MuxActorCommand::ConnectAmplifier {
                channel: amp_channel,
            })
            .await
            .unwrap();
        let _ = event_rx.recv().await;

        // Set frequency to make the radio active
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Frequency { hz: 14_250_000 },
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        while event_rx.try_recv().is_ok() {}

        // Radio reports CB1 (Sub band selected)
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::ControlBand { band: 1 },
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Radio reports TB1 (Sub band for TX)
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::TransmitBand { band: 1 },
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Now amp queries CB; - should get CB1;
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"CB;".to_vec(),
            })
            .await
            .unwrap();

        loop {
            let event = event_rx.recv().await.unwrap();
            if let MuxEvent::AmpDataOut { data, .. } = event {
                let s = String::from_utf8_lossy(&data);
                assert_eq!(s, "CB1;", "Expected CB1; response, got: {}", s);
                break;
            }
        }

        // Verify amp received CB1
        let amp_data = amp_rx.recv().await.unwrap();
        assert_eq!(String::from_utf8_lossy(&amp_data), "CB1;");

        // Amp queries TB; - should get TB1;
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"TB;".to_vec(),
            })
            .await
            .unwrap();

        loop {
            let event = event_rx.recv().await.unwrap();
            if let MuxEvent::AmpDataOut { data, .. } = event {
                let s = String::from_utf8_lossy(&data);
                assert_eq!(s, "TB1;", "Expected TB1; response, got: {}", s);
                break;
            }
        }

        let amp_data = amp_rx.recv().await.unwrap();
        assert_eq!(String::from_utf8_lossy(&amp_data), "TB1;");

        cmd_tx.send(MuxActorCommand::Shutdown).await.unwrap();
        actor_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_vfo_a_infers_cb0_tb0() {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = mpsc::channel(16);

        let actor_handle = tokio::spawn(run_mux_actor(cmd_rx, event_tx));

        // Register a radio
        let meta =
            RadioChannelMeta::new_virtual("Test".to_string(), "sim".to_string(), Protocol::Kenwood);
        let (resp_tx, resp_rx) = oneshot::channel();
        cmd_tx
            .send(MuxActorCommand::RegisterRadio {
                meta,
                response: resp_tx,
                cmd_tx: None,
            })
            .await
            .unwrap();
        let handle = resp_rx.await.unwrap();
        let _ = event_rx.recv().await;

        // Connect an amplifier
        let (amp_channel, _resp_tx, mut amp_rx) =
            create_virtual_amp_channel(Protocol::Kenwood, None, 16);
        cmd_tx
            .send(MuxActorCommand::ConnectAmplifier {
                channel: amp_channel,
            })
            .await
            .unwrap();
        let _ = event_rx.recv().await;

        // Set frequency to make the radio active
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Frequency { hz: 14_250_000 },
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        while event_rx.try_recv().is_ok() {}

        // Radio reports VFO A selection
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Vfo {
                    vfo: cat_protocol::Vfo::A,
                },
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Amp queries CB; - should get CB0;
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"CB;".to_vec(),
            })
            .await
            .unwrap();

        loop {
            let event = event_rx.recv().await.unwrap();
            if let MuxEvent::AmpDataOut { data, .. } = event {
                let s = String::from_utf8_lossy(&data);
                assert_eq!(s, "CB0;", "Expected CB0; for VFO A, got: {}", s);
                break;
            }
        }
        let _ = amp_rx.recv().await.unwrap();

        // Amp queries TB; - should get TB0;
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"TB;".to_vec(),
            })
            .await
            .unwrap();

        loop {
            let event = event_rx.recv().await.unwrap();
            if let MuxEvent::AmpDataOut { data, .. } = event {
                let s = String::from_utf8_lossy(&data);
                assert_eq!(s, "TB0;", "Expected TB0; for VFO A, got: {}", s);
                break;
            }
        }

        cmd_tx.send(MuxActorCommand::Shutdown).await.unwrap();
        actor_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_vfo_b_infers_cb1_tb1() {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = mpsc::channel(16);

        let actor_handle = tokio::spawn(run_mux_actor(cmd_rx, event_tx));

        // Register a radio
        let meta =
            RadioChannelMeta::new_virtual("Test".to_string(), "sim".to_string(), Protocol::Kenwood);
        let (resp_tx, resp_rx) = oneshot::channel();
        cmd_tx
            .send(MuxActorCommand::RegisterRadio {
                meta,
                response: resp_tx,
                cmd_tx: None,
            })
            .await
            .unwrap();
        let handle = resp_rx.await.unwrap();
        let _ = event_rx.recv().await;

        // Connect an amplifier
        let (amp_channel, _resp_tx, mut amp_rx) =
            create_virtual_amp_channel(Protocol::Kenwood, None, 16);
        cmd_tx
            .send(MuxActorCommand::ConnectAmplifier {
                channel: amp_channel,
            })
            .await
            .unwrap();
        let _ = event_rx.recv().await;

        // Set frequency to make the radio active
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Frequency { hz: 14_250_000 },
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        while event_rx.try_recv().is_ok() {}

        // Radio reports VFO B selection
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Vfo {
                    vfo: cat_protocol::Vfo::B,
                },
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Amp queries CB; - should get CB1;
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"CB;".to_vec(),
            })
            .await
            .unwrap();

        loop {
            let event = event_rx.recv().await.unwrap();
            if let MuxEvent::AmpDataOut { data, .. } = event {
                let s = String::from_utf8_lossy(&data);
                assert_eq!(s, "CB1;", "Expected CB1; for VFO B, got: {}", s);
                break;
            }
        }
        let _ = amp_rx.recv().await.unwrap();

        // Amp queries TB; - should get TB1;
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"TB;".to_vec(),
            })
            .await
            .unwrap();

        loop {
            let event = event_rx.recv().await.unwrap();
            if let MuxEvent::AmpDataOut { data, .. } = event {
                let s = String::from_utf8_lossy(&data);
                assert_eq!(s, "TB1;", "Expected TB1; for VFO B, got: {}", s);
                break;
            }
        }

        cmd_tx.send(MuxActorCommand::Shutdown).await.unwrap();
        actor_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_vfo_a_then_split_infers_cb0_tb1() {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = mpsc::channel(16);

        let actor_handle = tokio::spawn(run_mux_actor(cmd_rx, event_tx));

        // Register a radio
        let meta =
            RadioChannelMeta::new_virtual("Test".to_string(), "sim".to_string(), Protocol::Kenwood);
        let (resp_tx, resp_rx) = oneshot::channel();
        cmd_tx
            .send(MuxActorCommand::RegisterRadio {
                meta,
                response: resp_tx,
                cmd_tx: None,
            })
            .await
            .unwrap();
        let handle = resp_rx.await.unwrap();
        let _ = event_rx.recv().await;

        // Connect an amplifier
        let (amp_channel, _resp_tx, mut amp_rx) =
            create_virtual_amp_channel(Protocol::Kenwood, None, 16);
        cmd_tx
            .send(MuxActorCommand::ConnectAmplifier {
                channel: amp_channel,
            })
            .await
            .unwrap();
        let _ = event_rx.recv().await;

        // Set frequency to make the radio active
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Frequency { hz: 14_250_000 },
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        while event_rx.try_recv().is_ok() {}

        // Radio reports VFO A selection first
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Vfo {
                    vfo: cat_protocol::Vfo::A,
                },
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Then radio enables split mode
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Vfo {
                    vfo: cat_protocol::Vfo::Split,
                },
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Amp queries CB; - should get CB0; (RX on A)
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"CB;".to_vec(),
            })
            .await
            .unwrap();

        loop {
            let event = event_rx.recv().await.unwrap();
            if let MuxEvent::AmpDataOut { data, .. } = event {
                let s = String::from_utf8_lossy(&data);
                assert_eq!(s, "CB0;", "Expected CB0; for VFO A + Split, got: {}", s);
                break;
            }
        }
        let _ = amp_rx.recv().await.unwrap();

        // Amp queries TB; - should get TB1; (TX on B, opposite of A)
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"TB;".to_vec(),
            })
            .await
            .unwrap();

        loop {
            let event = event_rx.recv().await.unwrap();
            if let MuxEvent::AmpDataOut { data, .. } = event {
                let s = String::from_utf8_lossy(&data);
                assert_eq!(s, "TB1;", "Expected TB1; for VFO A + Split, got: {}", s);
                break;
            }
        }

        cmd_tx.send(MuxActorCommand::Shutdown).await.unwrap();
        actor_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_vfo_b_then_split_infers_cb1_tb0() {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = mpsc::channel(16);

        let actor_handle = tokio::spawn(run_mux_actor(cmd_rx, event_tx));

        // Register a radio
        let meta =
            RadioChannelMeta::new_virtual("Test".to_string(), "sim".to_string(), Protocol::Kenwood);
        let (resp_tx, resp_rx) = oneshot::channel();
        cmd_tx
            .send(MuxActorCommand::RegisterRadio {
                meta,
                response: resp_tx,
                cmd_tx: None,
            })
            .await
            .unwrap();
        let handle = resp_rx.await.unwrap();
        let _ = event_rx.recv().await;

        // Connect an amplifier
        let (amp_channel, _resp_tx, mut amp_rx) =
            create_virtual_amp_channel(Protocol::Kenwood, None, 16);
        cmd_tx
            .send(MuxActorCommand::ConnectAmplifier {
                channel: amp_channel,
            })
            .await
            .unwrap();
        let _ = event_rx.recv().await;

        // Set frequency to make the radio active
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Frequency { hz: 14_250_000 },
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        while event_rx.try_recv().is_ok() {}

        // Radio reports VFO B selection first
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Vfo {
                    vfo: cat_protocol::Vfo::B,
                },
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Then radio enables split mode
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Vfo {
                    vfo: cat_protocol::Vfo::Split,
                },
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Amp queries CB; - should get CB1; (RX on B)
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"CB;".to_vec(),
            })
            .await
            .unwrap();

        loop {
            let event = event_rx.recv().await.unwrap();
            if let MuxEvent::AmpDataOut { data, .. } = event {
                let s = String::from_utf8_lossy(&data);
                assert_eq!(s, "CB1;", "Expected CB1; for VFO B + Split, got: {}", s);
                break;
            }
        }
        let _ = amp_rx.recv().await.unwrap();

        // Amp queries TB; - should get TB0; (TX on A, opposite of B)
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"TB;".to_vec(),
            })
            .await
            .unwrap();

        loop {
            let event = event_rx.recv().await.unwrap();
            if let MuxEvent::AmpDataOut { data, .. } = event {
                let s = String::from_utf8_lossy(&data);
                assert_eq!(s, "TB0;", "Expected TB0; for VFO B + Split, got: {}", s);
                break;
            }
        }

        cmd_tx.send(MuxActorCommand::Shutdown).await.unwrap();
        actor_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_split_then_vfo_a_clears_split() {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = mpsc::channel(16);

        let actor_handle = tokio::spawn(run_mux_actor(cmd_rx, event_tx));

        // Register a radio
        let meta =
            RadioChannelMeta::new_virtual("Test".to_string(), "sim".to_string(), Protocol::Kenwood);
        let (resp_tx, resp_rx) = oneshot::channel();
        cmd_tx
            .send(MuxActorCommand::RegisterRadio {
                meta,
                response: resp_tx,
                cmd_tx: None,
            })
            .await
            .unwrap();
        let handle = resp_rx.await.unwrap();
        let _ = event_rx.recv().await;

        // Connect an amplifier
        let (amp_channel, _resp_tx, mut amp_rx) =
            create_virtual_amp_channel(Protocol::Kenwood, None, 16);
        cmd_tx
            .send(MuxActorCommand::ConnectAmplifier {
                channel: amp_channel,
            })
            .await
            .unwrap();
        let _ = event_rx.recv().await;

        // Set frequency to make the radio active
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Frequency { hz: 14_250_000 },
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        while event_rx.try_recv().is_ok() {}

        // Radio selects VFO A, then enables split
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Vfo {
                    vfo: cat_protocol::Vfo::A,
                },
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Vfo {
                    vfo: cat_protocol::Vfo::Split,
                },
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Now radio selects VFO A again (exits split mode)
        cmd_tx
            .send(MuxActorCommand::RadioResponse {
                handle,
                response: RadioResponse::Vfo {
                    vfo: cat_protocol::Vfo::A,
                },
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Amp queries CB; - should get CB0;
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"CB;".to_vec(),
            })
            .await
            .unwrap();

        loop {
            let event = event_rx.recv().await.unwrap();
            if let MuxEvent::AmpDataOut { data, .. } = event {
                let s = String::from_utf8_lossy(&data);
                assert_eq!(s, "CB0;", "Expected CB0; after exiting split, got: {}", s);
                break;
            }
        }
        let _ = amp_rx.recv().await.unwrap();

        // Amp queries TB; - should get TB0; (split cleared, TX back to A)
        cmd_tx
            .send(MuxActorCommand::AmpRawData {
                data: b"TB;".to_vec(),
            })
            .await
            .unwrap();

        loop {
            let event = event_rx.recv().await.unwrap();
            if let MuxEvent::AmpDataOut { data, .. } = event {
                let s = String::from_utf8_lossy(&data);
                assert_eq!(s, "TB0;", "Expected TB0; after exiting split, got: {}", s);
                break;
            }
        }

        cmd_tx.send(MuxActorCommand::Shutdown).await.unwrap();
        actor_handle.await.unwrap();
    }
}
