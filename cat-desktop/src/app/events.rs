//! Event processing - handling mux events and background messages

use std::time::Instant;

use cat_mux::{MuxActorCommand, MuxEvent, RadioChannelMeta, RadioHandle};
use tokio::sync::oneshot;
use tracing::Level;

use crate::traffic_monitor::DiagnosticSeverity;

use super::{BackgroundMessage, CatapultApp};

impl CatapultApp {
    /// Process diagnostic events from tracing layer
    pub(super) fn process_diagnostic_events(&mut self) {
        while let Ok(event) = self.diag_rx.try_recv() {
            // Map tracing Level to DiagnosticSeverity
            let severity = match event.level {
                Level::ERROR => DiagnosticSeverity::Error,
                Level::WARN => DiagnosticSeverity::Warning,
                Level::INFO => DiagnosticSeverity::Info,
                // DEBUG and TRACE map to Debug
                _ => DiagnosticSeverity::Debug,
            };

            self.traffic_monitor
                .add_diagnostic(event.source, severity, event.message);
        }
    }

    /// Process background messages
    pub(super) fn process_messages(&mut self) {
        while let Ok(msg) = self.bg_rx.try_recv() {
            match msg {
                BackgroundMessage::ProbeComplete {
                    port,
                    baud_rate,
                    result,
                } => {
                    self.probing = false;
                    if port == self.add_radio_port {
                        if let Some(probe_result) = result {
                            // Update UI fields with probe result
                            // Note: protocol is NOT updated - user already selected it
                            if baud_rate > 0 {
                                self.add_radio_baud = baud_rate;
                            }
                            if let Some(addr) = probe_result.address {
                                self.add_radio_civ_address = addr;
                            }
                            // Set model name from detected model
                            self.add_radio_model = probe_result
                                .model
                                .map(|m| format!("{} {}", m.manufacturer, m.model))
                                .unwrap_or_else(|| {
                                    format!("{} radio", self.add_radio_protocol.name())
                                });
                            self.set_status(format!(
                                "Identified {} on {}",
                                self.add_radio_model, port
                            ));
                        } else {
                            self.set_status(format!(
                                "No response on {} using {} at {} baud. Check protocol, baud rate, and connection.",
                                port,
                                self.add_radio_protocol.name(),
                                self.add_radio_baud
                            ));
                        }
                    }
                }
                BackgroundMessage::RadioRegistered {
                    correlation_id,
                    handle,
                } => {
                    // Look up panel index from pending_registrations
                    if let Some(panel_idx) = self.pending_registrations.remove(&correlation_id) {
                        if let Some(panel) = self.radio_panels.get_mut(panel_idx) {
                            panel.handle = Some(handle);
                            tracing::info!("Radio registered: handle={:?}", handle);
                        }
                    }

                    // Spawn the connection task (unified for both COM and Virtual)
                    if let Some(config) = self.pending_radio_configs.remove(&correlation_id) {
                        self.spawn_radio_connection(handle, config);
                    }
                }
                BackgroundMessage::RadioConnected {
                    handle,
                    model,
                    port,
                } => {
                    // Update radio panel with actual model name and send rename to mux actor
                    if let Some(panel) = self
                        .radio_panels
                        .iter_mut()
                        .find(|p| p.handle == Some(handle))
                    {
                        panel.name = model.clone();
                        self.send_mux_command(
                            MuxActorCommand::UpdateRadioMeta {
                                handle,
                                name: Some(model.clone()),
                            },
                            "UpdateRadioMeta",
                        );
                    }

                    self.report_info("Radio", format!("Connected {} on {}", model, port));
                }
                BackgroundMessage::RadioStateSync { handle, state } => {
                    // Update RadioPanel from authoritative mux actor state
                    if let Some(panel) = self
                        .radio_panels
                        .iter_mut()
                        .find(|p| p.handle == Some(handle))
                    {
                        // Only update if different (avoid unnecessary changes)
                        if panel.frequency_hz != state.frequency_hz {
                            panel.frequency_hz = state.frequency_hz;
                        }
                        if panel.mode != state.mode {
                            panel.mode = state.mode;
                        }
                        if panel.ptt != state.ptt {
                            panel.ptt = state.ptt;
                        }
                    }
                }
            }
        }
    }

    /// Process events from the mux actor and update local state
    pub(super) fn process_mux_events(&mut self) {
        while let Ok(event) = self.mux_event_rx.try_recv() {
            match event {
                MuxEvent::RadioStateChanged {
                    handle,
                    freq,
                    mode,
                    ptt,
                } => {
                    // Update the RadioPanel's local state
                    if let Some(panel) = self
                        .radio_panels
                        .iter_mut()
                        .find(|p| p.handle == Some(handle))
                    {
                        if let Some(f) = freq {
                            panel.frequency_hz = Some(f);
                        }
                        if let Some(m) = mode {
                            panel.mode = Some(m);
                        }
                        if let Some(p) = ptt {
                            panel.ptt = p;
                        }

                        // Also update SimulationPanel for virtual radios
                        if let Some(sim_id) = panel.sim_id() {
                            self.simulation_panel
                                .update_radio_state(sim_id, freq, mode, ptt);
                        }
                    }
                }
                MuxEvent::ActiveRadioChanged { from: _, to } => {
                    self.active_radio = Some(to);
                }
                MuxEvent::SwitchingModeChanged { mode } => {
                    self.switching_mode = mode;
                }
                MuxEvent::RadioConnected { handle, meta } => {
                    tracing::debug!(
                        "MuxEvent::RadioConnected: handle={}, name={}",
                        handle.0,
                        meta.display_name
                    );
                }
                MuxEvent::RadioDisconnected { handle } => {
                    // Remove the task sender
                    self.radio_task_senders.remove(&handle);
                    tracing::debug!("MuxEvent::RadioDisconnected: handle={}", handle.0);
                }
                MuxEvent::Error { source, message } => {
                    self.report_err(&source, message);
                }
                MuxEvent::AmpConnected { meta: _ } => {
                    tracing::debug!("MuxEvent::AmpConnected");
                }
                MuxEvent::AmpDisconnected => {
                    tracing::debug!("MuxEvent::AmpDisconnected");
                    // Reset virtual amp state when disconnected
                    self.virtual_amp_state = None;
                    self.virtual_amp_cmd_tx = None;
                    self.virtual_amp_state_rx = None;
                }
                MuxEvent::SwitchingBlocked {
                    requested,
                    current,
                    remaining_ms,
                } => {
                    tracing::debug!(
                        "Switching blocked: requested={}, current={}, remaining={}ms",
                        requested.0,
                        current.0,
                        remaining_ms
                    );
                }
                // Traffic events - forward to traffic monitor
                MuxEvent::RadioDataIn { .. }
                | MuxEvent::RadioDataOut { .. }
                | MuxEvent::AmpDataOut { .. }
                | MuxEvent::AmpDataIn { .. } => {
                    self.forward_traffic_event(event);
                }
            }
        }
    }

    /// Process events from virtual amplifier actor (if connected)
    pub(super) fn process_virtual_amp_events(&mut self) {
        // Poll the broadcast channel for state updates
        if let Some(ref mut rx) = self.virtual_amp_state_rx {
            // Try to receive all pending events (non-blocking)
            loop {
                match rx.try_recv() {
                    Ok(event) => {
                        tracing::debug!(
                            "Virtual amp state: freq={}, mode={:?}, ptt={}",
                            event.frequency_hz,
                            event.mode,
                            event.ptt
                        );
                        self.virtual_amp_state = Some(event);
                    }
                    Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                        tracing::warn!("Virtual amp state receiver lagged by {} events", n);
                        // Continue to get the latest state
                    }
                    Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                        tracing::debug!("Virtual amp state channel closed");
                        self.virtual_amp_state_rx = None;
                        break;
                    }
                }
            }
        }
    }

    /// Periodically sync radio states from the mux actor (every 5 seconds)
    ///
    /// This ensures that the UI's RadioPanel state stays in sync with the
    /// authoritative state in the mux actor, even if events are dropped.
    pub(super) fn maybe_sync_radio_states(&mut self) {
        const SYNC_INTERVAL_SECS: u64 = 5;

        if self.last_state_sync.elapsed().as_secs() < SYNC_INTERVAL_SECS {
            return;
        }

        self.last_state_sync = Instant::now();

        // Query state for each radio panel that has a valid handle
        for panel in &self.radio_panels {
            let Some(handle) = panel.handle else {
                // No handle yet, not registered
                continue;
            };

            let (resp_tx, resp_rx) = oneshot::channel();

            self.send_mux_command(
                MuxActorCommand::QueryRadioState {
                    handle,
                    response: resp_tx,
                },
                "QueryRadioState",
            );

            // Spawn task to handle the response
            let bg_tx = self.bg_tx.clone();
            self.rt_handle.spawn(async move {
                if let Ok(Some(summary)) = resp_rx.await {
                    let _ = bg_tx.send(BackgroundMessage::RadioStateSync {
                        handle,
                        state: summary,
                    });
                }
            });
        }
    }

    /// Forward a traffic event to the traffic monitor
    pub(super) fn forward_traffic_event(&mut self, event: MuxEvent) {
        // Build radio metadata lookup from radio panels
        let radio_metas = |handle: RadioHandle| -> Option<RadioChannelMeta> {
            self.radio_panels
                .iter()
                .find(|p| p.handle == Some(handle))
                .map(|p| {
                    if p.is_virtual() {
                        RadioChannelMeta::new_virtual(
                            p.name.clone(),
                            p.sim_id().unwrap_or_default().to_string(),
                            p.protocol,
                        )
                    } else {
                        RadioChannelMeta::new_real(
                            p.name.clone(),
                            p.port.clone(),
                            p.protocol,
                            p.civ_address,
                        )
                    }
                })
        };
        let amp_port = self.amp_port.clone();
        self.traffic_monitor
            .process_event_with_amp_port(event, &radio_metas, &amp_port);
    }
}
